use crate::config::Config;
use crate::constants::{
    BALANCE_CHECK_INTERVAL_SECS, COORDINATOR_ACTIVE_EVENT_INTERVAL_SECS,
    GRACEFUL_SHUTDOWN_TIMEOUT_SECS, JOB_SEARCHER_SHUTDOWN_TIMEOUT_SECS,
    PROOF_ANALYSIS_INTERVAL_SECS, RECONCILIATION_INTERVAL_SECS, SHUTDOWN_CLEANUP_DELAY_MS,
    SHUTDOWN_PROGRESS_INTERVAL_SECS, STARTUP_RECONCILIATION_DELAY_SECS,
};
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
        let mut sigint = match signal::unix::signal(signal::unix::SignalKind::interrupt()) {
            Ok(sig) => sig,
            Err(e) => {
                error!(
                    "Failed to create SIGINT handler: {}. Graceful shutdown disabled.",
                    e
                );
                return;
            }
        };
        let mut sigterm = match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(sig) => sig,
            Err(e) => {
                error!(
                    "Failed to create SIGTERM handler: {}. Graceful shutdown disabled.",
                    e
                );
                return;
            }
        };

        tokio::select! {
            _ = sigint.recv() => {
                if shutdown_state.is_shutting_down() {
                    // Second Ctrl-C - force shutdown
                    eprintln!("🛑 Second SIGINT received - forcing immediate shutdown!");
                    error!("🛑 Second SIGINT received - forcing immediate shutdown!");
                    shutdown_state.set_force_shutdown();
                } else {
                    // First Ctrl-C - graceful shutdown
                    eprintln!("⚠️  Received SIGINT (Ctrl-C), initiating graceful shutdown...");
                    eprintln!("    Press Ctrl-C again to force immediate shutdown");
                    warn!("⚠️  Received SIGINT (Ctrl-C), initiating graceful shutdown...");
                    warn!("    Press Ctrl-C again to force immediate shutdown");
                    shutdown_state.set_shutdown();

                    // Spawn a task to listen for second Ctrl-C
                    tokio::spawn(async move {
                        match signal::unix::signal(signal::unix::SignalKind::interrupt()) {
                            Ok(mut sigint2) => {
                                sigint2.recv().await;
                                eprintln!("🛑 Second SIGINT received - forcing immediate shutdown!");
                                error!("🛑 Second SIGINT received - forcing immediate shutdown!");
                                force_shutdown_state.set_force_shutdown();
                            }
                            Err(e) => {
                                error!("Failed to create second SIGINT handler: {}", e);
                            }
                        }
                    });
                }
            }
            _ = sigterm.recv() => {
                eprintln!("⚠️  Received SIGTERM (system shutdown/reboot), initiating graceful shutdown...");
                warn!("⚠️  Received SIGTERM (system shutdown/reboot), initiating graceful shutdown...");
                shutdown_state.set_shutdown();
            }
        }
    });

    info!("✅ Coordinator initialized, starting services...");

    // Send coordinator started event
    let coordinator_id = sui::get_current_address();
    let ethereum_address = std::env::var("ETHEREUM_ADDRESS")
        .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".to_string());

    // Send coordinator started event
    {
        let rpc_client = state.get_rpc_client().await;
        if let Some(mut client) = rpc_client {
            let event = proto::Event {
                event: Some(proto::event::Event::CoordinatorStarted(
                    proto::CoordinatorStartedEvent {
                        coordinator_id: coordinator_id.clone(),
                        ethereum_address: ethereum_address.clone(),
                        event_timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                    },
                )),
            };

            let request = proto::SubmitEventRequest { event: Some(event) };

            match client.submit_event(request).await {
                Ok(response) => {
                    let resp = response.into_inner();
                    if resp.success {
                        info!("✅ Sent coordinator started event");
                    } else {
                        warn!(
                            "⚠️ Failed to send coordinator started event: {}",
                            resp.message
                        );
                    }
                }
                Err(e) => {
                    warn!("⚠️ Failed to send coordinator started event: {}", e);
                }
            }
        } else {
            warn!("⚠️ RPC client not available, skipping coordinator started event");
        }
    }

    // Start periodic coordinator active events
    let active_state = state.clone();
    let active_coordinator_id = coordinator_id.clone();
    let active_handle = task::spawn(async move {
        info!(
            "📡 Starting coordinator active event task (sends every {} minutes)...",
            COORDINATOR_ACTIVE_EVENT_INTERVAL_SECS / 60
        );

        let mut interval =
            tokio::time::interval(Duration::from_secs(COORDINATOR_ACTIVE_EVENT_INTERVAL_SECS));
        interval.tick().await; // Skip the first immediate tick

        loop {
            if active_state.is_shutting_down() {
                info!("Coordinator active event task shutting down");
                break;
            }

            interval.tick().await;

            let rpc_client = active_state.get_rpc_client().await;
            if let Some(mut client) = rpc_client {
                let event = proto::Event {
                    event: Some(proto::event::Event::CoordinatorActive(
                        proto::CoordinatorActiveEvent {
                            coordinator_id: active_coordinator_id.clone(),
                            event_timestamp: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_secs(),
                        },
                    )),
                };

                let request = proto::SubmitEventRequest { event: Some(event) };

                match client.submit_event(request).await {
                    Ok(response) => {
                        let resp = response.into_inner();
                        if resp.success {
                            debug!("Sent coordinator active event");
                        } else {
                            warn!("Failed to send coordinator active event: {}", resp.message);
                        }
                    }
                    Err(e) => {
                        warn!("Failed to send coordinator active event: {}", e);
                    }
                }
            }
        }
    });

    // 1. Start gRPC server in a separate thread
    let grpc_state = state.clone();
    let grpc_handle = task::spawn(async move {
        info!("🔌 Starting gRPC server...");
        if let Err(e) = crate::grpc::start_grpc_server(&grpc_socket_path, grpc_state).await {
            error!("gRPC server error: {}", e);
        }
    });

    // 2. Start reconciliation task in a separate thread
    let reconciliation_state = state.clone();

    let reconciliation_handle = task::spawn(async move {
        info!(
            "🔄 Starting reconciliation task (runs every {} minutes)...",
            RECONCILIATION_INTERVAL_SECS / 60
        );

        // Run immediate reconciliation on startup to catch stuck jobs
        info!("🔍 Running immediate reconciliation check on startup...");
        tokio::time::sleep(Duration::from_secs(STARTUP_RECONCILIATION_DELAY_SECS)).await; // Small delay to let the system initialize

        match reconciliation_state
            .get_jobs_tracker()
            .reconcile_with_chain(|app_instance: String, job_sequence: u64, error: String| {
                let state = reconciliation_state.clone();
                Box::pin(async move {
                    state
                        .add_fail_job_request(app_instance, job_sequence, error)
                        .await;
                })
            })
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

        let mut interval = tokio::time::interval(Duration::from_secs(RECONCILIATION_INTERVAL_SECS));

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
                .reconcile_with_chain(|app_instance: String, job_sequence: u64, error: String| {
                    let state = reconciliation_state.clone();
                    Box::pin(async move {
                        state
                            .add_fail_job_request(app_instance, job_sequence, error)
                            .await;
                    })
                })
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
    info!(
        "🔄 Started reconciliation task (runs every {} minutes)",
        RECONCILIATION_INTERVAL_SECS / 60
    );

    // Note: Stuck job monitoring is handled by reconciliation task
    // to avoid overloading Sui nodes with duplicate checks

    // 4. Start periodic block creation task (runs every minute)
    let block_creation_state = state.clone();
    let block_creation_handle = task::spawn(async move {
        start_periodic_block_creation(block_creation_state).await;
    });
    info!("📦 Started periodic block creation task (runs every minute)");

    // 4.5 Start periodic proof analysis task
    let proof_analysis_state = state.clone();
    let proof_analysis_handle = task::spawn(async move {
        start_periodic_proof_analysis(proof_analysis_state).await;
    });
    info!(
        "🔬 Started proof completion analysis task (runs every {} minutes)",
        PROOF_ANALYSIS_INTERVAL_SECS / 60
    );

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
        let mut interval = tokio::time::interval(Duration::from_secs(BALANCE_CHECK_INTERVAL_SECS));

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

    // 6. Start job searcher in a separate thread (only collects jobs and adds to multicall queue)
    let job_searcher_state = state.clone();
    let job_searcher_metrics = metrics.clone();
    let job_searcher_handle = task::spawn(async move {
        let mut job_searcher = match JobSearcher::new(job_searcher_state) {
            Ok(searcher) => searcher,
            Err(e) => {
                error!("Failed to create job searcher: {}", e);
                return;
            }
        };

        // Set metrics reference
        job_searcher.set_metrics(job_searcher_metrics);

        // Delay job searcher startup by 10 seconds to allow system initialization
        info!("Job searcher waiting 10 seconds before starting...");
        tokio::time::sleep(Duration::from_secs(10)).await;
        info!("Job searcher starting now");

        if let Err(e) = job_searcher.run().await {
            error!("Job searcher error: {}", e);
        }
    });
    info!("🔍 Started job searcher thread");

    // 7. Start multicall processor in a separate thread (processes multicall queue and executes batches)
    let multicall_state = state.clone();
    let multicall_metrics = metrics.clone();
    let multicall_handle = task::spawn(async move {
        let mut multicall_processor = crate::multicall::MulticallProcessor::new(multicall_state);
        multicall_processor.set_metrics(multicall_metrics);

        // Delay multicall processor startup by 5 seconds
        info!("Multicall processor waiting 5 seconds before starting...");
        tokio::time::sleep(Duration::from_secs(5)).await;
        info!("Multicall processor starting now");

        if let Err(e) = multicall_processor.run().await {
            error!("Multicall processor error: {}", e);
        }
    });
    info!("🚀 Started multicall processor thread");

    // 8. Start docker buffer processor in a separate thread (processes started jobs buffer and launches containers)
    let docker_state = state.clone();
    let docker_metrics = metrics.clone();
    let docker_handle = task::spawn(async move {
        let mut docker_processor = match crate::docker::DockerBufferProcessor::new(
            docker_state,
            use_tee,
            container_timeout,
        ) {
            Ok(processor) => processor,
            Err(e) => {
                error!("Failed to create docker buffer processor: {}", e);
                return;
            }
        };
        docker_processor.set_metrics(docker_metrics);

        // Delay docker processor startup by 8 seconds
        info!("Docker buffer processor waiting 8 seconds before starting...");
        tokio::time::sleep(Duration::from_secs(8)).await;
        info!("Docker buffer processor starting now");

        if let Err(e) = docker_processor.run().await {
            error!("Docker buffer processor error: {}", e);
        }
    });
    info!("🐳 Started docker buffer processor thread");

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

        tokio::time::sleep(Duration::from_millis(SHUTDOWN_CLEANUP_DELAY_MS)).await;
    }

    info!("🔄 Starting graceful shutdown sequence...");
    info!("    Shutdown order:");
    info!("    1️⃣ Stop searching for new jobs");
    info!("    2️⃣ Process all buffered jobs and wait for running containers");
    info!("    3️⃣ Send all pending multicall operations to blockchain");
    info!(
        "    ⏱️  Maximum wait time: {} minutes",
        GRACEFUL_SHUTDOWN_TIMEOUT_SECS / 60
    );
    let mut wait_time = 0;
    let max_wait = GRACEFUL_SHUTDOWN_TIMEOUT_SECS;

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

        // Show progress periodically
        if wait_time % SHUTDOWN_PROGRESS_INTERVAL_SECS == 0 {
            if wait_time < 60 {
                info!(
                    "  ⏳ {} agents still running, waiting... ({}/{}s)",
                    current_agents, wait_time, max_wait
                );
            } else {
                let minutes = wait_time / 60;
                let seconds = wait_time % 60;
                let max_minutes = GRACEFUL_SHUTDOWN_TIMEOUT_SECS / 60;
                info!(
                    "  ⏳ {} agents still running, waiting... ({}m {}s / {}m)",
                    current_agents, minutes, seconds, max_minutes
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

    // Phase 4: Cancel non-critical background tasks (but keep gRPC running for containers)
    info!("  3️⃣ Stopping non-critical background tasks...");
    // Don't abort gRPC yet - containers need it to communicate
    reconciliation_handle.abort();
    block_creation_handle.abort();
    proof_analysis_handle.abort();
    balance_check_handle.abort();
    active_handle.abort();

    // Shutdown sequence:
    // 1. Job searcher stops immediately (no new jobs)
    // 2. Docker processor processes all buffered jobs and waits for containers
    // 3. Multicall processor continues until docker is done, then processes final operations
    // 4. gRPC server stays running until Docker containers complete

    // Job searcher should stop immediately
    if !job_searcher_handle.is_finished() {
        info!("  ⏳ Waiting for job searcher to stop...");
        let _ = tokio::time::timeout(
            Duration::from_secs(JOB_SEARCHER_SHUTDOWN_TIMEOUT_SECS),
            job_searcher_handle,
        )
        .await;
        info!("  ✅ Job searcher stopped (no new jobs will be searched)");
    }

    // Docker and Multicall processors work together
    // Docker processes buffered jobs and waits for containers
    // Multicall continues processing requests from containers
    let mut docker_done = false;
    let mut multicall_done = false;
    let shutdown_start = tokio::time::Instant::now();

    while (!docker_done || !multicall_done)
        && shutdown_start.elapsed().as_secs() < GRACEFUL_SHUTDOWN_TIMEOUT_SECS
    {
        // Check docker status
        if !docker_done && docker_handle.is_finished() {
            docker_done = true;
            info!("  ✅ Docker processor completed all jobs and containers");
        }

        // Check multicall status
        if !multicall_done && multicall_handle.is_finished() {
            multicall_done = true;
            info!("  ✅ Multicall processor completed all operations");
        }

        // Show progress
        if !docker_done || !multicall_done {
            let buffer_size = state.get_started_jobs_buffer_size().await;
            let current_agents = state.get_current_agent_count().await;
            let pending_ops = state.get_total_operations_count().await;

            if shutdown_start.elapsed().as_secs() % 5 == 0 {
                info!(
                    "  ⏳ Shutdown progress: {} buffered jobs, {} containers, {} pending operations",
                    buffer_size, current_agents, pending_ops
                );
            }
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    // Force abort if timeout
    if !docker_done {
        warn!("  ⚠️ Docker processor timeout - forcing shutdown");
        docker_handle.abort();
    }
    if !multicall_done {
        warn!("  ⚠️ Multicall processor timeout - forcing shutdown");
        multicall_handle.abort();
    }

    // Now that Docker containers are done, we can safely stop the gRPC server
    info!("  ⏳ Stopping gRPC server...");
    grpc_handle.abort();
    info!("  ✅ gRPC server stopped");

    // Send coordinator shutdown event
    {
        let rpc_client = state.get_rpc_client().await;
        if let Some(mut client) = rpc_client {
            let event = proto::Event {
                event: Some(proto::event::Event::CoordinatorShutdown(
                    proto::CoordinatorShutdownEvent {
                        coordinator_id: coordinator_id.clone(),
                        event_timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                    },
                )),
            };

            let request = proto::SubmitEventRequest { event: Some(event) };

            match client.submit_event(request).await {
                Ok(response) => {
                    let resp = response.into_inner();
                    if resp.success {
                        info!("✅ Sent coordinator shutdown event");
                    } else {
                        warn!(
                            "⚠️ Failed to send coordinator shutdown event: {}",
                            resp.message
                        );
                    }
                }
                Err(e) => {
                    warn!("⚠️ Failed to send coordinator shutdown event: {}", e);
                }
            }
        }
    }

    info!("✅ Graceful shutdown complete");
    println!("✅ Graceful shutdown complete");
    Ok(())
}
