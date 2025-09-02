use anyhow::Result;
use crate::state::SharedSuiState;
use crate::coin::{list_coins, CoinInfo};
use crate::coin_management::{CoinPoolConfig, get_gas_coins_info, GasCoinsInfo};
use std::env;

/// Information about the account balance
#[derive(Debug)]
pub struct BalanceInfo {
    pub address: String,
    pub total_balance: u64,
    pub total_balance_sui: f64,
    pub coins: Vec<CoinInfo>,
    pub gas_coins_info: GasCoinsInfo,
}

/// Get complete balance information for the current account
pub async fn get_balance_info() -> Result<BalanceInfo> {
    let shared_state = SharedSuiState::get_instance();
    let mut client = shared_state.get_sui_client();
    let address = shared_state.get_sui_address();
    
    // List all coins
    let coins = list_coins(&mut client, address).await?;
    let total_balance: u64 = coins.iter().map(|c| c.balance).sum();
    let total_balance_sui = total_balance as f64 / 1_000_000_000.0;
    
    // Get gas coin pool info
    let config = CoinPoolConfig::default();
    let gas_coins_info = get_gas_coins_info(&config).await?;
    
    Ok(BalanceInfo {
        address: address.to_string(),
        total_balance,
        total_balance_sui,
        coins,
        gas_coins_info,
    })
}

/// Get just the total balance in SUI
pub async fn get_total_balance_sui() -> Result<f64> {
    let shared_state = SharedSuiState::get_instance();
    let mut client = shared_state.get_sui_client();
    let address = shared_state.get_sui_address();
    
    let coins = list_coins(&mut client, address).await?;
    let total_balance: u64 = coins.iter().map(|c| c.balance).sum();
    Ok(total_balance as f64 / 1_000_000_000.0)
}

/// Get balance for a specific address in SUI
pub async fn get_balance_in_sui(address_str: &str) -> Result<f64> {
    use sui_sdk_types::Address;
    use std::str::FromStr;
    
    let shared_state = SharedSuiState::get_instance();
    let mut client = shared_state.get_sui_client();
    
    // Parse the address
    let address = Address::from_str(address_str)?;
    
    let coins = list_coins(&mut client, address).await?;
    let total_balance: u64 = coins.iter().map(|c| c.balance).sum();
    Ok(total_balance as f64 / 1_000_000_000.0)
}

/// Get the current address
pub fn get_current_address() -> String {
    let shared_state = SharedSuiState::get_instance();
    shared_state.get_sui_address().to_string()
}

/// Get the current network name from SUI_CHAIN env var
pub fn get_network_name() -> String {
    env::var("SUI_CHAIN").unwrap_or_else(|_| "unknown".to_string())
}

/// Print formatted balance information
pub async fn print_balance_info() -> Result<()> {
    let balance_info = get_balance_info().await?;
    let env_network = get_network_name();
    
    // Try to get actual network info from RPC
    if let Ok(info) = crate::network_info::get_network_info().await {
        // If RPC provides a chain name, use it; otherwise use env var
        if let Some(ref chain_name) = info.chain_name {
            // RPC provided a meaningful chain name
            if chain_name != &env_network && env_network != "unknown" {
                println!("Network: {} (⚠️ env says {})", chain_name, env_network);
            } else {
                println!("Network: {}", chain_name);
            }
        } else {
            // RPC didn't provide chain name or returned "unknown", use env var
            println!("Network: {} (from env)", env_network);
        }
        
        // Show summary
        let summary = format!(
            "Epoch {} | Protocol v{} | {} validators | Gas: {} MIST",
            info.epoch, info.protocol_version, info.validator_count, info.reference_gas_price
        );
        println!("Stats: {}", summary);
    } else {
        println!("Network: {} (from env)", env_network);
    }
    
    println!("Address: {}", balance_info.address);
    println!("Total balance: {} SUI ({} MIST)", balance_info.total_balance_sui, balance_info.total_balance);
    println!("Number of coins: {}", balance_info.coins.len());
    
    let config = CoinPoolConfig::default();
    println!("\nGas coin pool status:");
    println!("  Suitable coins (0.1-0.4 SUI): {}/{}", 
        balance_info.gas_coins_info.suitable_coins, 
        config.target_gas_coins
    );
    println!("  Total coins: {}", balance_info.gas_coins_info.total_coins);
    
    if let Some(faucet_coin) = balance_info.gas_coins_info.faucet_coin {
        let faucet_balance_sui = faucet_coin.balance as f64 / 1_000_000_000.0;
        println!("  Largest coin: {} SUI (suitable for splitting)", faucet_balance_sui);
    } else {
        println!("  No coin large enough for splitting (need > 5 SUI)");
    }
    
    // Show individual coins if not too many
    if balance_info.coins.len() <= 30 {
        println!("\nIndividual coins:");
        for coin in balance_info.coins.iter() {
            let coin_balance_sui = coin.balance as f64 / 1_000_000_000.0;
            println!("  {} - {} SUI", &coin.object_id().to_string()[..16], coin_balance_sui);
        }
    }
    
    Ok(())
}