use crate::error::{CoordinatorError, Result};
use tracing::error;
use tracing_subscriber::prelude::*;

pub async fn handle_network_command(rpc_url: Option<String>, chain_override: Option<String>) -> Result<()> {
    // Initialize minimal logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("info"))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Resolve and initialize Sui connection (read-only mode)
    let rpc_url = sui::resolve_rpc_url(rpc_url, chain_override.clone())
        .map_err(CoordinatorError::Other)?;
    sui::SharedSuiState::initialize_read_only(&rpc_url)
        .await
        .map_err(CoordinatorError::Other)?;

    let network_name = sui::get_network_name();
    
    println!("🌐 Network: {}", network_name);
    
    // Only show address if SUI_ADDRESS is set
    if let Ok(address) = std::env::var("SUI_ADDRESS") {
        println!("👤 Address: {}", address);
    }

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