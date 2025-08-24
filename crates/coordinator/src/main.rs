mod agent;
mod config;
mod error;
mod events;
mod fetch;
mod grpc;
mod hardware;
mod job_id;
mod job_searcher;
mod jobs;
mod pending;
mod processor;
mod registry;
mod session_id;
mod state;
mod sui_interface;

use anyhow::Result;
use clap::Parser;
use dotenv::dotenv;
use tokio::task;
use tokio::time::Duration;
use tracing::{debug, info, warn, error};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::Config;
use crate::job_searcher::JobSearcher;
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

    #[arg(long, env = "CONTAINER_TIMEOUT_SECS", default_value = "300")]
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
    info!("📦 Monitoring package: {}", args.package_id);
    info!("📝 Module: jobs");
    info!("🔗 RPC URL: {}", args.rpc_url);

    // Create Sui client once
    let sui_client = sui_rpc::Client::new(&args.rpc_url)
        .map_err(|e| anyhow::anyhow!("Failed to create Sui client: {}", e))?;
    info!("✅ Connected to Sui RPC");

    let config = Config {
        package_id: args.package_id,
        modules: vec!["jobs".to_string()],
    };

    // Create shared state with a cloned Sui client
    let state = SharedState::new(sui_client.clone());

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
    let reconciliation_client = sui_client.clone();
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

    // 3. Start job searcher in a separate thread
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

    // 4. Start event processor in main thread (processes events and updates shared state)
    let mut processor = EventProcessor::new(config, state.clone()).await?;
    info!("👁️ Starting event monitoring...");
    
    // Run processor (this blocks)
    let processor_result = processor.run().await;

    // If processor exits, cancel all background tasks
    grpc_handle.abort();
    reconciliation_handle.abort();
    job_searcher_handle.abort();

    if let Err(e) = processor_result {
        error!("Fatal error in event processor: {}", e);
        return Err(e.into());
    }

    Ok(())
}
