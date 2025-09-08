use anyhow::Result;
use clap::Parser;
use tracing_subscriber::FmtSubscriber;
use avs_operator::cli::{AvsCliArgs, execute_avs_command};

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file first
    dotenvy::dotenv().ok();
    
    // Initialize tracing
    let subscriber = FmtSubscriber::builder()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;
    
    // Parse CLI arguments
    let cli = AvsCliArgs::parse();
    
    // Execute command
    execute_avs_command(cli.command).await
}