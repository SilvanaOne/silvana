use anyhow::{Result, anyhow};
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::json;
use sui_sdk_types as sui;
use tracing::{info, warn, debug};
use std::env;

/// Default faucet URL for devnet
const DEFAULT_DEVNET_FAUCET_URL: &str = "https://faucet.devnet.sui.io/v2/gas";

/// Response from the devnet faucet service
#[derive(Debug, Deserialize)]
struct FaucetResponse {
    error: Option<String>,
}

/// Response from the testnet faucet service (Silvana/StakeTab)
#[derive(Debug, Deserialize)]
struct TestnetFaucetResponse {
    message: Option<String>,
    success: Option<bool>,
    transaction_hash: Option<String>,
    error: Option<String>,
}

/// Network type for faucet selection
#[derive(Debug, Clone, Copy)]
pub enum FaucetNetwork {
    Devnet,
    Testnet,
}

/// Request tokens from the Sui faucet (devnet or testnet)
/// 
/// # Arguments
/// * `address` - The address to send tokens to
/// * `network` - The network to use (Devnet or Testnet)
/// * `amount` - Amount in SUI (only used for testnet, max 10 SUI)
/// 
/// # Returns
/// Ok(()) if successful, error otherwise
pub async fn request_tokens_from_faucet_network(
    address: sui::Address,
    network: FaucetNetwork,
    amount: Option<f64>,
) -> Result<()> {
    match network {
        FaucetNetwork::Devnet => request_tokens_from_devnet(address).await,
        FaucetNetwork::Testnet => request_tokens_from_testnet(address, amount).await,
    }
}

/// Request tokens from the Sui devnet faucet
async fn request_tokens_from_devnet(address: sui::Address) -> Result<()> {
    // Get faucet URL from environment or use default
    let url = env::var("SUI_FAUCET_DEVNET")
        .unwrap_or_else(|_| DEFAULT_DEVNET_FAUCET_URL.to_string());
    
    info!("Requesting tokens from devnet faucet at {} for address {}", url, address);
    
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
            debug!("Devnet faucet response: {:?}", faucet_resp);

            if let Some(err) = faucet_resp.error {
                return Err(anyhow!("Devnet faucet request was unsuccessful: {}", err));
            } else {
                info!(
                    "✅ Devnet faucet request successful for {}. Tokens should arrive within 1 minute.",
                    address
                );
            }
        }
        StatusCode::BAD_REQUEST => {
            let faucet_resp: FaucetResponse = resp.json().await?;
            if let Some(err) = faucet_resp.error {
                return Err(anyhow!("Devnet faucet request was unsuccessful: {}", err));
            }
            return Err(anyhow!("Devnet faucet request failed with bad request"));
        }
        StatusCode::TOO_MANY_REQUESTS => {
            return Err(anyhow!(
                "Devnet faucet service received too many requests from this IP address. Please try again after 60 minutes."
            ));
        }
        StatusCode::SERVICE_UNAVAILABLE => {
            return Err(anyhow!(
                "Devnet faucet service is currently overloaded or unavailable. Please try again later."
            ));
        }
        status_code => {
            return Err(anyhow!("Devnet faucet request was unsuccessful: {}", status_code));
        }
    }
    
    Ok(())
}

/// Request tokens from the Sui testnet faucet (Silvana/StakeTab)
async fn request_tokens_from_testnet(address: sui::Address, amount: Option<f64>) -> Result<()> {
    // Get faucet URL from environment
    let url = env::var("SUI_FAUCET_TESTNET")
        .or_else(|_| env::var("SILVANA_FAUCET_ENDPOINT"))
        .map_err(|_| anyhow!("SUI_FAUCET_TESTNET or SILVANA_FAUCET_ENDPOINT environment variable not set"))?;
    
    // Default to 1 SUI, max 10 SUI for testnet
    let amount_sui = amount.unwrap_or(1.0).min(10.0);
    
    info!("Requesting {} SUI from testnet faucet at {} for address {}", amount_sui, url, address);
    
    let address_str = address.to_string();
    
    // Testnet faucet uses a different payload format
    let json_body = json!({
        "address": address_str,
        "amount": amount_sui as u32,
    });

    // Make the request to the testnet faucet API
    let client = reqwest::Client::new();
    let endpoint = if url.ends_with("/fund") {
        url.clone()
    } else {
        format!("{}/fund", url.trim_end_matches('/'))
    };
    
    let resp = client
        .post(&endpoint)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(&json_body)
        .send()
        .await?;

    match resp.status() {
        StatusCode::OK | StatusCode::CREATED => {
            let faucet_resp: TestnetFaucetResponse = resp.json().await?;
            debug!("Testnet faucet response: {:?}", faucet_resp);

            if let Some(true) = faucet_resp.success {
                if let Some(tx_hash) = faucet_resp.transaction_hash {
                    info!(
                        "✅ Testnet faucet request successful for {}. Transaction: {}",
                        address, tx_hash
                    );
                } else {
                    info!(
                        "✅ Testnet faucet request successful for {}. {} SUI sent.",
                        address, amount_sui
                    );
                }
            } else if let Some(err) = faucet_resp.error {
                return Err(anyhow!("Testnet faucet request failed: {}", err));
            } else if let Some(msg) = faucet_resp.message {
                return Err(anyhow!("Testnet faucet request failed: {}", msg));
            } else {
                return Err(anyhow!("Testnet faucet request failed with unknown error"));
            }
        }
        StatusCode::BAD_REQUEST => {
            let body = resp.text().await?;
            return Err(anyhow!("Testnet faucet request failed with bad request: {}", body));
        }
        StatusCode::TOO_MANY_REQUESTS => {
            return Err(anyhow!(
                "Testnet faucet rate limit exceeded. Please try again later."
            ));
        }
        StatusCode::SERVICE_UNAVAILABLE => {
            return Err(anyhow!(
                "Testnet faucet service is currently unavailable. Please try again later."
            ));
        }
        status_code => {
            let body = resp.text().await?;
            return Err(anyhow!("Testnet faucet request failed with status {}: {}", status_code, body));
        }
    }
    
    Ok(())
}

/// Request tokens from the Sui faucet (backward compatibility - uses devnet by default)
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
    // For backward compatibility, use devnet by default
    if faucet_url.is_some() {
        // If a custom URL is provided, try to determine which network based on the URL
        let url = faucet_url.as_ref().unwrap();
        if url.contains("testnet") || url.contains("staketab") || url.contains("silvana") {
            request_tokens_from_testnet(address, None).await
        } else {
            request_tokens_from_devnet(address).await
        }
    } else {
        // Default to devnet for backward compatibility
        request_tokens_from_devnet(address).await
    }
}

/// Request tokens from faucet using the default address from SharedSuiState
pub async fn request_tokens_for_default_address() -> Result<()> {
    use crate::state::SharedSuiState;
    
    let shared_state = SharedSuiState::get_instance();
    let address = shared_state.get_sui_address();
    
    request_tokens_from_faucet(address, None).await
}

/// Request tokens from a specific network's faucet for the default address
pub async fn request_tokens_for_default_address_network(
    network: FaucetNetwork,
    amount: Option<f64>,
) -> Result<()> {
    use crate::state::SharedSuiState;
    
    let shared_state = SharedSuiState::get_instance();
    let address = shared_state.get_sui_address();
    
    request_tokens_from_faucet_network(address, network, amount).await
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

/// Check if we need tokens and request from specific network's faucet if necessary
/// Returns true if tokens were requested, false if not needed
pub async fn ensure_sufficient_balance_network(
    min_balance_sui: f64,
    network: FaucetNetwork,
    amount: Option<f64>,
) -> Result<bool> {
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
            "Balance {} SUI is below minimum {} SUI, requesting from {:?} faucet...",
            total_balance_sui, min_balance_sui, network
        );
        
        request_tokens_from_faucet_network(address, network, amount).await?;
        
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
    
    // Try to detect network from RPC URL if available
    let network = if let Ok(rpc_url) = env::var("SUI_RPC_URL") {
        if rpc_url.contains("testnet") {
            FaucetNetwork::Testnet
        } else {
            FaucetNetwork::Devnet
        }
    } else {
        FaucetNetwork::Devnet
    };
    
    // Check if we need tokens (minimum 5 SUI for operations)
    let min_balance = if matches!(network, FaucetNetwork::Testnet) { 10.0 } else { 5.0 };
    
    match ensure_sufficient_balance_network(min_balance, network, None).await {
        Ok(requested) => {
            if requested {
                info!("✅ Faucet tokens requested successfully from {:?}", network);
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