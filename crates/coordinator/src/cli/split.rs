use crate::error::{CoordinatorError, Result};
use tracing::error;
use tracing_subscriber::prelude::*;

pub async fn handle_split_command(rpc_url: Option<String>, chain_override: Option<String>) -> Result<()> {
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

    println!("Checking gas coin pool and splitting if needed...");

    match sui::coin_management::ensure_gas_coin_pool().await {
        Ok(()) => {
            println!("✅ Gas coin pool check complete");

            // Show updated balance info
            println!("\nUpdated balance:");
            sui::print_balance_info(None)
                .await
                .map_err(CoordinatorError::Other)?;
        }
        Err(e) => {
            error!("Failed to manage gas coin pool: {}", e);
            return Err(anyhow::anyhow!("Failed to manage gas coin pool: {}", e).into());
        }
    }

    Ok(())
}