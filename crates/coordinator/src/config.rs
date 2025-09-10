use anyhow::Result;
use rpc_client::{RpcClientConfig, SilvanaRpcClient};
use std::collections::HashMap;
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone)]
pub struct Config {
    pub package_id: String,
    pub modules: Vec<String>,
}

/// Get the default RPC server endpoint from environment or fallback
fn get_default_endpoint() -> String {
    std::env::var("SILVANA_RPC_SERVER")
        .unwrap_or_else(|_| "https://rpc.silvana.dev".to_string())
}

/// Fetch configuration from the RPC server for a given chain
/// Returns a HashMap of configuration key-value pairs
pub async fn fetch_config(chain: &str) -> Result<HashMap<String, String>> {
    fetch_config_with_endpoint(chain, None).await
}

/// Fetch configuration from the RPC server with a specific endpoint
/// Returns a HashMap of configuration key-value pairs
pub async fn fetch_config_with_endpoint(
    chain: &str,
    endpoint: Option<&str>,
) -> Result<HashMap<String, String>> {
    // Validate chain
    if !["devnet", "testnet", "mainnet"].contains(&chain) {
        anyhow::bail!(
            "Invalid chain: {}. Must be devnet, testnet, or mainnet",
            chain
        );
    }

    let endpoint = endpoint
        .map(|e| e.to_string())
        .unwrap_or_else(get_default_endpoint);

    info!("Fetching configuration for chain {} from {}", chain, endpoint);

    // Create RPC client
    let mut client = SilvanaRpcClient::new(RpcClientConfig::new(&endpoint))
        .await
        .map_err(|e| {
            error!("Failed to connect to RPC endpoint {}: {}", endpoint, e);
            anyhow::anyhow!("Failed to connect to RPC endpoint {}: {}", endpoint, e)
        })?;

    // Fetch the configuration
    let response = client.get_config(chain).await.map_err(|e| {
        error!("Failed to fetch config for chain {}: {}", chain, e);
        anyhow::anyhow!("Failed to fetch config for chain {}: {}", chain, e)
    })?;

    if !response.success {
        error!(
            "RPC server returned failure for chain {}: {}",
            chain, response.message
        );
        anyhow::bail!(
            "Failed to fetch configuration: {}",
            response.message
        );
    }

    if response.config.is_empty() {
        warn!("No configuration found for chain: {}", chain);
    } else {
        info!(
            "Successfully fetched {} configuration items for chain {}",
            response.config.len(),
            chain
        );
        debug!("Configuration keys: {:?}", response.config.keys().collect::<Vec<_>>());
    }

    Ok(response.config)
}

/// Fetch and log configuration (similar to make read-config output)
pub async fn fetch_and_display_config(chain: &str) -> Result<HashMap<String, String>> {
    let config = fetch_config(chain).await?;

    if config.is_empty() {
        println!("⚠️  No configuration found for chain: {}", chain);
    } else {
        println!("📋 Configuration ({} items):", config.len());
        println!("  {}", "-".repeat(60));

        // Sort keys for consistent output
        let mut keys: Vec<_> = config.keys().collect();
        keys.sort();

        for key in keys {
            if let Some(value) = config.get(key) {
                // Mask sensitive values
                let display_value = if key.to_lowercase().contains("key")
                    || key.to_lowercase().contains("secret")
                    || key.to_lowercase().contains("password")
                    || key.to_lowercase().contains("token")
                {
                    if value.len() > 8 {
                        format!("{}...{}", &value[..4], &value[value.len() - 4..])
                    } else {
                        "***".to_string()
                    }
                } else {
                    value.clone()
                };

                println!("  {} = {}", key, display_value);
            }
        }
        println!("  {}", "-".repeat(60));
    }

    Ok(config)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fetch_config_invalid_chain() {
        let result = fetch_config("invalid_chain").await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid chain"));
    }
}