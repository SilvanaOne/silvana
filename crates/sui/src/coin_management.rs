use anyhow::{Result, Context, anyhow};
use sui_sdk_types::{Argument};
use sui_transaction_builder::{TransactionBuilder, Serialized};
use sui_crypto::SuiSigner;
use sui_rpc::proto::sui::rpc::v2beta2 as proto;
use tracing::{info, debug, warn, error};

use crate::chain::get_reference_gas_price;
use crate::coin::{list_coins, CoinInfo};
use crate::state::SharedSuiState;

/// Configuration for coin pool management
pub struct CoinPoolConfig {
    /// Target number of gas coins to maintain
    pub target_gas_coins: usize,
    /// Target balance for each gas coin (in MIST)
    pub target_coin_balance: u64,
    /// Minimum balance for a coin to be considered usable (in MIST)
    pub min_coin_balance: u64,
    /// Minimum balance needed in source coin for splitting
    pub min_faucet_coin_balance: u64,
}

impl Default for CoinPoolConfig {
    fn default() -> Self {
        Self {
            target_gas_coins: 20,
            target_coin_balance: 200_000_000,  // 0.2 SUI per coin
            min_coin_balance: 100_000_000,     // 0.1 SUI minimum
            min_faucet_coin_balance: 5_000_000_000, // 5 SUI minimum for splitting
        }
    }
}

/// Information about available gas coins
#[derive(Debug)]
pub struct GasCoinsInfo {
    /// Number of coins in the target balance range
    pub suitable_coins: usize,
    /// Coin that can be used for splitting (if any)
    pub faucet_coin: Option<CoinInfo>,
    /// Number of coins that can be used for splitting (>= 2 SUI each)
    pub splittable_coins: usize,
    /// Total number of coins
    pub total_coins: usize,
}

/// Get information about available gas coins
pub async fn get_gas_coins_info(config: &CoinPoolConfig) -> Result<GasCoinsInfo> {
    let shared_state = SharedSuiState::get_instance();
    let mut client = shared_state.get_sui_client();
    let sender = shared_state.get_sui_address();
    
    // List all available coins
    let coins = list_coins(&mut client, sender).await?;
    let total_coins = coins.len();
    
    // Count coins in the target balance range
    let suitable_coins = coins.iter()
        .filter(|c| c.balance >= config.min_coin_balance && c.balance <= config.target_coin_balance * 2)
        .count();
    
    // Count coins that can be used for splitting (>= 2 SUI)
    let splittable_coins = coins.iter()
        .filter(|c| c.balance >= config.min_faucet_coin_balance)
        .count();
    
    // Find a coin suitable for splitting
    let faucet_coin = coins.into_iter()
        .filter(|c| c.balance >= config.min_faucet_coin_balance)
        .max_by_key(|c| c.balance);
    
    Ok(GasCoinsInfo {
        suitable_coins,
        faucet_coin,
        splittable_coins,
        total_coins,
    })
}

/// Split a large coin into multiple smaller coins for parallel execution
/// Returns the transaction digest if successful
pub async fn split_gas_coins(
    source_coin: CoinInfo,
    num_coins: usize,
    coin_balance: u64,
) -> Result<String> {
    let shared_state = SharedSuiState::get_instance();
    let mut client = shared_state.get_sui_client();
    let sender = shared_state.get_sui_address();
    let secret_key = shared_state.get_sui_private_key();
    
    info!(
        "Splitting coin {} (balance: {} MIST) into {} coins of {} MIST each",
        source_coin.object_id(), source_coin.balance, num_coins, coin_balance
    );
    
    // Verify we have enough balance for the split plus gas
    let total_needed = (num_coins as u64 * coin_balance) + 50_000_000;
    if source_coin.balance < total_needed {
        return Err(anyhow!(
            "Insufficient balance for split: have {} MIST, need {} MIST",
            source_coin.balance, total_needed
        ));
    }
    
    debug!("Coin details: id={}, version={}, digest={}", 
        source_coin.object_id(), 
        source_coin.object_ref.version(),
        source_coin.object_ref.digest()
    );
    
    // Build the transaction
    let mut tb = TransactionBuilder::new();
    tb.set_sender(sender);
    tb.set_gas_budget(50_000_000); // 0.05 SUI for gas
    
    // Set gas price
    let gas_price = get_reference_gas_price(&mut client).await?;
    tb.set_gas_price(gas_price);
    debug!("Gas price for split transaction: {}", gas_price);
    
    // Set the source coin as gas payment
    let gas_input = sui_transaction_builder::unresolved::Input::owned(
        source_coin.object_id(),
        source_coin.object_ref.version(),
        *source_coin.object_ref.digest(),
    );
    tb.add_gas_objects(vec![gas_input]);
    
    // Build split amounts - create inputs for each amount
    let amounts: Vec<Argument> = (0..num_coins)
        .map(|_| tb.input(Serialized(&coin_balance)))
        .collect();
    
    // Split the gas coin
    let split_result = tb.split_coins(Argument::Gas, amounts);
    
    // Transfer the split coins back to sender
    // split_result is an Argument::Result that contains all the split coins
    // We need to use nested to access each individual coin from the result
    let coin_args: Vec<Argument> = (0..num_coins)
        .map(|i| {
            // nested returns Option<Argument>, and we know it will succeed for valid indices
            split_result.nested(i as u16).expect("Invalid nested index")
        })
        .collect();
    
    let recipient = tb.input(Serialized(&sender));
    tb.transfer_objects(coin_args, recipient);
    
    // Finalize and sign the transaction
    let tx = tb.finish()?;
    let signature = secret_key.sign_transaction(&tx)?;
    
    // Execute the transaction
    let mut exec_client = client.execution_client();
    let req = proto::ExecuteTransactionRequest {
        transaction: Some(tx.into()),
        signatures: vec![signature.into()],
        read_mask: Some(sui_rpc::field::FieldMask {
            paths: vec![
                "transaction".into(),
                "transaction.digest".into(), 
                "transaction.effects".into(),
                "transaction.effects.status".into(),
            ],
        }),
    };
    
    debug!("Executing split transaction...");
    let resp = exec_client.execute_transaction(req).await
        .map_err(|e| {
            error!("gRPC error executing split transaction: {}", e);
            anyhow!("Failed to execute split transaction: {}", e)
        })?;
    
    let inner_resp = resp.into_inner();
    
    // Check transaction effects for any errors
    if let Some(ref transaction) = inner_resp.transaction {
        if let Some(ref effects) = transaction.effects {
            if let Some(ref status) = effects.status {
                // status is ExecutionStatus
                if let Some(success) = status.success {
                    if !success {
                        error!("Split transaction failed. Error: {:?}", status.error);
                        return Err(anyhow!("Split transaction failed: {:?}", status.error));
                    } else {
                        debug!("Split transaction executed successfully");
                    }
                }
            }
        }
    }
    
    let tx_digest = inner_resp
        .transaction
        .and_then(|t| t.digest)
        .context("Failed to get transaction digest")?;
    
    info!("Split transaction executed successfully: {}", tx_digest);
    
    // Wait for transaction to be confirmed
    wait_for_split_transaction(&tx_digest).await?;
    
    Ok(tx_digest.to_string())
}

/// Wait for a split transaction to be confirmed
async fn wait_for_split_transaction(digest: &str) -> Result<()> {
    let shared_state = SharedSuiState::get_instance();
    let mut client = shared_state.get_sui_client();
    let mut ledger = client.ledger_client();
    
    let max_retries = 20; // More retries for split transactions
    for i in 0..max_retries {
        let req = proto::GetTransactionRequest {
            digest: Some(digest.to_string()),
            read_mask: Some(sui_rpc::field::FieldMask {
                paths: vec!["transaction".into()],
            }),
        };
        
        match ledger.get_transaction(req).await {
            Ok(resp) => {
                let inner = resp.into_inner();
                if inner.transaction.is_some() {
                    // Transaction exists, consider it successful
                    debug!("Split transaction {} found in ledger", digest);
                    return Ok(());
                }
                // Transaction not found yet
                if i < max_retries - 1 {
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                }
            }
            Err(_) if i < max_retries - 1 => {
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
            Err(e) => {
                return Err(anyhow!("Split transaction not available after {} retries: {}", max_retries, e));
            }
        }
    }
    
    warn!("Split transaction {} not finalized after {} retries, continuing anyway", digest, max_retries);
    Ok(())
}

/// Check and split coins if needed to maintain the coin pool
pub async fn ensure_gas_coin_pool() -> Result<()> {
    let config = CoinPoolConfig::default();
    let gas_info = get_gas_coins_info(&config).await?;
    
    info!(
        "Gas coin status: {} suitable coins, {} splittable coins out of {} total (target: {} suitable)",
        gas_info.suitable_coins, gas_info.splittable_coins, gas_info.total_coins, config.target_gas_coins
    );
    
    // Always ensure we have at least 2 coins that can be split (for emergency use)
    // Check if we need to split coins for gas operations OR if we don't have enough splittable coins
    if gas_info.suitable_coins >= config.target_gas_coins && gas_info.splittable_coins >= 2 {
        debug!("Sufficient gas coins available, no splitting needed");
        return Ok(());
    }
    
    // Check if we have a coin to split
    let source_coin = match gas_info.faucet_coin {
        Some(coin) => coin,
        None => {
            warn!(
                "No coin with sufficient balance for splitting (need at least {} MIST)",
                config.min_faucet_coin_balance
            );
            
            // Try to get tokens from faucet if we don't have enough
            // Request more tokens to ensure we have splittable coins
            info!("Attempting to request tokens from faucet for coin splitting...");
            if let Err(e) = crate::faucet::ensure_sufficient_balance(10.0).await {
                warn!("Failed to request faucet tokens: {}", e);
            }
            
            // Try one more time to find a suitable coin after faucet
            let gas_info_after = get_gas_coins_info(&config).await?;
            match gas_info_after.faucet_coin {
                Some(coin) => coin,
                None => {
                    warn!("Still no suitable coin for splitting after faucet attempt");
                    return Ok(());
                }
            }
        }
    };
    
    // Calculate how many coins to create
    let coins_needed = config.target_gas_coins - gas_info.suitable_coins;
    let coins_to_create = coins_needed.min(config.target_gas_coins); // Don't create too many at once
    
    // Check if source coin has enough balance
    let required_balance = (coins_to_create as u64 * config.target_coin_balance) + 50_000_000; // Plus gas
    if source_coin.balance < required_balance {
        warn!(
            "Source coin has insufficient balance for splitting {} coins (have: {}, need: {})",
            coins_to_create, source_coin.balance, required_balance
        );
        return Ok(());
    }
    
    // Execute the split
    match split_gas_coins(source_coin, coins_to_create, config.target_coin_balance).await {
        Ok(tx_digest) => {
            info!(
                "Successfully split {} gas coins (tx: {})",
                coins_to_create, tx_digest
            );
            Ok(())
        }
        Err(e) => {
            error!("Failed to split gas coins: {}", e);
            // Don't fail hard, the system can work with fewer coins
            Ok(())
        }
    }
}

/// Initialize the gas coin pool on startup
pub async fn initialize_gas_coin_pool() -> Result<()> {
    info!("Initializing gas coin pool...");
    
    // Check current state
    let config = CoinPoolConfig::default();
    let gas_info = get_gas_coins_info(&config).await?;
    
    info!(
        "Initial gas coin state: {} suitable coins, {} total coins",
        gas_info.suitable_coins, gas_info.total_coins
    );
    
    // Only split if we have very few suitable coins
    if gas_info.suitable_coins < 5 {
        info!("Low number of suitable gas coins, attempting to split...");
        ensure_gas_coin_pool().await?;
    }
    
    Ok(())
}