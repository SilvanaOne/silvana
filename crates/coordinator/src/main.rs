mod config;
mod error;
mod events;
mod processor;

use anyhow::Result;
use clap::Parser;
use dotenv::dotenv;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::Config;
use crate::processor::EventProcessor;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long, env = "SUI_RPC_URL", default_value = "http://148.251.75.59:9000")]
    rpc_url: String,

    #[arg(long, env = "COORDINATION_PACKAGE_ID")]
    package_id: String,

    #[arg(long, env = "COORDINATION_MODULE", default_value = "agent")]
    module: String,

    #[arg(long, env = "DOCKER_USE_TEE", default_value = "false")]
    use_tee: bool,

    #[arg(long, env = "CONTAINER_TIMEOUT_SECS", default_value = "300")]
    container_timeout: u64,

    #[arg(long, env = "LOG_LEVEL", default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<()> {
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
    info!("📝 Module: {}", args.module);
    info!("🔗 RPC URL: {}", args.rpc_url);

    let config = Config {
        rpc_url: args.rpc_url,
        package_id: args.package_id,
        module: args.module,
        use_tee: args.use_tee,
        container_timeout_secs: args.container_timeout,
    };

    let mut processor = EventProcessor::new(config).await?;

    info!("✅ Coordinator initialized, starting event monitoring...");

    if let Err(e) = processor.run().await {
        error!("Fatal error in event processor: {}", e);
        return Err(e.into());
    }

    Ok(())
}