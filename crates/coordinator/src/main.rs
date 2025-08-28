mod agent;
mod block;
mod config;
mod error;
mod events;
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

    #[arg(long, env = "CONTAINER_TIMEOUT_SECS", default_value = "600")]
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
    let reconciliation_client = state.get_sui_client();
    
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
            
            let mut client = reconciliation_client.clone();
            match reconciliation_state.get_jobs_tracker().reconcile_with_chain(&mut client).await {
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

    // 3. Start block creation task in a separate thread (runs every minute)
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
    
    // Run processor (this blocks)
    let processor_result = processor.run().await;

    // If processor exits, cancel all background tasks
    grpc_handle.abort();
    reconciliation_handle.abort();
    block_creation_handle.abort();
    proof_analysis_handle.abort();
    job_searcher_handle.abort();

    if let Err(e) = processor_result {
        error!("Fatal error in event processor: {}", e);
        return Err(e.into());
    }

    Ok(())
}
