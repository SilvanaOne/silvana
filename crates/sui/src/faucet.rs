use anyhow::{Result, anyhow};
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::json;
use sui_sdk_types as sui;
use tracing::{info, warn, debug};
use std::env;

/// Default faucet URL for devnet
const DEFAULT_DEVNET_FAUCET_URL: &str = "https://faucet.devnet.sui.io/v2/gas";

/// Response from the faucet service
#[derive(Debug, Deserialize)]
struct FaucetResponse {
    error: Option<String>,
}

/// Request tokens from the Sui faucet
/// 
/// # Arguments
/// * `address` - The address to send tokens to
/// * `faucet_url` - Optional faucet URL, uses environment variable or default if None
/// 
/// # Returns
/// Ok(()) if successful, error otherwise
pub async fn request_tokens_from_faucet(
    address: sui::Address,
    faucet_url: Option<String>,
) -> Result<()> {
    // Get faucet URL from parameter, environment, or use default
    let url = faucet_url
        .or_else(|| env::var("SUI_FAUCET_DEVNET").ok())
        .unwrap_or_else(|| DEFAULT_DEVNET_FAUCET_URL.to_string());
    
    info!("Requesting tokens from faucet at {} for address {}", url, address);
    
    let address_str = address.to_string();
    let json_body = json![{
        "FixedAmountRequest": {
            "recipient": &address_str
        }
    }];

    // Make the request to the faucet JSON RPC API for coins
    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(reqwest::header::USER_AGENT, "silvana-coordinator")
        .json(&json_body)
        .send()
        .await?;

    match resp.status() {
        StatusCode::ACCEPTED | StatusCode::CREATED | StatusCode::OK => {
            let faucet_resp: FaucetResponse = resp.json().await?;
            debug!("Faucet response: {:?}", faucet_resp);

            if let Some(err) = faucet_resp.error {
                return Err(anyhow!("Faucet request was unsuccessful: {}", err));
            } else {
                info!(
                    "✅ Faucet request successful for {}. Tokens should arrive within 1 minute.",
                    address
                );
            }
        }
        StatusCode::BAD_REQUEST => {
            let faucet_resp: FaucetResponse = resp.json().await?;
            if let Some(err) = faucet_resp.error {
                return Err(anyhow!("Faucet request was unsuccessful: {}", err));
            }
            return Err(anyhow!("Faucet request failed with bad request"));
        }
        StatusCode::TOO_MANY_REQUESTS => {
            return Err(anyhow!(
                "Faucet service received too many requests from this IP address. Please try again after 60 minutes."
            ));
        }
        StatusCode::SERVICE_UNAVAILABLE => {
            return Err(anyhow!(
                "Faucet service is currently overloaded or unavailable. Please try again later."
            ));
        }
        status_code => {
            return Err(anyhow!("Faucet request was unsuccessful: {}", status_code));
        }
    }
    
    Ok(())
}

/// Request tokens from faucet using the default address from SharedSuiState
pub async fn request_tokens_for_default_address() -> Result<()> {
    use crate::state::SharedSuiState;
    
    let shared_state = SharedSuiState::get_instance();
    let address = shared_state.get_sui_address();
    
    request_tokens_from_faucet(address, None).await
}

/// Check if we need tokens and request from faucet if necessary
/// Returns true if tokens were requested, false if not needed
pub async fn ensure_sufficient_balance(min_balance_sui: f64) -> Result<bool> {
    use crate::state::SharedSuiState;
    use crate::coin::list_coins;
    
    let shared_state = SharedSuiState::get_instance();
    let mut client = shared_state.get_sui_client();
    let address = shared_state.get_sui_address();
    
    // Check current balance
    let coins = list_coins(&mut client, address).await?;
    let total_balance: u64 = coins.iter().map(|c| c.balance).sum();
    let total_balance_sui = total_balance as f64 / 1_000_000_000.0;
    
    info!(
        "Current balance: {} SUI ({} MIST) across {} coins",
        total_balance_sui, total_balance, coins.len()
    );
    
    if total_balance_sui < min_balance_sui {
        warn!(
            "Balance {} SUI is below minimum {} SUI, requesting from faucet...",
            total_balance_sui, min_balance_sui
        );
        
        request_tokens_from_faucet(address, None).await?;
        
        // Wait a bit for tokens to arrive
        info!("Waiting 5 seconds for faucet tokens to arrive...");
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        
        // Check balance again
        let coins_after = list_coins(&mut client, address).await?;
        let new_balance: u64 = coins_after.iter().map(|c| c.balance).sum();
        let new_balance_sui = new_balance as f64 / 1_000_000_000.0;
        
        info!(
            "New balance after faucet: {} SUI ({} MIST) across {} coins",
            new_balance_sui, new_balance, coins_after.len()
        );
        
        Ok(true)
    } else {
        debug!("Sufficient balance available, no faucet request needed");
        Ok(false)
    }
}

/// Initialize faucet module and ensure minimum balance
/// This is called during coordinator startup
pub async fn initialize_faucet() -> Result<()> {
    info!("🚰 Checking balance and faucet availability...");
    
    // Check if we need tokens (minimum 5 SUI for operations)
    match ensure_sufficient_balance(5.0).await {
        Ok(requested) => {
            if requested {
                info!("✅ Faucet tokens requested successfully");
            } else {
                info!("✅ Sufficient balance available");
            }
            Ok(())
        }
        Err(e) => {
            // Don't fail hard, just warn
            warn!("⚠️ Failed to check/request faucet tokens: {}. Continuing with existing balance.", e);
            Ok(())
        }
    }
}