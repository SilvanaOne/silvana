mod config;
mod error;
mod events;
mod fetch;
mod grpc;
mod jobs;
mod pending;
mod processor;
mod registry;
mod state;

use anyhow::Result;
use clap::Parser;
use dotenv::dotenv;
use tokio::task;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::Config;
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

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    info!("🚀 Starting Silvana Coordinator");
    info!("📦 Monitoring package: {}", args.package_id);
    info!("📝 Module: jobs");
    info!("🔗 RPC URL: {}", args.rpc_url);

    // Create Sui client once
    let sui_client = sui_rpc::Client::new(&args.rpc_url)
        .map_err(|e| anyhow::anyhow!("Failed to create Sui client: {}", e))?;
    info!("✅ Connected to Sui RPC");

    let config = Config {
        rpc_url: args.rpc_url.clone(),
        package_id: args.package_id,
        modules: vec!["jobs".to_string()],
        use_tee: args.use_tee,
        container_timeout_secs: args.container_timeout,
    };

    // Create shared state with a cloned Sui client
    let state = SharedState::new(sui_client.clone());

    let mut processor = EventProcessor::new(config, state.clone()).await?;

    info!("✅ Coordinator initialized, starting services...");

    // Start gRPC server in a separate task with shared state
    let grpc_socket_path = args.grpc_socket_path.clone();
    let grpc_state = state.clone();
    let grpc_handle = task::spawn(async move {
        info!("🔌 Starting gRPC server on socket: {}", grpc_socket_path);
        if let Err(e) = grpc::start_grpc_server(&grpc_socket_path, grpc_state).await {
            error!("gRPC server error: {}", e);
        }
    });

    // Start reconciliation task that syncs with on-chain state every 10 minutes
    let reconciliation_handle = crate::jobs::JobsTracker::start_reconciliation_task(
        state.get_jobs_tracker().clone(),
        sui_client,
    );
    info!("🔄 Started reconciliation task (runs every 10 minutes)");

    // Start event monitoring
    info!("👁️ Starting event monitoring...");
    let processor_result = processor.run().await;

    // If processor exits, cancel background tasks
    grpc_handle.abort();
    reconciliation_handle.abort();

    if let Err(e) = processor_result {
        error!("Fatal error in event processor: {}", e);
        return Err(e.into());
    }

    Ok(())
}
