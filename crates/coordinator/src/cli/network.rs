use crate::error::{CoordinatorError, Result};
use tracing::error;
use tracing_subscriber::prelude::*;

pub async fn handle_network_command(rpc_url: Option<String>, chain_override: Option<String>) -> Result<()> {
    // Initialize minimal logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("info"))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Resolve and initialize Sui connection
    let rpc_url = sui::resolve_rpc_url(rpc_url, chain_override.clone())
        .map_err(CoordinatorError::Other)?;
    sui::SharedSuiState::initialize(&rpc_url)
        .await
        .map_err(CoordinatorError::Other)?;

    let network_name = sui::get_network_name();
    let address = sui::get_current_address();

    println!("🌐 Network: {}", network_name);
    println!("👤 Address: {}", address);

    // Print detailed network info
    match sui::print_network_info().await {
        Ok(()) => {}
        Err(e) => {
            error!("Failed to fetch network info: {}", e);
            return Err(anyhow::anyhow!("Failed to fetch network info: {}", e).into());
        }
    }

    Ok(())
}