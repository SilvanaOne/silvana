use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;
use reqwest;
use crate::networks::MinaNetwork;

/// Default token ID for MINA
pub const DEFAULT_TOKEN_ID: &str = "wSHV2S4qX9jFsLjQo8r1BsMLH2ZRKsZx6EJd1sbozGPieEC4Jf";

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountBalance {
    pub total: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountInfo {
    #[serde(rename = "publicKey")]
    pub public_key: String,
    pub token: String,
    pub nonce: String,
    pub balance: AccountBalance,
    #[serde(rename = "tokenSymbol")]
    pub token_symbol: Option<String>,
    #[serde(rename = "receiptChainHash")]
    pub receipt_chain_hash: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GraphQLResponse<T> {
    data: Option<T>,
    errors: Option<Vec<GraphQLError>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GraphQLError {
    message: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct BalanceQueryResponse {
    account: Option<BalanceAccount>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BalanceAccount {
    balance: AccountBalance,
}

#[derive(Debug, Serialize, Deserialize)]
struct AccountQueryResponse {
    account: Option<AccountInfo>,
}

/// Fetch balance for a given public key from a specific network
pub async fn get_balance(address: &str, network: &str) -> Result<u64> {
    get_balance_with_token(address, network, DEFAULT_TOKEN_ID).await
}

/// Fetch balance for a given public key and token from a specific network
pub async fn get_balance_with_token(address: &str, network: &str, token_id: &str) -> Result<u64> {
    let network_config = MinaNetwork::get_network(network)
        .ok_or_else(|| anyhow!("Unsupported network: {}", network))?;
    
    if network_config.mina.is_empty() {
        return Err(anyhow!("No GraphQL endpoints configured for network: {}", network));
    }
    
    // Try each endpoint until one succeeds
    let mut last_error = None;
    for endpoint in &network_config.mina {
        match fetch_balance_from_endpoint(address, token_id, endpoint).await {
            Ok(balance) => return Ok(balance),
            Err(e) => last_error = Some(e),
        }
    }
    
    Err(last_error.unwrap_or_else(|| anyhow!("Failed to fetch balance")))
}

/// Fetch balance from a specific GraphQL endpoint
async fn fetch_balance_from_endpoint(address: &str, token_id: &str, endpoint: &str) -> Result<u64> {
    let query = format!(
        r#"{{
            account(publicKey: "{}", token: "{}") {{
                balance {{ total }}
            }}
        }}"#,
        address, token_id
    );
    
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    
    let request_body = json!({
        "operationName": null,
        "query": query,
        "variables": {}
    });
    
    let response = client
        .post(endpoint)
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| anyhow!("Failed to send GraphQL request: {}", e))?;
    
    if !response.status().is_success() {
        return Err(anyhow!(
            "GraphQL request failed with status {}: {}",
            response.status(),
            response.text().await.unwrap_or_else(|_| "Unknown error".to_string())
        ));
    }
    
    let response_text = response.text().await?;
    let graphql_response: GraphQLResponse<BalanceQueryResponse> = 
        serde_json::from_str(&response_text)
            .map_err(|e| anyhow!("Failed to parse GraphQL response: {}", e))?;
    
    // Check for GraphQL errors
    if let Some(errors) = graphql_response.errors {
        if !errors.is_empty() {
            return Err(anyhow!(
                "GraphQL errors: {}",
                errors.iter().map(|e| &e.message).cloned().collect::<Vec<_>>().join(", ")
            ));
        }
    }
    
    // Extract balance
    let balance_str = graphql_response
        .data
        .and_then(|d| d.account)
        .map(|a| a.balance.total)
        .ok_or_else(|| anyhow!("Account not found or no balance data"))?;
    
    // Parse balance (it's in nanomina, 1 MINA = 1e9 nanomina)
    balance_str.parse::<u64>()
        .map_err(|e| anyhow!("Failed to parse balance: {}", e))
}

/// Fetch full account information from a specific network
pub async fn get_account_info(address: &str, network: &str) -> Result<AccountInfo> {
    get_account_info_with_token(address, network, DEFAULT_TOKEN_ID).await
}

/// Fetch full account information with a specific token from a network
pub async fn get_account_info_with_token(address: &str, network: &str, token_id: &str) -> Result<AccountInfo> {
    let network_config = MinaNetwork::get_network(network)
        .ok_or_else(|| anyhow!("Unsupported network: {}", network))?;
    
    if network_config.mina.is_empty() {
        return Err(anyhow!("No GraphQL endpoints configured for network: {}", network));
    }
    
    // Try each endpoint until one succeeds
    let mut last_error = None;
    for endpoint in &network_config.mina {
        match fetch_account_from_endpoint(address, token_id, endpoint).await {
            Ok(account) => return Ok(account),
            Err(e) => last_error = Some(e),
        }
    }
    
    Err(last_error.unwrap_or_else(|| anyhow!("Failed to fetch account info")))
}

/// Fetch account information from a specific GraphQL endpoint
async fn fetch_account_from_endpoint(address: &str, token_id: &str, endpoint: &str) -> Result<AccountInfo> {
    let query = format!(
        r#"{{
            account(publicKey: "{}", token: "{}") {{
                publicKey
                token
                nonce
                balance {{ total }}
                tokenSymbol
                receiptChainHash
            }}
        }}"#,
        address, token_id
    );
    
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    
    let request_body = json!({
        "operationName": null,
        "query": query,
        "variables": {}
    });
    
    let response = client
        .post(endpoint)
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| anyhow!("Failed to send GraphQL request: {}", e))?;
    
    if !response.status().is_success() {
        return Err(anyhow!(
            "GraphQL request failed with status {}: {}",
            response.status(),
            response.text().await.unwrap_or_else(|_| "Unknown error".to_string())
        ));
    }
    
    let response_text = response.text().await?;
    let graphql_response: GraphQLResponse<AccountQueryResponse> = 
        serde_json::from_str(&response_text)
            .map_err(|e| anyhow!("Failed to parse GraphQL response: {}", e))?;
    
    // Check for GraphQL errors
    if let Some(errors) = graphql_response.errors {
        if !errors.is_empty() {
            return Err(anyhow!(
                "GraphQL errors: {}",
                errors.iter().map(|e| &e.message).cloned().collect::<Vec<_>>().join(", ")
            ));
        }
    }
    
    // Extract account info
    graphql_response
        .data
        .and_then(|d| d.account)
        .ok_or_else(|| anyhow!("Account not found"))
}

/// Convert balance from nanomina to MINA
pub fn nanomina_to_mina(nanomina: u64) -> f64 {
    nanomina as f64 / 1_000_000_000.0
}

/// Convert balance from MINA to nanomina
pub fn mina_to_nanomina(mina: f64) -> u64 {
    (mina * 1_000_000_000.0) as u64
}

/// Get balance in MINA (as f64) for a given address on a network
pub async fn get_balance_in_mina(address: &str, network: &str) -> Result<f64> {
    let balance = get_balance(address, network).await?;
    Ok(nanomina_to_mina(balance))
}

/// Check if an account exists on a specific network
pub async fn account_exists(address: &str, network: &str) -> Result<bool> {
    match get_account_info(address, network).await {
        Ok(_) => Ok(true),
        Err(e) => {
            if e.to_string().contains("Account not found") {
                Ok(false)
            } else {
                Err(e)
            }
        }
    }
}