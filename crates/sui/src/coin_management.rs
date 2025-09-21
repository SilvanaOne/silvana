use anyhow::{Context, Result, anyhow};
use sui_crypto::SuiSigner;
use sui_rpc::proto::sui::rpc::v2 as proto;
use sui_sdk_types::{Address, Argument};
use sui_transaction_builder::{Serialized, TransactionBuilder};
use tracing::{debug, error, info, warn};

use crate::chain::get_reference_gas_price;
use crate::coin::{CoinInfo, list_coins};
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
            target_gas_coins: 9,
            target_coin_balance: 1_000_000_000, // 1.0 SUI per coin
            min_coin_balance: 500_000_000,      // 0.5 SUI minimum
            min_faucet_coin_balance: 9_100_000_000, // 5 SUI minimum for splitting
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
    /// Coins with balance <= min_coin_balance (dust that should be merged)
    pub dust_coins: Vec<CoinInfo>,
}

/// Get information about available gas coins
pub async fn get_gas_coins_info(config: &CoinPoolConfig, address: Address) -> Result<GasCoinsInfo> {
    let shared_state = SharedSuiState::get_instance();
    let mut client = shared_state.get_sui_client();

    // List all available coins
    let coins = list_coins(&mut client, address).await?;
    let total_coins = coins.len();

    // Collect dust coins (balance <= min_coin_balance)
    let dust_coins: Vec<CoinInfo> = coins
        .iter()
        .filter(|c| c.balance < config.min_coin_balance)
        .cloned()
        .collect();

    // Count coins in the target balance range (suitable for gas)
    let suitable_coins = coins
        .iter()
        .filter(|c| {
            c.balance >= config.min_coin_balance && c.balance <= config.target_coin_balance * 2
        })
        .count();

    // Count coins that can be used for splitting (>= min_faucet_coin_balance)
    let splittable_coins = coins
        .iter()
        .filter(|c| c.balance >= config.min_faucet_coin_balance)
        .count();

    // Find a coin suitable for splitting (choose the smallest one that's still splittable)
    let faucet_coin = coins
        .into_iter()
        .filter(|c| c.balance >= config.min_faucet_coin_balance)
        .min_by_key(|c| c.balance);

    Ok(GasCoinsInfo {
        suitable_coins,
        faucet_coin,
        splittable_coins,
        total_coins,
        dust_coins,
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
    let sender = shared_state.get_sui_address_required();
    let secret_key = shared_state.get_sui_private_key_required();

    info!(
        "Splitting coin {} (balance: {} MIST) into {} coins of {} MIST each",
        source_coin.object_id(),
        source_coin.balance,
        num_coins,
        coin_balance
    );

    // Verify we have enough balance for the split plus gas
    let total_needed = (num_coins as u64 * coin_balance) + 50_000_000;
    if source_coin.balance < total_needed {
        return Err(anyhow!(
            "Insufficient balance for split: have {} MIST, need {} MIST",
            source_coin.balance,
            total_needed
        ));
    }

    debug!(
        "Coin details: id={}, version={}, digest={}",
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
    let mut req = proto::ExecuteTransactionRequest::default();
    req.transaction = Some(tx.into());
    req.signatures = vec![signature.into()];
    req.read_mask = Some(sui_rpc::field::FieldMask {
        paths: vec![
            "transaction".into(),
            "transaction.digest".into(),
            "transaction.effects".into(),
            "transaction.effects.status".into(),
        ],
    });

    debug!("Executing split transaction...");
    let resp = exec_client.execute_transaction(req).await.map_err(|e| {
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
        let mut req = proto::GetTransactionRequest::default();
        req.digest = Some(digest.to_string());
        req.read_mask = Some(sui_rpc::field::FieldMask {
            paths: vec!["transaction".into()],
        });

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
                return Err(anyhow!(
                    "Split transaction not available after {} retries: {}",
                    max_retries,
                    e
                ));
            }
        }
    }

    warn!(
        "Split transaction {} not finalized after {} retries, continuing anyway",
        digest, max_retries
    );
    Ok(())
}

/// Check and split coins if needed to maintain the coin pool
pub async fn ensure_gas_coin_pool() -> Result<()> {
    let shared_state = SharedSuiState::get_instance();
    let sender = shared_state.get_sui_address_required();
    let config = CoinPoolConfig::default();
    let mut gas_info = get_gas_coins_info(&config, sender).await?;

    info!(
        "Gas coin status: {} suitable coins, {} splittable coins out of {} total (target: {} suitable)",
        gas_info.suitable_coins,
        gas_info.splittable_coins,
        gas_info.total_coins,
        config.target_gas_coins
    );

    // Check if we have too many dust coins that should be merged
    if gas_info.dust_coins.len() >= 10 {
        info!(
            "Found {} dust coins, merging them to reduce clutter",
            gas_info.dust_coins.len()
        );

        // Merge all dust coins
        match merge_gas_coins(gas_info.dust_coins.clone()).await {
            Ok(tx_digest) => {
                info!(
                    "Successfully merged {} dust coins (tx: {})",
                    gas_info.dust_coins.len(),
                    tx_digest
                );

                // Re-fetch gas info after merging
                gas_info = get_gas_coins_info(&config, sender).await?;

                // Check if we still need to split after merging
                if gas_info.suitable_coins >= config.target_gas_coins
                    && gas_info.splittable_coins >= 2
                {
                    debug!("Sufficient gas coins available after merging, no splitting needed");
                    return Ok(());
                }
            }
            Err(e) => {
                warn!("Failed to merge dust coins: {}", e);
                // Continue with splitting logic even if merge failed
            }
        }
    }

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
            gas_info = get_gas_coins_info(&config, sender).await?;
            match gas_info.faucet_coin {
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

/// Merge multiple coins into a single coin
/// The first coin in the vector will be the target coin that all others merge into
/// Returns the transaction digest if successful
pub async fn merge_gas_coins(coins_to_merge: Vec<CoinInfo>) -> Result<String> {
    if coins_to_merge.len() < 2 {
        return Err(anyhow!("Need at least 2 coins to merge"));
    }

    let shared_state = SharedSuiState::get_instance();
    let mut client = shared_state.get_sui_client();
    let sender = shared_state.get_sui_address_required();
    let secret_key = shared_state.get_sui_private_key_required();

    // Calculate total balance that will be merged
    let total_balance: u64 = coins_to_merge.iter().map(|c| c.balance).sum();

    info!(
        "Merging {} coins with total balance {} MIST into coin {}",
        coins_to_merge.len(),
        total_balance,
        coins_to_merge[0].object_id()
    );

    // Build the transaction
    let mut tb = TransactionBuilder::new();
    tb.set_sender(sender);
    tb.set_gas_budget(10_000_000); // 0.01 SUI for gas

    // Set gas price
    let gas_price = get_reference_gas_price(&mut client).await?;
    tb.set_gas_price(gas_price);
    debug!("Gas price for merge transaction: {}", gas_price);

    // Use the first coin as the target and gas payment
    let target_coin = &coins_to_merge[0];
    let gas_input = sui_transaction_builder::unresolved::Input::owned(
        target_coin.object_id(),
        target_coin.object_ref.version(),
        *target_coin.object_ref.digest(),
    );
    tb.add_gas_objects(vec![gas_input.clone()]);

    // Create input objects for the coins to merge (excluding the first one which is the target)
    let coins_to_merge_inputs: Vec<Argument> = coins_to_merge[1..]
        .iter()
        .map(|coin| {
            let input = sui_transaction_builder::unresolved::Input::owned(
                coin.object_id(),
                coin.object_ref.version(),
                *coin.object_ref.digest(),
            );
            tb.input(input)
        })
        .collect();

    // Merge all coins into the gas coin
    tb.merge_coins(Argument::Gas, coins_to_merge_inputs);

    // Finalize and sign the transaction
    let tx = tb.finish()?;
    let signature = secret_key.sign_transaction(&tx)?;

    // Execute the transaction
    let mut exec_client = client.execution_client();
    let mut req = proto::ExecuteTransactionRequest::default();
    req.transaction = Some(tx.into());
    req.signatures = vec![signature.into()];
    req.read_mask = Some(sui_rpc::field::FieldMask {
        paths: vec![
            "transaction".into(),
            "transaction.digest".into(),
            "transaction.effects".into(),
            "transaction.effects.status".into(),
        ],
    });

    debug!("Executing merge transaction...");
    let resp = exec_client.execute_transaction(req).await.map_err(|e| {
        error!("gRPC error executing merge transaction: {}", e);
        anyhow!("Failed to execute merge transaction: {}", e)
    })?;

    let inner_resp = resp.into_inner();

    // Check transaction effects for any errors
    if let Some(ref transaction) = inner_resp.transaction {
        if let Some(ref effects) = transaction.effects {
            if let Some(ref status) = effects.status {
                if let Some(success) = status.success {
                    if !success {
                        error!("Merge transaction failed. Error: {:?}", status.error);
                        return Err(anyhow!("Merge transaction failed: {:?}", status.error));
                    } else {
                        debug!("Merge transaction executed successfully");
                    }
                }
            }
        }
    }

    let tx_digest = inner_resp
        .transaction
        .and_then(|t| t.digest)
        .context("Failed to get transaction digest")?;

    info!("Merge transaction executed successfully: {}", tx_digest);

    // Wait for transaction to be confirmed
    wait_for_merge_transaction(&tx_digest).await?;

    Ok(tx_digest.to_string())
}

/// Wait for a merge transaction to be confirmed
async fn wait_for_merge_transaction(digest: &str) -> Result<()> {
    let shared_state = SharedSuiState::get_instance();
    let mut client = shared_state.get_sui_client();
    let mut ledger = client.ledger_client();

    let max_retries = 10;
    for i in 0..max_retries {
        let mut req = proto::GetTransactionRequest::default();
        req.digest = Some(digest.to_string());
        req.read_mask = Some(sui_rpc::field::FieldMask {
            paths: vec!["transaction".into()],
        });

        match ledger.get_transaction(req).await {
            Ok(resp) => {
                let inner = resp.into_inner();
                if inner.transaction.is_some() {
                    debug!("Merge transaction {} found in ledger", digest);
                    return Ok(());
                }
            }
            Err(_) if i < max_retries - 1 => {
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
            Err(e) => {
                return Err(anyhow!(
                    "Merge transaction not available after {} retries: {}",
                    max_retries,
                    e
                ));
            }
        }
    }

    warn!(
        "Merge transaction {} not finalized after {} retries, continuing anyway",
        digest, max_retries
    );
    Ok(())
}

/// Initialize the gas coin pool on startup
pub async fn initialize_gas_coin_pool() -> Result<()> {
    info!("Initializing gas coin pool...");

    let shared_state = SharedSuiState::get_instance();
    let sender = shared_state.get_sui_address_required();

    // Check current state
    let config = CoinPoolConfig::default();
    let gas_info = get_gas_coins_info(&config, sender).await?;

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
