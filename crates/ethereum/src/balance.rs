use alloy::providers::{Provider, ProviderBuilder};
use alloy::primitives::{Address, U256};
use anyhow::{Result, anyhow};
use crate::networks::EthereumNetwork;

/// Get balance in wei for an address on a specific network
pub async fn get_balance_wei(address: &str, network: &str) -> Result<U256> {
    let network_config = EthereumNetwork::get_network(network)
        .ok_or_else(|| anyhow!("Unsupported network: {}", network))?;
    
    if network_config.rpc_endpoints.is_empty() {
        return Err(anyhow!("No RPC endpoints configured for network: {}", network));
    }
    
    // Parse the address
    let addr: Address = address.parse()
        .map_err(|e| anyhow!("Invalid Ethereum address {}: {}", address, e))?;
    
    // Try each endpoint until one succeeds
    let mut last_error = None;
    for endpoint in &network_config.rpc_endpoints {
        match fetch_balance_from_endpoint(addr, endpoint).await {
            Ok(balance) => return Ok(balance),
            Err(e) => {
                last_error = Some(e);
                // Continue to next endpoint
            }
        }
    }
    
    Err(last_error.unwrap_or_else(|| anyhow!("Failed to fetch balance")))
}

/// Fetch balance from a specific RPC endpoint
async fn fetch_balance_from_endpoint(address: Address, endpoint: &str) -> Result<U256> {
    // Create provider
    let provider = ProviderBuilder::new()
        .connect_http(endpoint.parse()?);
    
    // Get balance
    let balance = provider.get_balance(address)
        .await
        .map_err(|e| anyhow!("Failed to fetch balance from {}: {}", endpoint, e))?;
    
    Ok(balance)
}

/// Get balance in ETH (or native token) as f64
pub async fn get_balance(address: &str, network: &str) -> Result<f64> {
    let network_config = EthereumNetwork::get_network(network)
        .ok_or_else(|| anyhow!("Unsupported network: {}", network))?;
    
    let balance_wei = get_balance_wei(address, network).await?;
    let decimals = network_config.native_currency.decimals;
    
    Ok(wei_to_ether(balance_wei, decimals))
}

/// Convert wei to ether (or native token with specified decimals)
pub fn wei_to_ether(wei: U256, decimals: u8) -> f64 {
    let divisor = U256::from(10u64).pow(U256::from(decimals));
    let whole = wei / divisor;
    let fraction = wei % divisor;
    
    // Convert to f64 (may lose precision for very large numbers)
    let whole_f64 = whole.to_string().parse::<f64>().unwrap_or(0.0);
    let fraction_f64 = fraction.to_string().parse::<f64>().unwrap_or(0.0) / (10_f64.powi(decimals as i32));
    
    whole_f64 + fraction_f64
}

/// Convert ether (or native token) to wei
pub fn ether_to_wei(ether: f64, decimals: u8) -> U256 {
    let wei_per_ether = 10_f64.powi(decimals as i32);
    let wei = (ether * wei_per_ether) as u128;
    U256::from(wei)
}

/// Check if an account exists (has any transaction history)
pub async fn account_exists(address: &str, network: &str) -> Result<bool> {
    let network_config = EthereumNetwork::get_network(network)
        .ok_or_else(|| anyhow!("Unsupported network: {}", network))?;
    
    if network_config.rpc_endpoints.is_empty() {
        return Err(anyhow!("No RPC endpoints configured for network: {}", network));
    }
    
    // Parse the address
    let addr: Address = address.parse()
        .map_err(|e| anyhow!("Invalid Ethereum address {}: {}", address, e))?;
    
    // Try to get the transaction count (nonce)
    for endpoint in &network_config.rpc_endpoints {
        match check_account_at_endpoint(addr, endpoint).await {
            Ok(exists) => return Ok(exists),
            Err(_) => continue, // Try next endpoint
        }
    }
    
    // If we can't determine, assume it doesn't exist
    Ok(false)
}

/// Check if account exists at a specific endpoint
async fn check_account_at_endpoint(address: Address, endpoint: &str) -> Result<bool> {
    let provider = ProviderBuilder::new()
        .connect_http(endpoint.parse()?);
    
    // Get transaction count (nonce)
    let nonce = provider.get_transaction_count(address)
        .await
        .map_err(|e| anyhow!("Failed to get transaction count: {}", e))?;
    
    // If nonce > 0, account has sent transactions
    // If balance > 0, account has received funds
    // Check both
    if nonce > 0 {
        return Ok(true);
    }
    
    let balance = provider.get_balance(address)
        .await
        .map_err(|e| anyhow!("Failed to get balance: {}", e))?;
    
    Ok(balance > U256::ZERO)
}

/// Get account information (balance and nonce)
pub async fn get_account_info(address: &str, network: &str) -> Result<AccountInfo> {
    let network_config = EthereumNetwork::get_network(network)
        .ok_or_else(|| anyhow!("Unsupported network: {}", network))?;
    
    if network_config.rpc_endpoints.is_empty() {
        return Err(anyhow!("No RPC endpoints configured for network: {}", network));
    }
    
    // Parse the address
    let addr: Address = address.parse()
        .map_err(|e| anyhow!("Invalid Ethereum address {}: {}", address, e))?;
    
    // Try each endpoint until one succeeds
    let mut last_error = None;
    for endpoint in &network_config.rpc_endpoints {
        match fetch_account_info_from_endpoint(addr, endpoint, &network_config).await {
            Ok(info) => return Ok(info),
            Err(e) => {
                last_error = Some(e);
                continue;
            }
        }
    }
    
    Err(last_error.unwrap_or_else(|| anyhow!("Failed to fetch account info")))
}

/// Fetch account info from a specific endpoint
async fn fetch_account_info_from_endpoint(
    address: Address, 
    endpoint: &str,
    network: &EthereumNetwork
) -> Result<AccountInfo> {
    let provider = ProviderBuilder::new()
        .connect_http(endpoint.parse()?);
    
    // Get balance and nonce in parallel
    let (balance, nonce) = tokio::try_join!(
        provider.get_balance(address),
        provider.get_transaction_count(address)
    ).map_err(|e| anyhow!("Failed to fetch account data: {}", e))?;
    
    Ok(AccountInfo {
        address: format!("{:?}", address),
        balance_wei: balance,
        balance: wei_to_ether(balance, network.native_currency.decimals),
        nonce,
        network: network.name.clone(),
        symbol: network.native_currency.symbol.clone(),
    })
}

#[derive(Debug)]
pub struct AccountInfo {
    pub address: String,
    pub balance_wei: U256,
    pub balance: f64,
    pub nonce: u64,
    pub network: String,
    pub symbol: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_wei_conversion() {
        // 1 ETH = 10^18 wei
        let one_eth_wei = U256::from(1_000_000_000_000_000_000u128);
        let eth = wei_to_ether(one_eth_wei, 18);
        assert!((eth - 1.0).abs() < 0.0001);
        
        // Test conversion back
        let wei = ether_to_wei(1.0, 18);
        assert_eq!(wei, one_eth_wei);
        
        // Test with different decimals (e.g., USDC has 6 decimals)
        let one_usdc_wei = U256::from(1_000_000u64);
        let usdc = wei_to_ether(one_usdc_wei, 6);
        assert!((usdc - 1.0).abs() < 0.0001);
    }
    
    #[tokio::test]
    async fn test_balance_localhost() {
        // This test requires a local Ethereum node running
        // Skip if not available
        if let Ok(balance) = get_balance("0x0000000000000000000000000000000000000000", "localhost").await {
            println!("Zero address balance on localhost: {} ETH", balance);
            assert!(balance >= 0.0);
        }
    }
}