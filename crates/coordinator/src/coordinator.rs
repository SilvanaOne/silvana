use crate::config::Config;
use crate::constants::SHUTDOWN_TIMEOUT_SECS;
use crate::error::Result;
use crate::job_searcher::JobSearcher;
use crate::merge::{start_periodic_block_creation, start_periodic_proof_analysis};
use crate::metrics::{CoordinatorMetrics, start_metrics_reporter};
use crate::processor::EventProcessor;
use crate::state::SharedState;
// use crate::stuck_jobs::StuckJobMonitor; // Removed - reconciliation handles stuck jobs
use std::time::Duration;
use tokio::signal;
use tokio::task;
use tracing::{debug, error, info, warn};

pub async fn start_coordinator(
    rpc_url: String,
    package_id: String,
    use_tee: bool,
    container_timeout: u64,
    grpc_socket_path: String,
    app_instance_filter: Option<String>,
    settle_only: bool,
) -> Result<()> {
    info!("🚀 Starting Silvana Coordinator");
    info!("🔗 Sui RPC URL: {}", rpc_url);
    info!("📦 Monitoring package: {}", package_id);
    
    if let Some(ref instance) = app_instance_filter {
        info!("🎯 Filtering jobs for app instance: {}", instance);
    }
    
    if settle_only {
        info!("⚖️ Running as dedicated settlement node");
    }

    // Initialize the global SharedSuiState
    sui::SharedSuiState::initialize(&rpc_url).await?;

    // Get network and address info
    let env_network = sui::get_network_name();
    let address = sui::get_current_address();

    info!("✅ Connected to Sui RPC");

    // Verify chain configuration and get actual chain name
    match sui::network_info::get_service_info_full().await {
        Ok(service_info) => {
            // Use RPC chain name if provided and not "unknown", otherwise use env
            let network = service_info.chain_name.as_ref().unwrap_or(&env_network);

            info!("🌐 Network: {}", network);

            if let Some(id) = service_info.chain_id {
                debug!("Chain ID: {}", id);
            }

            if let Some(version) = service_info.server_version {
                debug!("Server: {}", version);
            }

            if let Some(height) = service_info.checkpoint_height {
                debug!("Checkpoint height: {}", height);
            }
        }
        Err(e) => {
            debug!("Could not get service info: {}", e);
            info!("🌐 Network: {} (from env)", env_network);
        }
    }

    info!("👤 Address: {}", address);

    // Try to get and display network stats
    match sui::get_network_summary().await {
        Ok(summary) => info!("📊 Network Stats: {}", summary),
        Err(e) => debug!("Could not fetch network stats: {}", e),
    }

    // Check balance and request from faucet if needed
    info!("🚰 Checking balance and faucet availability...");
    match sui::faucet::initialize_faucet().await {
        Ok(()) => info!("✅ Balance check complete"),
        Err(e) => warn!(
            "⚠️ Failed to check/request faucet tokens: {}. Continuing with existing balance.",
            e
        ),
    }

    // Initialize gas coin pool for better parallel transaction performance
    info!("🪙 Initializing gas coin pool...");
    match sui::coin_management::initialize_gas_coin_pool().await {
        Ok(()) => info!("✅ Gas coin pool initialized"),
        Err(e) => warn!(
            "⚠️ Failed to initialize gas coin pool: {}. System will continue with existing coins.",
            e
        ),
    }

    let config = Config {
        package_id,
        modules: vec!["jobs".to_string()],
    };

    // Create shared state
    let state = SharedState::new();
    
    // Set the settle_only flag
    state.set_settle_only(settle_only);
    
    // Set the app instance filter if provided
    if let Some(instance) = app_instance_filter {
        state.set_app_instance_filter(Some(instance)).await;
    }

    // Setup signal handlers for graceful shutdown (Ctrl-C and SIGTERM for system reboot)
    let shutdown_state = state.clone();
    let force_shutdown_state = state.clone();
    tokio::spawn(async move {
        let mut sigint = signal::unix::signal(signal::unix::SignalKind::interrupt())
            .expect("Failed to create SIGINT handler");
        let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to create SIGTERM handler");

        tokio::select! {
            _ = sigint.recv() => {
                if shutdown_state.is_shutting_down() {
                    // Second Ctrl-C - force shutdown
                    error!("🛑 Second SIGINT received - forcing immediate shutdown!");
                    shutdown_state.set_force_shutdown();
                } else {
                    // First Ctrl-C - graceful shutdown
                    warn!("⚠️  Received SIGINT (Ctrl-C), initiating graceful shutdown...");
                    warn!("    Press Ctrl-C again to force immediate shutdown");
                    shutdown_state.set_shutdown();

                    // Spawn a task to listen for second Ctrl-C
                    tokio::spawn(async move {
                        let mut sigint2 = signal::unix::signal(signal::unix::SignalKind::interrupt())
                            .expect("Failed to create second SIGINT handler");
                        sigint2.recv().await;
                        error!("🛑 Second SIGINT received - forcing immediate shutdown!");
                        force_shutdown_state.set_force_shutdown();
                    });
                }
            }
            _ = sigterm.recv() => {
                warn!("⚠️  Received SIGTERM (system shutdown/reboot), initiating graceful shutdown...");
                shutdown_state.set_shutdown();
            }
        }
    });

    info!("✅ Coordinator initialized, starting services...");

    // 1. Start gRPC server in a separate thread
    let grpc_state = state.clone();
    let grpc_handle = task::spawn(async move {
        info!("🔌 Starting gRPC server...");
        if let Err(e) = crate::grpc::start_grpc_server(&grpc_socket_path, grpc_state).await {
            error!("gRPC server error: {}", e);
        }
    });

    // 2. Start reconciliation task in a separate thread (runs every 10 minutes)
    let reconciliation_state = state.clone();

    let reconciliation_handle = task::spawn(async move {
        // TODO: Change back to 10 minutes (600 seconds) after testing stuck job handling
        info!("🔄 Starting reconciliation task (runs every 10 minutes for testing)...");

        // Run immediate reconciliation on startup to catch stuck jobs
        info!("🔍 Running immediate reconciliation check on startup...");
        tokio::time::sleep(Duration::from_secs(60)).await; // Small delay to let the system initialize

        match reconciliation_state
            .get_jobs_tracker()
            .reconcile_with_chain()
            .await
        {
            Ok(has_pending) => {
                if has_pending {
                    info!("✅ Initial reconciliation complete - found pending jobs");
                } else {
                    info!("✅ Initial reconciliation complete - no pending jobs");
                }
            }
            Err(e) => {
                error!("❌ Initial reconciliation failed: {}", e);
            }
        }

        let mut interval = tokio::time::interval(Duration::from_secs(600)); // 10 minutes for testing

        // Skip the immediate first tick since we just ran reconciliation
        interval.tick().await;

        loop {
            // Check for shutdown
            if reconciliation_state.is_shutting_down() {
                info!("Reconciliation task shutting down...");
                break;
            }

            interval.tick().await;

            if reconciliation_state.is_shutting_down() {
                break;
            }

            info!("📊 Running periodic reconciliation check...");

            // Log current stats before reconciliation
            let stats = reconciliation_state.get_jobs_tracker().get_stats().await;
            debug!(
                "Starting periodic reconciliation (currently tracking {} app_instances, {} agent methods)",
                stats.app_instances_count, stats.agent_methods_count
            );

            // Run reconciliation with chain
            match reconciliation_state
                .get_jobs_tracker()
                .reconcile_with_chain()
                .await
            {
                Ok(has_pending) => {
                    if has_pending {
                        debug!("Reconciliation complete - still have pending jobs");
                    } else {
                        debug!("Reconciliation complete - no pending jobs");
                    }
                    // Always refresh the has_pending_jobs flag so the job_searcher
                    // is notified when pending jobs exist after reconciliation.
                    reconciliation_state.update_pending_jobs_flag().await;
                }
                Err(e) => {
                    error!("Failed to reconcile with chain: {}", e);
                }
            }

            info!("✅ Reconciliation check complete");
        }
    });
    info!("🔄 Started reconciliation task (runs every 1 minute for testing)"); // TODO: Change back to 10 minutes

    // Note: Stuck job monitoring is handled by reconciliation task (every 10 minutes)
    // to avoid overloading Sui nodes with duplicate checks

    // 4. Start periodic block creation task (runs every minute)
    let block_creation_state = state.clone();
    let block_creation_handle = task::spawn(async move {
        start_periodic_block_creation(block_creation_state).await;
    });
    info!("📦 Started periodic block creation task (runs every minute)");

    // 4.5 Start periodic proof analysis task (runs every 5 minutes)
    let proof_analysis_state = state.clone();
    let proof_analysis_handle = task::spawn(async move {
        start_periodic_proof_analysis(proof_analysis_state).await;
    });
    info!("🔬 Started proof completion analysis task (runs every 5 minutes)");

    // 5. Start metrics reporter for New Relic
    let metrics = std::sync::Arc::new(CoordinatorMetrics::new());
    let metrics_state = state.clone();
    let metrics_clone = metrics.clone();
    let jobs_tracker_clone = state.get_jobs_tracker().clone();
    let _metrics_handle = task::spawn(async move {
        start_metrics_reporter(metrics_state, metrics_clone, jobs_tracker_clone).await;
    });
    info!("📊 Started New Relic metrics reporter");

    // 6. Start periodic balance check task (runs every 30 minutes)
    let balance_check_handle = task::spawn(async move {
        info!("💰 Starting periodic balance check task (runs every 30 minutes)...");
        let mut interval = tokio::time::interval(Duration::from_secs(1800)); // 30 minutes

        // Skip the immediate first tick since we already checked on startup
        interval.tick().await;

        loop {
            interval.tick().await;

            info!("💰 Running periodic balance check...");

            // Check balance and request from faucet if below 5 SUI (topup early to avoid running out)
            match sui::faucet::ensure_sufficient_balance(10.0).await {
                Ok(requested) => {
                    if requested {
                        info!("✅ Balance was low, requested tokens from faucet");
                    } else {
                        debug!("✅ Balance check complete - sufficient balance available");
                    }
                }
                Err(e) => {
                    warn!("⚠️ Failed to check/request faucet tokens: {}", e);
                }
            }

            // Also check if we need to split coins for the pool
            if let Err(e) = sui::coin_management::ensure_gas_coin_pool().await {
                warn!("⚠️ Failed to maintain gas coin pool: {}", e);
            }
        }
    });
    info!("💰 Started periodic balance check task (runs every 30 minutes)");

    // 6. Start job searcher in a separate thread
    let job_searcher_state = state.clone();
    let job_searcher_metrics = metrics.clone();
    let job_searcher_handle = task::spawn(async move {
        let mut job_searcher =
            match JobSearcher::new(job_searcher_state, use_tee, container_timeout) {
                Ok(searcher) => searcher,
                Err(e) => {
                    error!("Failed to create job searcher: {}", e);
                    return;
                }
            };
        
        // Set metrics reference
        job_searcher.set_metrics(job_searcher_metrics);

        if let Err(e) = job_searcher.run().await {
            error!("Job searcher error: {}", e);
        }
    });
    info!("🔍 Started job searcher thread");

    // 7. Start event processor in main thread (processes events and updates shared state)
    let mut processor = EventProcessor::new(config, state.clone()).await?;
    processor.set_metrics(metrics.clone());
    info!("👁️ Starting event monitoring...");

    // Run processor in a task so we can monitor shutdown
    let processor_handle = task::spawn(async move { processor.run().await });

    // Monitor for shutdown or processor exit
    loop {
        if state.is_shutting_down() {
            info!("🛑 Shutdown requested, starting graceful shutdown sequence...");
            break;
        }

        // Check if processor exited
        if processor_handle.is_finished() {
            match processor_handle.await {
                Ok(processor_result) => {
                    if let Err(e) = processor_result {
                        error!("Fatal error in event processor: {}", e);
                        return Err(e.into());
                    }
                }
                Err(e) => {
                    error!("Processor task panicked: {}", e);
                    return Err(anyhow::anyhow!("Processor task panicked: {}", e).into());
                }
            }
            break;
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    info!("🔄 Starting graceful shutdown sequence...");

    // Phase 1: Stop accepting new jobs (already done by setting shutdown flag)
    info!("  1️⃣ Stopping new job acceptance...");

    // Phase 2: Wait for current jobs to complete (with timeout)
    info!("  2️⃣ Waiting for current jobs to complete (max {} minutes)...", SHUTDOWN_TIMEOUT_SECS / 60);
    let mut wait_time = 0;
    let max_wait = SHUTDOWN_TIMEOUT_SECS;

    while wait_time < max_wait {
        // Check for force shutdown
        if state.is_force_shutdown() {
            error!("  🛑 Force shutdown requested!");
            break;
        }

        let current_agents = state.get_current_agent_count().await;
        if current_agents == 0 {
            info!("  ✅ All jobs completed");
            break;
        }

        // Show progress every 10 seconds
        if wait_time % 10 == 0 {
            if wait_time < 60 {
                info!(
                    "  ⏳ {} agents still running, waiting... ({}/{}s)",
                    current_agents, wait_time, max_wait
                );
            } else {
                let minutes = wait_time / 60;
                let seconds = wait_time % 60;
                info!(
                    "  ⏳ {} agents still running, waiting... ({}m {}s / 5m)",
                    current_agents, minutes, seconds
                );
            }
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
        wait_time += 1;
    }

    // Phase 3: If jobs are still running, try to get their status and logs
    let remaining_agents = state.get_current_agent_count().await;
    if remaining_agents > 0 {
        if state.is_force_shutdown() {
            error!(
                "  🛑 Force shutdown - terminating {} running agents...",
                remaining_agents
            );
        } else {
            warn!(
                "  ⚠️ {} agents still running after timeout, collecting logs...",
                remaining_agents
            );
        }

        // Get information about running agents
        let running_agents = state.get_all_current_agents().await;

        // Try to get Docker container logs and terminate if force shutdown
        let docker_manager = match docker::DockerManager::new(false) {
            Ok(dm) => Some(dm),
            Err(e) => {
                error!("Failed to create Docker manager for shutdown: {}", e);
                None
            }
        };

        for (session_id, agent_info) in running_agents {
            warn!(
                "    - Session {}: {}/{}/{}",
                session_id, agent_info.developer, agent_info.agent, agent_info.agent_method
            );
        }

        // Try to get container logs and optionally stop them
        if let Some(ref dm) = docker_manager {
            info!("  📋 Fetching logs from running Docker containers...");
            let container_results = dm
                .fetch_and_stop_silvana_containers(state.is_force_shutdown())
                .await;

            for (container_id, container_name, logs) in container_results {
                info!(
                    "  📦 Container: {} ({})",
                    &container_id[..12.min(container_id.len())],
                    container_name
                );
                if !logs.is_empty() {
                    // Print full container logs during shutdown
                    info!("  📄 Container logs:\n{}", logs);
                } else {
                    info!("  ⚠️ No logs available from container");
                }
            }
        }
    }

    // Phase 4: Cancel all background tasks
    info!("  3️⃣ Stopping background tasks...");
    grpc_handle.abort();
    reconciliation_handle.abort();
    block_creation_handle.abort();
    proof_analysis_handle.abort();
    balance_check_handle.abort();

    // Give job_searcher a chance to cleanup
    if !job_searcher_handle.is_finished() {
        // Wait a bit for job_searcher to finish gracefully
        let _ = tokio::time::timeout(Duration::from_secs(5), job_searcher_handle).await;
    }

    info!("✅ Graceful shutdown complete");
    Ok(())
}
