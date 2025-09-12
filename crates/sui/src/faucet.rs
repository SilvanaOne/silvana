use anyhow::{Result, anyhow};
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::json;
use sui_sdk_types as sui;
use tracing::{info, warn, debug};
use std::env;
use crate::balance::get_total_balance_sui;

/// Default faucet URL for devnet
const DEFAULT_DEVNET_FAUCET_URL: &str = "https://faucet.devnet.sui.io/v2/gas";

/// Response from the devnet faucet service
#[derive(Debug, Deserialize)]
struct FaucetResponse {
    #[allow(dead_code)]
    status: Option<String>,
    error: Option<String>,
    task: Option<String>,
    #[serde(rename = "txDigests")]
    tx_digests: Option<Vec<String>>,
    coins_sent: Option<Vec<CoinsSent>>,
}

#[derive(Debug, Deserialize)]
struct CoinsSent {
    #[allow(dead_code)]
    amount: u64,
    #[allow(dead_code)]
    id: String,
    #[serde(rename = "transferTxDigest")]
    transfer_tx_digest: String,
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

impl FaucetNetwork {
    /// Parse network from chain string
    pub fn from_chain(chain: &str) -> Result<Self> {
        match chain.to_lowercase().as_str() {
            "devnet" => Ok(FaucetNetwork::Devnet),
            "testnet" => Ok(FaucetNetwork::Testnet),
            "mainnet" => Err(anyhow!("Faucet is not available for mainnet")),
            _ => Err(anyhow!("Unknown chain: {}. Expected 'devnet' or 'testnet'", chain)),
        }
    }
}

/// Request tokens from the Sui faucet
/// 
/// # Arguments
/// * `chain` - The chain name ("devnet" or "testnet")
/// * `address` - The address to send tokens to
/// * `amount` - Amount in SUI (only used for testnet, max 10 SUI)
/// 
/// # Returns
/// Ok(transaction_digest) if successful, error otherwise
pub async fn request_tokens_from_faucet(
    chain: &str,
    address: &str,
    amount: Option<f64>,
) -> Result<String> {
    let network = FaucetNetwork::from_chain(chain)?;
    let sui_address = address.parse::<sui::Address>()
        .map_err(|e| anyhow!("Invalid address format: {}", e))?;
    request_tokens_from_faucet_network(sui_address, network, amount).await
}

/// Request tokens from the Sui faucet (devnet or testnet)
/// 
/// # Arguments
/// * `address` - The address to send tokens to
/// * `network` - The network to use (Devnet or Testnet)
/// * `amount` - Amount in SUI (only used for testnet, max 10 SUI)
/// 
/// # Returns
/// Ok(transaction_digest) if successful, error otherwise
pub async fn request_tokens_from_faucet_network(
    address: sui::Address,
    network: FaucetNetwork,
    amount: Option<f64>,
) -> Result<String> {
    match network {
        FaucetNetwork::Devnet => request_tokens_from_devnet(address).await,
        FaucetNetwork::Testnet => request_tokens_from_testnet(address, amount).await,
    }
}

/// Request tokens from the Sui devnet faucet
async fn request_tokens_from_devnet(address: sui::Address) -> Result<String> {
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
                // Extract transaction digest - try multiple fields based on response format
                let tx_digest = faucet_resp.coins_sent
                    .and_then(|coins| coins.first().map(|c| c.transfer_tx_digest.clone()))
                    .or_else(|| faucet_resp.tx_digests.and_then(|digests| digests.first().cloned()))
                    .or(faucet_resp.task)
                    .unwrap_or_else(|| "unknown".to_string());
                
                info!(
                    "✅ Devnet faucet request successful for {}. Transaction: {}",
                    address, tx_digest
                );
                return Ok(tx_digest);
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
}

/// Request tokens from the Sui testnet faucet (Silvana/StakeTab)
async fn request_tokens_from_testnet(address: sui::Address, amount: Option<f64>) -> Result<String> {
    // Get faucet URL from environment
    let url = env::var("SUI_FAUCET_TESTNET")
        .or_else(|_| env::var("SILVANA_FAUCET_ENDPOINT"))
        .map_err(|_| anyhow!("SUI_FAUCET_TESTNET or SILVANA_FAUCET_ENDPOINT environment variable not set"))?;
    
    // Default to 1 SUI, max 10 SUI for testnet
    let amount_sui = amount.unwrap_or(1.0).min(10.0);
    
    info!("Requesting {} SUI from testnet faucet at {} for address {}", amount_sui, url, address);
    
    let address_str = address.to_string();
    
    // Convert SUI to MIST (1 SUI = 1,000,000,000 MIST)
    let amount_mist = (amount_sui * 1_000_000_000.0) as u64;
    
    // Testnet faucet uses a different payload format - amount is in MIST
    let json_body = json!({
        "address": address_str,
        "amount": amount_mist,
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
                let tx_digest = faucet_resp.transaction_hash
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string());
                
                info!(
                    "✅ Testnet faucet request successful for {}. Transaction: {}",
                    address, tx_digest
                );
                return Ok(tx_digest);
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
}


/// Request tokens from faucet using the default address from SharedSuiState
pub async fn request_tokens_for_default_address() -> Result<()> {
    use crate::state::SharedSuiState;
    
    let shared_state = SharedSuiState::get_instance();
    let address = shared_state.get_sui_address_required();
    
    // Determine chain from environment or default to devnet
    let chain = std::env::var("SUI_CHAIN").unwrap_or_else(|_| "devnet".to_string());
    request_tokens_from_faucet(&chain, &address.to_string(), None).await?;
    Ok(())
}

/// Request tokens from a specific network's faucet for the default address
pub async fn request_tokens_for_default_address_network(
    network: FaucetNetwork,
    amount: Option<f64>,
) -> Result<()> {
    use crate::state::SharedSuiState;
    
    let shared_state = SharedSuiState::get_instance();
    let address = shared_state.get_sui_address_required();
    
    request_tokens_from_faucet_network(address, network, amount).await?;
    Ok(())
}

/// Check if we need tokens and request from faucet if necessary
/// Returns true if tokens were requested, false if not needed
pub async fn ensure_sufficient_balance(min_balance_sui: f64) -> Result<bool> {
    use crate::state::SharedSuiState;
    use crate::coin::list_coins;
    
    let shared_state = SharedSuiState::get_instance();
    let mut client = shared_state.get_sui_client();
    let address = shared_state.get_sui_address_required();
    
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
        
        // Determine chain from environment or default to devnet
        let chain = std::env::var("SUI_CHAIN").unwrap_or_else(|_| "devnet".to_string());
        // Request 5 SUI from faucet (reasonable amount that faucet can provide)
        let amount = if chain.to_lowercase() == "testnet" { Some(5.0) } else { None };
        request_tokens_from_faucet(&chain, &address.to_string(), amount).await?;
        
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
    let address = shared_state.get_sui_address_required();
    
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
        
        // For testnet, request a reasonable amount (2.5 SUI)
        let request_amount = if matches!(network, FaucetNetwork::Testnet) { 
            Some(2.5) 
        } else { 
            amount 
        };
        request_tokens_from_faucet_network(address, network, request_amount).await?;
        
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
    
    // Detect network from SUI_CHAIN env var or default to devnet
    let chain = env::var("SUI_CHAIN")
        .unwrap_or_else(|_| "devnet".to_string())
        .to_lowercase();
    
    let network = match chain.as_str() {
        "testnet" => Some(FaucetNetwork::Testnet),
        "devnet" => Some(FaucetNetwork::Devnet),
        "mainnet" => None, // No faucet available for mainnet
        _ => {
            warn!("Unknown chain '{}', assuming no faucet available", chain);
            None
        }
    };
    
    // If no faucet is available (mainnet or unknown chain), just check balance
    let Some(network) = network else {
        info!("💰 No faucet available for chain '{}', checking balance only...", chain);
        
        // Just check and report the balance
        match get_total_balance_sui().await {
            Ok(balance) => {
                if balance < 1.0 {
                    warn!("⚠️ Low balance: {:.4} SUI. Please fund your account manually.", balance);
                } else {
                    info!("✅ Current balance: {:.4} SUI", balance);
                }
            }
            Err(e) => {
                warn!("⚠️ Failed to check balance: {}", e);
            }
        }
        return Ok(());
    };
    
    // Check if we need tokens (minimum 10 SUI for testnet, 5 for devnet)
    let min_balance = if matches!(network, FaucetNetwork::Testnet) { 10.0 } else { 5.0 };
    // Request 2.5 SUI for testnet (reasonable amount that faucet can provide)
    let amount = if matches!(network, FaucetNetwork::Testnet) { Some(2.5) } else { None };
    
    match ensure_sufficient_balance_network(min_balance, network, amount).await {
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