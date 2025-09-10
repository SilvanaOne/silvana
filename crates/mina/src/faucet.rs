use crate::networks::MinaNetwork;
use anyhow::{Result, anyhow};
use reqwest;
use serde::Deserialize;
use std::time::Duration;
use tracing::info;

#[derive(Debug, Deserialize)]
pub struct FaucetResponse {
    pub status: Option<String>,
    pub message: Option<String>,
    pub error: Option<String>,
}

/// Request MINA tokens from the devnet faucet
pub async fn request_mina_from_faucet(address: &str, network: &str) -> Result<FaucetResponse> {
    // Get the network configuration
    let network_config = MinaNetwork::get_network(network)
        .ok_or_else(|| anyhow!("Unsupported network: {}. Use 'mina:devnet' or 'zeko:testnet' (or 'devnet'/'zeko' for short)", network))?;

    let faucet_url = network_config
        .faucet_endpoint
        .ok_or_else(|| anyhow!("Network {} does not have a faucet", network))?;

    info!(
        "🚰 Requesting MINA from {} faucet for address {}",
        network_config.name, address
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    let request_body = if network == "zeko" {
        // Zeko faucet only needs address
        serde_json::json!({
            "address": address,
        })
    } else {
        // Devnet faucet needs both address and network
        serde_json::json!({
            "address": address,
            "network": "devnet",
        })
    };

    let response = client
        .post(faucet_url)
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| anyhow!("Failed to send faucet request: {}", e))?;

    let status_code = response.status();

    if !status_code.is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(anyhow!(
            "Faucet request failed with status {}: {}",
            status_code,
            error_text
        ));
    }

    // Try to parse as JSON, but if it fails, create a response with the text
    let response_text = response.text().await?;

    // For Zeko, the response might be plain text
    if network == "zeko" && response_text.contains("Successfully sent") {
        return Ok(FaucetResponse {
            status: Some("success".to_string()),
            message: Some(response_text),
            error: None,
        });
    }

    // Try to parse as JSON
    match serde_json::from_str::<FaucetResponse>(&response_text) {
        Ok(faucet_response) => Ok(faucet_response),
        Err(_) => {
            // If JSON parsing fails, create a response with the raw text
            Ok(FaucetResponse {
                status: Some("success".to_string()),
                message: Some(response_text),
                error: None,
            })
        }
    }
}

/// Check if the address format is valid for Mina
pub fn validate_mina_address(address: &str) -> bool {
    // Mina addresses start with B62 and are base58 encoded
    address.starts_with("B62") && address.len() >= 55
}
