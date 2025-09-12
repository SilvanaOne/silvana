use crate::error::Result;
use tracing_subscriber::prelude::*;

pub async fn handle_avs_command(subcommand: avs_operator::cli::AvsCommands) -> Result<()> {
    // Initialize tracing for AVS commands
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // Execute the AVS command
    Ok(avs_operator::cli::execute_avs_command(subcommand).await?)
}