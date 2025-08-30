mod agent;
mod block;
mod config;
mod error;
mod events;
mod failed_jobs_cache;
mod grpc;
mod hardware;
mod job_id;
mod job_searcher;
mod jobs;
mod merge;
mod processor;
mod proof;
mod session_id;
mod settlement;
mod state;

use anyhow::Result;
use clap::Parser;
use dotenvy::dotenv;
use tokio::signal;
use tokio::task;
use tokio::time::Duration;
use tracing::{debug, info, warn, error};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::Config;
use sui::fetch::fetch_app_instance;
use crate::proof::analyze_proof_completion;
use crate::job_searcher::JobSearcher;
use crate::block::try_create_block;
use sui::interface::SilvanaSuiInterface;
use crate::processor::EventProcessor;
use crate::state::SharedState;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long, env = "SUI_RPC_URL")]
    rpc_url: String,

    #[arg(long, env = "SILVANA_REGISTRY_PACKAGE")]
    package_id: String,

    #[arg(long, env = "DOCKER_USE_TEE", default_value = "false")]
    use_tee: bool,

    #[arg(long, env = "CONTAINER_TIMEOUT_SECS", default_value = "900")]
    container_timeout: u64,

    #[arg(long, env = "LOG_LEVEL", default_value = "info")]
    log_level: String,

    #[arg(long, env = "GRPC_SOCKET_PATH", default_value = "/tmp/coordinator.sock")]
    grpc_socket_path: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file from current directory
    dotenv().ok();

    let args = Args::parse();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| args.log_level.clone().into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    info!("🚀 Starting Silvana Coordinator");
    info!("🔗 Sui RPC URL: {}", args.rpc_url);
    info!("📦 Monitoring package: {}", args.package_id);
    

    // Initialize the global SharedSuiState
    sui::SharedSuiState::initialize(&args.rpc_url).await?;
    info!("✅ Connected to Sui RPC");

    let config = Config {
        package_id: args.package_id,
        modules: vec!["jobs".to_string()],
    };

    // Create shared state
    let state = SharedState::new();
    
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
                    error!("\n🛑 Second SIGINT received - forcing immediate shutdown!");
                    shutdown_state.set_force_shutdown();
                } else {
                    // First Ctrl-C - graceful shutdown
                    warn!("\n⚠️  Received SIGINT (Ctrl-C), initiating graceful shutdown...");
                    warn!("    Press Ctrl-C again to force immediate shutdown");
                    shutdown_state.set_shutdown();
                    
                    // Spawn a task to listen for second Ctrl-C
                    tokio::spawn(async move {
                        let mut sigint2 = signal::unix::signal(signal::unix::SignalKind::interrupt())
                            .expect("Failed to create second SIGINT handler");
                        sigint2.recv().await;
                        error!("\n🛑 Second SIGINT received - forcing immediate shutdown!");
                        force_shutdown_state.set_force_shutdown();
                    });
                }
            }
            _ = sigterm.recv() => {
                warn!("\n⚠️  Received SIGTERM (system shutdown/reboot), initiating graceful shutdown...");
                shutdown_state.set_shutdown();
            }
        }
    });

    info!("✅ Coordinator initialized, starting services...");

    // 1. Start gRPC server in a separate thread
    let grpc_socket_path = args.grpc_socket_path.clone();
    let grpc_state = state.clone();
    let grpc_handle = task::spawn(async move {
        info!("🔌 Starting gRPC server...");
        if let Err(e) = grpc::start_grpc_server(&grpc_socket_path, grpc_state).await {
            error!("gRPC server error: {}", e);
        }
    });

    // 2. Start reconciliation task in a separate thread (runs every 10 minutes)
    let reconciliation_state = state.clone();
    
    // Test fetch_app_instance with the provided app instance ID
    // {
    //     let test_app_instance_id = "0x7881cb90e9363ea4bc5b6512a1e8e7e9b91e29fd78b13840b7b87790da3ce902";
    //     let mut test_client = sui_client.clone();
    //     info!("🧪 Testing fetch_app_instance with ID: {}", test_app_instance_id);
        
    //     match crate::fetch::fetch_app_instance(&mut test_client, test_app_instance_id).await {
    //         Ok(app_instance) => {
    //             info!("✅ Successfully fetched AppInstance:");
    //             info!("  Full result: {:#?}", app_instance);
    //         }
    //         Err(e) => {
    //             error!("❌ Failed to fetch AppInstance: {}", e);
    //         }
    //     }
    // }
    let reconciliation_handle = task::spawn(async move {
        let mut reconciliation_interval = tokio::time::interval(Duration::from_secs(600)); // 10 minutes
        reconciliation_interval.tick().await; // Skip the first immediate tick
        
        loop {
            reconciliation_interval.tick().await;
            
            // Get current stats before reconciliation
            let stats = reconciliation_state.get_jobs_tracker().get_stats().await;
            debug!(
                "Starting periodic reconciliation (currently tracking {} app_instances, {} agent methods)",
                stats.app_instances_count,
                stats.agent_methods_count
            );
            
            // First, check all known app_instances for stuck jobs
            let all_app_instances = reconciliation_state.get_app_instances().await;
            if !all_app_instances.is_empty() {
                info!("Checking {} known app_instances for stuck running jobs", all_app_instances.len());
                for app_instance_id in &all_app_instances {
                    // Check for stuck jobs in this app_instance
                    if let Ok(app_inst) = sui::fetch::fetch_app_instance(app_instance_id).await {
                        if let Ok(jobs) = sui::fetch::fetch_all_jobs_from_app_instance(&app_inst).await {
                            let current_time_ms = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_millis() as u64;
                            
                            for job in jobs {
                                if matches!(job.status, sui::fetch::JobStatus::Running) {
                                    let running_duration_ms = current_time_ms.saturating_sub(job.updated_at);
                                    let running_duration = std::time::Duration::from_millis(running_duration_ms);
                                    
                                    // Check if job has been running for more than 10 minutes
                                    if running_duration > std::time::Duration::from_secs(600) {
                                        warn!(
                                            "Found stuck job {} in app_instance {} running for {:.1} minutes",
                                            job.job_sequence, app_instance_id, running_duration.as_secs_f64() / 60.0
                                        );
                                        
                                        let mut sui_interface = sui::interface::SilvanaSuiInterface::new();
                                        let error_msg = format!("Job timed out after running for {} minutes", running_duration.as_secs() / 60);
                                        
                                        if sui_interface.fail_job(app_instance_id, job.job_sequence, &error_msg).await {
                                            info!("Successfully failed stuck job {} in app_instance {}", job.job_sequence, app_instance_id);
                                        } else {
                                            error!("Failed to mark stuck job {} as failed in app_instance {}", job.job_sequence, app_instance_id);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            
            // Then do the regular reconciliation
            match reconciliation_state.get_jobs_tracker().reconcile_with_chain().await {
                Ok(has_jobs) => {
                    // Update the pending jobs flag based on reconciliation result
                    reconciliation_state.update_pending_jobs_flag().await;
                    debug!("Reconciliation complete, has_pending_jobs={}", has_jobs);
                }
                Err(e) => {
                    warn!("Reconciliation failed: {}", e);
                }
            }
        }
    });
    info!("🔄 Started reconciliation task (runs every 10 minutes)");

    // 3. Start stuck job checker task (runs every 2 minutes)
    let stuck_job_state = state.clone();
    let stuck_job_handle = task::spawn(async move {
        let mut check_interval = tokio::time::interval(Duration::from_secs(120)); // 2 minutes
        check_interval.tick().await; // Skip the first immediate tick
        
        loop {
            check_interval.tick().await;
            
            let all_app_instances = stuck_job_state.get_app_instances().await;
            if !all_app_instances.is_empty() {
                debug!("Checking {} app_instances for stuck running jobs", all_app_instances.len());
                let mut stuck_count = 0;
                
                for app_instance_id in &all_app_instances {
                    if let Ok(app_inst) = sui::fetch::fetch_app_instance(app_instance_id).await {
                        if let Ok(jobs) = sui::fetch::fetch_all_jobs_from_app_instance(&app_inst).await {
                            let current_time_ms = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_millis() as u64;
                            
                            for job in jobs {
                                if matches!(job.status, sui::fetch::JobStatus::Running) {
                                    let running_duration_ms = current_time_ms.saturating_sub(job.updated_at);
                                    let running_duration = std::time::Duration::from_millis(running_duration_ms);
                                    
                                    // Check if job has been running for more than 10 minutes
                                    if running_duration > std::time::Duration::from_secs(600) {
                                        stuck_count += 1;
                                        
                                        // Check if this is a settlement job
                                        let is_settlement = if let Ok(Some(settle_id)) = sui::fetch::app_instance::get_settlement_job_id_for_instance(&app_inst).await {
                                            settle_id == job.job_sequence
                                        } else {
                                            false
                                        };
                                        
                                        if is_settlement {
                                            warn!(
                                                "Found stuck SETTLEMENT job {} in app_instance {} running for {:.1} minutes",
                                                job.job_sequence, app_instance_id, running_duration.as_secs_f64() / 60.0
                                            );
                                        } else {
                                            warn!(
                                                "Found stuck job {} in app_instance {} running for {:.1} minutes",
                                                job.job_sequence, app_instance_id, running_duration.as_secs_f64() / 60.0
                                            );
                                        }
                                        
                                        let mut sui_interface = sui::interface::SilvanaSuiInterface::new();
                                        let error_msg = format!("Job timed out after running for {} minutes", running_duration.as_secs() / 60);
                                        
                                        if sui_interface.fail_job(app_instance_id, job.job_sequence, &error_msg).await {
                                            info!("Successfully failed stuck job {} in app_instance {}", job.job_sequence, app_instance_id);
                                        } else {
                                            error!("Failed to mark stuck job {} as failed in app_instance {}", job.job_sequence, app_instance_id);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                
                if stuck_count > 0 {
                    info!("Found and failed {} stuck jobs", stuck_count);
                }
            }
        }
    });
    info!("🔍 Started stuck job checker task (runs every 2 minutes)");

    // 4. Start block creation task in a separate thread (runs every minute)
    let block_creation_state = state.clone();
    let block_creation_handle = task::spawn(async move {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        
        let mut block_interval = tokio::time::interval(Duration::from_secs(60)); // 1 minute
        block_interval.tick().await; // Skip the first immediate tick
        
        let task_running = Arc::new(AtomicBool::new(false));
        
        loop {
            block_interval.tick().await;
            
            // Check if previous task is still running
            if task_running.load(Ordering::Acquire) {
                warn!("Previous block creation task is still running, skipping this iteration");
                continue;
            }
            
            // Clone Arc for the spawned task
            let task_running_clone = task_running.clone();
            let block_creation_state_clone = block_creation_state.clone();
            
            // Spawn the actual block creation work as a separate task
            tokio::spawn(async move {
                // Mark task as running
                task_running_clone.store(true, Ordering::Release);
                
                debug!("Starting periodic block creation check");
                
                // Get all app_instances from shared state
                let app_instances = block_creation_state_clone.get_app_instances().await;
                
                if app_instances.is_empty() {
                    debug!("No app_instances to check for block creation");
                    task_running_clone.store(false, Ordering::Release);
                    return;
                }
                
                debug!("Checking {} app_instances for block creation", app_instances.len());
                
                // Start timing the entire cycle
                let cycle_start = std::time::Instant::now();
                
                let mut error_count = 0;
                let mut created_blocks = Vec::new(); // Store (app_id, tx_digest, sequences, time_ms)
                
                // Create a SilvanaSuiInterface for this iteration
                let mut sui_interface = SilvanaSuiInterface::new();
                
                for app_instance_id in app_instances.iter() {
                    let instance_start = std::time::Instant::now();
                    
                    match try_create_block(&mut sui_interface, &app_instance_id).await {
                        Ok(Some((tx_digest, new_sequences, time_since_last))) => {
                            created_blocks.push((app_instance_id.clone(), tx_digest, new_sequences, time_since_last));
                            debug!("Block created for app_instance {}", app_instance_id);
                        }
                        Ok(None) => {
                            // Conditions not met or another coordinator created it
                            debug!("Block not created for app_instance {} (conditions not met)", app_instance_id);
                        }
                        Err(e) => {
                            error_count += 1;
                            debug!("Error checking block creation for app_instance {}: {}", app_instance_id, e);
                        }
                    }
                    
                    let instance_duration = instance_start.elapsed();
                    if instance_duration.as_secs() > 5 {
                        // Log if processing a single instance takes more than 5 seconds
                        info!("⚠️ Block creation for app_instance {} took {:.2}s", app_instance_id, instance_duration.as_secs_f64());
                    }
                }
                
                // Log timing for the entire cycle
                let cycle_duration = cycle_start.elapsed();
                if !created_blocks.is_empty() {
                    // Log detailed info for each created block
                    for (app_id, tx_digest, sequences, time_ms) in created_blocks {
                        info!(
                            "✅ Block created | app: {} | tx: {} | sequences: {} | time_since_last: {}ms | cycle_time: {:.2}s",
                            app_id,
                            tx_digest,
                            sequences,
                            time_ms,
                            cycle_duration.as_secs_f64()
                        );
                    }
                } else if error_count > 0 {
                    info!(
                        "Block creation cycle completed in {:.2}s for {} app_instances (0 created, {} errors)",
                        cycle_duration.as_secs_f64(),
                        app_instances.len(),
                        error_count
                    );
                } else {
                    debug!("Block creation check completed for {} app_instances: no blocks created", app_instances.len());
                }
                
                // Mark task as completed
                task_running_clone.store(false, Ordering::Release);
            });
        }
    });
    info!("🔲 Started block creation task (runs every minute)");

    // 4. Start proof completion analysis task in a separate thread (runs every 5 minutes)
    let proof_analysis_state = state.clone();
    let proof_analysis_handle = task::spawn(async move {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        
        let mut proof_interval = tokio::time::interval(Duration::from_secs(300)); // 5 minutes
        proof_interval.tick().await; // Skip the first immediate tick
        
        let task_running = Arc::new(AtomicBool::new(false));
        
        loop {
            proof_interval.tick().await;
            
            // Check if previous task is still running
            if task_running.load(Ordering::Acquire) {
                warn!("Previous proof completion analysis task is still running, skipping this iteration");
                continue;
            }
            
            // Clone Arc for the spawned task
            let task_running_clone = task_running.clone();
            let proof_analysis_state_clone = proof_analysis_state.clone();
            
            // Spawn the actual proof analysis work as a separate task
            tokio::spawn(async move {
                // Mark task as running
                task_running_clone.store(true, Ordering::Release);
                
                debug!("Starting periodic proof completion analysis");
                
                // Get all app_instances from shared state
                let app_instances = proof_analysis_state_clone.get_app_instances().await;
                
                if app_instances.is_empty() {
                    debug!("No app_instances to analyze for proof completion");
                    task_running_clone.store(false, Ordering::Release);
                    return;
                }
                
                debug!("Analyzing proof completion for {} app_instances", app_instances.len());
                
                // Start timing the entire cycle
                let cycle_start = std::time::Instant::now();
                
                let mut analyzed_count = 0;
                let mut error_count = 0;
                let mut merge_opportunities_found = 0;
                
                
                for app_instance_id in app_instances.iter() {
                    let instance_start = std::time::Instant::now();
                    
                    debug!("🔍 Analyzing proof completion for app_instance {}", app_instance_id);
                    match fetch_app_instance(&app_instance_id).await {
                        Ok(app_instance) => {
                            analyzed_count += 1;
                            if let Err(e) = analyze_proof_completion(&app_instance).await {
                                error_count += 1;
                                debug!("Failed to analyze proof completion for {}: {}", app_instance_id, e);
                            } else {
                                merge_opportunities_found += 1;
                                debug!("✅ Proof completion analysis done for {}", app_instance_id);
                            }
                        }
                        Err(e) => {
                            error_count += 1;
                            debug!("Failed to fetch AppInstance {} for proof analysis: {}", app_instance_id, e);
                        }
                    }
                    
                    let instance_duration = instance_start.elapsed();
                    if instance_duration.as_secs() > 5 {
                        // Log if processing a single instance takes more than 5 seconds
                        info!("⚠️ Proof analysis for app_instance {} took {:.2}s", app_instance_id, instance_duration.as_secs_f64());
                    }
                }
                
                // Log timing for the entire cycle
                let cycle_duration = cycle_start.elapsed();
                info!(
                    "🔬 Proof analysis cycle completed in {:.2}s for {} app_instances ({} analyzed, {} errors)",
                    cycle_duration.as_secs_f64(),
                    app_instances.len(),
                    analyzed_count,
                    error_count
                );
                
                if merge_opportunities_found > 0 {
                    debug!("Found merge opportunities in {} app_instances", merge_opportunities_found);
                }
                
                // Mark task as completed
                task_running_clone.store(false, Ordering::Release);
            });
        }
    });
    info!("🔬 Started proof completion analysis task (runs every 5 minutes)");

    // 5. Start job searcher in a separate thread
    let job_searcher_state = state.clone();
    let use_tee = args.use_tee;
    let container_timeout_secs = args.container_timeout;
    let job_searcher_handle = task::spawn(async move {
        let mut job_searcher = match JobSearcher::new(
            job_searcher_state,
            use_tee,
            container_timeout_secs,
        ) {
            Ok(searcher) => searcher,
            Err(e) => {
                error!("Failed to create job searcher: {}", e);
                return;
            }
        };
        
        if let Err(e) = job_searcher.run().await {
            error!("Job searcher error: {}", e);
        }
    });
    info!("🔍 Started job searcher thread");

    // 6. Start event processor in main thread (processes events and updates shared state)
    let mut processor = EventProcessor::new(config, state.clone()).await?;
    info!("👁️ Starting event monitoring...");
    
    // Run processor in a task so we can monitor shutdown
    let processor_handle = task::spawn(async move {
        processor.run().await
    });
    
    // Monitor for shutdown or processor exit
    loop {
        if state.is_shutting_down() {
            info!("🛑 Shutdown requested, starting graceful shutdown sequence...");
            break;
        }
        
        // Check if processor exited
        if processor_handle.is_finished() {
            let processor_result = processor_handle.await?;
            if let Err(e) = processor_result {
                error!("Fatal error in event processor: {}", e);
                return Err(e.into());
            }
            break;
        }
        
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    
    info!("🔄 Starting graceful shutdown sequence...");
    
    // Phase 1: Stop accepting new jobs (already done by setting shutdown flag)
    info!("  1️⃣ Stopping new job acceptance...");
    
    // Phase 2: Wait for current jobs to complete (with timeout)
    info!("  2️⃣ Waiting for current jobs to complete (max 5 minutes)...");
    let mut wait_time = 0;
    let max_wait = 300; // 5 minutes in seconds
    
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
        
        // Show more detailed progress
        if wait_time < 60 {
            info!("  ⏳ {} agents still running, waiting... ({}/{}s)", current_agents, wait_time, max_wait);
        } else {
            let minutes = wait_time / 60;
            let seconds = wait_time % 60;
            info!("  ⏳ {} agents still running, waiting... ({}m {}s / 5m)", current_agents, minutes, seconds);
        }
        
        tokio::time::sleep(Duration::from_secs(1)).await;
        wait_time += 1;
    }
    
    // Phase 3: If jobs are still running, try to get their status and logs
    let remaining_agents = state.get_current_agent_count().await;
    if remaining_agents > 0 {
        if state.is_force_shutdown() {
            error!("  🛑 Force shutdown - terminating {} running agents...", remaining_agents);
        } else {
            warn!("  ⚠️ {} agents still running after timeout, collecting logs...", remaining_agents);
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
            warn!("    - Session {}: {}/{}/{}", 
                session_id, agent_info.developer, agent_info.agent, agent_info.agent_method);
        }
        
        // Try to get container logs and optionally stop them
        if let Some(ref dm) = docker_manager {
            info!("  📋 Fetching logs from running Docker containers...");
            let container_results = dm.fetch_and_stop_silvana_containers(state.is_force_shutdown()).await;
            
            for (container_id, container_name, logs) in container_results {
                info!("\n  📦 Container: {} ({})", &container_id[..12.min(container_id.len())], container_name);
                if !logs.is_empty() {
                    // Print first 2000 chars of logs to avoid flooding the terminal
                    let log_preview = if logs.len() > 2000 {
                        format!("{}
... [truncated {} more bytes]", &logs[..2000], logs.len() - 2000)
                    } else {
                        logs
                    };
                    info!("  📄 Container logs:\n{}", log_preview);
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
    stuck_job_handle.abort();
    block_creation_handle.abort();
    proof_analysis_handle.abort();
    
    // Give job_searcher a chance to cleanup
    if !job_searcher_handle.is_finished() {
        // Wait a bit for job_searcher to finish gracefully
        let _ = tokio::time::timeout(Duration::from_secs(5), job_searcher_handle).await;
    }
    
    info!("✅ Graceful shutdown complete");
    Ok(())
}
