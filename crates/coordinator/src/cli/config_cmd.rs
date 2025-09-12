use crate::config;
use crate::error::Result;
use anyhow::anyhow;
use tracing::error;
use tracing_subscriber::prelude::*;

pub async fn handle_config_command(
    endpoint: Option<String>,
    json: bool,
    chain_override: Option<String>,
) -> Result<()> {
    // Initialize minimal logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("info"))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Determine which chain to fetch config for
    let chain = chain_override
        .or_else(|| std::env::var("SUI_CHAIN").ok())
        .unwrap_or_else(|| "testnet".to_string());

    println!("📥 Fetching configuration for chain: {}", chain);

    // Fetch configuration
    let config_map = if let Some(endpoint) = endpoint.as_ref() {
        config::fetch_config_with_endpoint(&chain, Some(endpoint)).await
    } else {
        config::fetch_config(&chain).await
    };

    match config_map {
        Ok(config) => {
            if json {
                // Output as JSON
                match serde_json::to_string_pretty(&config) {
                    Ok(json_str) => println!("{}", json_str),
                    Err(e) => {
                        error!("Failed to serialize config to JSON: {}", e);
                        return Err(anyhow!(
                            "Failed to serialize config to JSON: {}",
                            e
                        )
                        .into());
                    }
                }
            } else {
                // Use the display function from config module
                config::fetch_and_display_config(&chain)
                    .await
                    .map_err(|e| anyhow!("Failed to display config: {}", e))?;
            }
            Ok(())
        }
        Err(e) => {
            error!("Failed to fetch configuration: {}", e);
            Err(anyhow!("Failed to fetch configuration: {}", e).into())
        }
    }
}