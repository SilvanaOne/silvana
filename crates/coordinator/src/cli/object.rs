use crate::error::{CoordinatorError, Result};
use anyhow::anyhow;
use tracing::error;
use tracing_subscriber::prelude::*;

pub async fn handle_object_command(
    rpc_url: Option<String>,
    object: String,
    chain_override: Option<String>,
) -> Result<()> {
    // Initialize minimal logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("warn"))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Resolve and initialize Sui connection (read-only mode)
    let rpc_url = sui::resolve_rpc_url(rpc_url, chain_override)
        .map_err(CoordinatorError::Other)?;
    sui::SharedSuiState::initialize_read_only(&rpc_url)
        .await
        .map_err(CoordinatorError::Other)?;

    // Fetch and display the raw object
    match sui::fetch::fetch_object(&object).await {
        Ok(json_value) => {
            // Pretty print the JSON
            let pretty_json =
                serde_json::to_string_pretty(&json_value).map_err(|e| anyhow!(e))?;
            println!("{}", pretty_json);
        }
        Err(e) => {
            error!("Failed to fetch object {}: {}", object, e);
            return Err(anyhow!(e).into());
        }
    }

    Ok(())
}