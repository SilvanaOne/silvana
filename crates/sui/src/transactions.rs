use anyhow::{Context, Result, anyhow};
use std::str::FromStr;
use sui_crypto::SuiSigner;
use sui_rpc::field::{FieldMask, FieldMaskUtil};
use sui_rpc::proto::sui::rpc::v2 as proto;
use sui_rpc::proto::sui::rpc::v2::{SimulateTransactionRequest, simulate_transaction_request};
use sui_sdk_types as sui;
use tokio::time::{Duration, sleep};
use tracing::{debug, error, info, warn};

use crate::chain::get_reference_gas_price;
use crate::coin::fetch_coin;
use crate::constants::{
    MAX_COMPUTATION_COST_MIST, MAX_GAS_BUDGET_MIST, MIN_GAS_BUDGET_MIST, SIMULATION_GAS_BUDGET_MIST,
};
use crate::error::SilvanaSuiInterfaceError;
use crate::object_lock::get_object_lock_manager;
use crate::state::SharedSuiState;

/// Helper function to check transaction effects for errors
fn check_transaction_effects(
    tx_resp: &proto::ExecuteTransactionResponse,
    operation: &str,
) -> Result<()> {
    // Get transaction digest first (available even for failed transactions)
    let tx_digest = tx_resp
        .transaction
        .as_ref()
        .and_then(|t| t.digest.as_ref())
        .map(|d| d.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Check for errors in transaction effects
    if let Some(ref transaction) = tx_resp.transaction {
        if let Some(ref effects) = transaction.effects {
            debug!("{} effects status: {:?}", operation, effects.status);
            if let Some(ref status) = effects.status {
                if let Some(ref error_msg) = status.error {
                    let error_str = format!("{:?}", error_msg);

                    // Clean up common error messages for better readability
                    let clean_error = if error_str.contains("MoveAbort") {
                        // Extract abort code and location for Move aborts
                        let mut parts = vec![];
                        if error_str.contains("abort_code: Some(") {
                            if let Some(start) = error_str.find("abort_code: Some(") {
                                let code_start = start + "abort_code: Some(".len();
                                if let Some(end) = error_str[code_start..].find(')') {
                                    let abort_code_str = &error_str[code_start..code_start + end];
                                    // Try to parse the abort code as u64 and format as hex
                                    if let Ok(abort_code) = abort_code_str.parse::<u64>() {
                                        parts.push(format!("abort_code: 0x{:016X}", abort_code));
                                    } else {
                                        parts.push(format!("abort_code: {}", abort_code_str));
                                    }
                                }
                            }
                        }
                        if error_str.contains("function_name: Some(") {
                            if let Some(start) = error_str.find("function_name: Some(\"") {
                                let name_start = start + "function_name: Some(\"".len();
                                if let Some(end) = error_str[name_start..].find("\"") {
                                    parts.push(format!(
                                        "function: {}",
                                        &error_str[name_start..name_start + end]
                                    ));
                                }
                            }
                        }
                        if !parts.is_empty() {
                            format!("MoveAbort: {}", parts.join(", "))
                        } else {
                            "Move execution aborted".to_string()
                        }
                    } else {
                        error_str
                    };

                    // Log as info for expected race conditions (multiple coordinators competing)
                    if clean_error.contains("reserve_proof")
                        || clean_error.contains("start_job")
                        || clean_error.contains("start_proving")
                    {
                        info!(
                            "{} transaction failed (normal for multiple coordinators): {} (tx: {})",
                            operation, clean_error, tx_digest
                        );
                    } else {
                        error!(
                            "{} transaction failed: {} (tx: {})",
                            operation, clean_error, tx_digest
                        );
                    }

                    return Err(SilvanaSuiInterfaceError::TransactionError {
                        message: format!("{} transaction failed: {}", operation, clean_error),
                        tx_digest: Some(tx_digest.clone()),
                    }
                    .into());
                }
            }
        }
    }

    // Check for transaction success in effects
    let tx_successful = tx_resp
        .transaction
        .as_ref()
        .and_then(|t| t.effects.as_ref())
        .and_then(|e| e.status.as_ref())
        .map(|s| s.error.is_none())
        .unwrap_or(false);

    if !tx_successful {
        error!("{} transaction failed despite being executed", operation);
        return Err(anyhow!(
            "{} transaction failed despite being executed",
            operation
        ));
    }

    Ok(())
}

/// Wait for a transaction to be available in the ledger
/// This polls GetTransaction until the transaction is found or timeout occurs
/// Optionally takes a CoinLockGuard to ensure the coin stays locked until confirmation
async fn wait_for_transaction(
    tx_digest: &str,
    max_wait_ms: Option<u64>,
    _gas_guard: Option<crate::coin::CoinLockGuard>,
) -> Result<()> {
    let timeout = max_wait_ms.unwrap_or(5000); // Default to 5000ms if not specified
    let start = std::time::Instant::now();
    let mut client = SharedSuiState::get_instance().get_sui_client();
    let mut ledger = client.ledger_client();

    debug!(
        "Waiting for transaction {} to be available in ledger (max {}ms)",
        tx_digest, timeout
    );

    loop {
        // Check if we've exceeded the maximum wait time
        if start.elapsed().as_millis() > timeout as u128 {
            return Err(anyhow!(
                "Timeout waiting for transaction {} after {}ms",
                tx_digest,
                timeout
            ));
        }

        // Try to get the transaction - just check if it exists
        let req = proto::GetTransactionRequest {
            digest: Some(tx_digest.to_string()),
            read_mask: Some(FieldMask {
                paths: vec!["digest".into()], // Just request minimal data to check existence
            }),
        };

        match ledger.get_transaction(req).await {
            Ok(_) => {
                // Transaction found! It's available in the ledger
                debug!(
                    "Transaction {} is now available in ledger (took {}ms)",
                    tx_digest,
                    start.elapsed().as_millis()
                );
                return Ok(());
            }
            Err(e) => {
                // Transaction not found yet, this is expected while we wait
                debug!("Transaction {} not yet available: {}", tx_digest, e);
            }
        }

        // Wait before polling again
        sleep(Duration::from_millis(200)).await;
    }
}

/// Get the clock object ID (0x6 for system clock)
fn get_clock_object_id() -> sui::Address {
    sui::Address::from_str("0x0000000000000000000000000000000000000000000000000000000000000006")
        .expect("Valid clock object ID")
}

/// Get the app instance object ID from the job data
pub(crate) fn get_object_id(app_instance_str: &str) -> Result<sui::Address> {
    let app_instance_id = if app_instance_str.starts_with("0x") {
        app_instance_str.to_string()
    } else {
        format!("0x{}", app_instance_str)
    };
    Ok(sui::Address::from_str(&app_instance_id)?)
}

/// Get object details including ownership information and initial_shared_version
async fn get_object_details(
    object_id: sui::Address,
) -> Result<(sui::ObjectReference, Option<u64>)> {
    let mut client = SharedSuiState::get_instance().get_sui_client();
    let mut ledger = client.ledger_client();

    let response = ledger
        .get_object(proto::GetObjectRequest {
            object_id: Some(object_id.to_string()),
            version: None,
            read_mask: Some(FieldMask::from_paths([
                "object_id",
                "version",
                "digest",
                "owner",
            ])),
        })
        .await
        .context("Failed to get object")?
        .into_inner();

    if let Some(object) = response.object {
        let id = object
            .object_id
            .context("Missing object_id")?
            .parse()
            .context("Failed to parse object_id")?;
        let version = object.version.context("Missing version")?;
        let digest = object
            .digest
            .context("Missing digest")?
            .parse()
            .context("Failed to parse digest")?;

        let obj_ref = sui::ObjectReference::new(id, version, digest);

        // Extract initial_shared_version from owner information
        let initial_shared_version = object.owner.and_then(|owner| {
            // For shared objects, the owner.version contains the initial_shared_version
            // and address should be empty/None
            if owner.address.is_none() || owner.address == Some("".to_string()) {
                // This is likely a shared object, return the version as initial_shared_version
                owner.version
            } else {
                // This is an owned object
                None
            }
        });
        Ok((obj_ref, initial_shared_version))
    } else {
        Err(anyhow!("Object not found: {}", object_id))
    }
}

/// Result of executing a transaction block
pub struct TransactionBlockResult {
    /// The transaction digest
    pub digest: String,
    /// Created object IDs (if any)
    pub created_objects: Vec<String>,
}

/// Options for publishing Move modules
pub struct PublishOptions {
    /// Base64-encoded compiled modules
    pub modules: Vec<String>,
    /// Dependencies (usually addresses)
    pub dependencies: Vec<String>,
}

/// Execute multiple transactions in a single transaction block
/// Each operation is a tuple of (object_ids, module_name, function_name, args_builder)
/// Operations can use different objects and share clock objects
/// object_ids can be empty for functions that don't need object arguments (like create_registry)
/// If publish_options is provided, the modules will be published first
pub(crate) async fn execute_transaction_block<F>(
    package_id: sui::Address,
    operations: Vec<(Vec<String>, String, String, F)>, // object_ids, module_name, function_name, args_builder
    custom_gas_budget: Option<u64>,
    publish_options: Option<PublishOptions>,
    max_computation_cost: Option<u64>,
) -> Result<String>
where
    F: Fn(
        &mut sui_transaction_builder::TransactionBuilder,
        Vec<sui_sdk_types::Argument>, // object_args (can be empty)
        sui_sdk_types::Argument,      // clock_arg
    ) -> Vec<sui_sdk_types::Argument>,
{
    const MAX_RETRIES: u32 = 3;
    let mut retry_count = 0;

    if operations.is_empty() && publish_options.is_none() {
        return Err(anyhow!("No operations or publish options provided"));
    }

    let function_names: Vec<String> = operations
        .iter()
        .map(|(_, _, name, _)| name.clone())
        .collect();
    let all_object_ids: Vec<String> = operations
        .iter()
        .flat_map(|(object_ids, _, _, _)| object_ids.clone())
        .collect();
    debug!(
        "Creating batch transaction for {} objects: {:?} with functions: {:?}",
        all_object_ids.len(),
        all_object_ids,
        function_names
    );

    // Get shared state and client
    let shared_state = SharedSuiState::get_instance();
    let sender = shared_state.get_sui_address_required();
    let sk = shared_state.get_sui_private_key_required().clone();
    let clock_object_id = get_clock_object_id();

    debug!("Package ID: {}", package_id);
    debug!("Sender: {}", sender);

    // Parse and collect all unique object IDs
    let mut unique_object_ids: Vec<String> = all_object_ids.clone();
    unique_object_ids.sort();
    unique_object_ids.dedup();

    let mut object_addresses = Vec::new();
    for object_id_str in &unique_object_ids {
        let object_id = get_object_id(object_id_str)
            .context(format!("Failed to parse object ID for '{}'", object_id_str))?;
        object_addresses.push(object_id);
        debug!("Object: {} -> ID: {}", object_id_str, object_id);
    }

    // Use SIMULATION_GAS_BUDGET_MIST (5 SUI) for dry run simulation to ensure it can complete and provide accurate estimates
    // But keep track of the actual requested budget for validation
    let mut gas_budget = SIMULATION_GAS_BUDGET_MIST;

    debug!(
        "Gas budget: using {} MIST ({} SUI) for simulation, custom budget provided: {:?} MIST",
        gas_budget,
        gas_budget as f64 / 1_000_000_000.0,
        custom_gas_budget
    );

    // Lock all object objects BEFORE fetching their versions
    // This prevents race conditions where multiple threads fetch the same versions
    let object_lock_manager = get_object_lock_manager();
    let mut object_guards = Vec::new();
    for object_id in &object_addresses {
        let guard = object_lock_manager
            .lock_object_with_retry(*object_id, 50)
            .await
            .context(format!("Failed to lock object: {}", object_id))?;
        debug!("Locked object: {}", object_id);
        object_guards.push(guard);
    }

    // Variables that will be reused across retries
    let mut gas_guard: Option<crate::coin::CoinLockGuard> = None;

    loop {
        // Get a fresh client for each attempt
        let mut client = shared_state.get_sui_client();

        // Build transaction using TransactionBuilder
        let mut tb = sui_transaction_builder::TransactionBuilder::new();
        tb.set_sender(sender);
        tb.set_gas_budget(gas_budget);

        // Get gas price
        let gas_price = get_reference_gas_price(&mut client).await?;
        tb.set_gas_price(gas_price);
        debug!("Gas price: {}", gas_price);

        // Release old coin if we're retrying
        if retry_count > 0 {
            if let Some(old_guard) = gas_guard.take() {
                info!(
                    "Retry {}/{}: Releasing old gas coin {} due to version conflict",
                    retry_count,
                    MAX_RETRIES,
                    old_guard.coin_id()
                );
                drop(old_guard);
                // Small delay to allow the coin to be released
                sleep(Duration::from_millis(100)).await;
            }
        }

        // Select new gas coin for this attempt
        let (gas_coin, new_gas_guard) = match fetch_coin(&mut client, sender, gas_budget).await? {
            Some((coin, guard)) => (coin, guard),
            None => {
                warn!(
                    "No available coins with sufficient balance for gas, attempting to request from faucet..."
                );

                // Request fixed amount from faucet
                match crate::faucet::ensure_sufficient_balance(
                    crate::constants::FAUCET_REQUEST_AMOUNT_SUI,
                )
                .await
                {
                    Ok(true) => {
                        info!(
                            "Faucet tokens requested successfully ({} SUI), retrying coin fetch...",
                            crate::constants::FAUCET_REQUEST_AMOUNT_SUI
                        );
                        // Wait a bit more for tokens to be available
                        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

                        // Try to fetch coin again after faucet
                        match fetch_coin(&mut client, sender, gas_budget).await? {
                            Some((coin, guard)) => (coin, guard),
                            None => {
                                error!("Still no available coins after faucet request");
                                return Err(anyhow!(
                                    "No available coins with sufficient balance for gas even after faucet request"
                                ));
                            }
                        }
                    }
                    Ok(false) => {
                        // Faucet wasn't needed (shouldn't happen here but handle it)
                        error!("No available coins but faucet claims sufficient balance exists");
                        return Err(anyhow!(
                            "No available coins with sufficient balance for gas"
                        ));
                    }
                    Err(e) => {
                        error!("Failed to request faucet tokens: {}", e);
                        return Err(anyhow!(
                            "No available coins with sufficient balance for gas and faucet request failed: {}",
                            e
                        ));
                    }
                }
            }
        };
        gas_guard = Some(new_gas_guard);

        let gas_input = sui_transaction_builder::unresolved::Input::owned(
            gas_coin.object_id(),
            gas_coin.object_ref.version(),
            *gas_coin.object_ref.digest(),
        );
        tb.add_gas_objects(vec![gas_input]);
        debug!(
            "Gas coin selected: id={} ver={} digest={} balance={}",
            gas_coin.object_id(),
            gas_coin.object_ref.version(),
            gas_coin.object_ref.digest(),
            gas_coin.balance
        );

        // Fetch the current version and ownership info of all object objects
        // Do this fresh for each retry to get the latest versions
        let mut object_args = std::collections::HashMap::new();

        for (i, &object_id) in object_addresses.iter().enumerate() {
            let object_id_str = &unique_object_ids[i];

            let (object_ref, initial_shared_version) =
                get_object_details(object_id).await.context(format!(
                    "Failed to get object details for {}",
                    object_id_str
                ))?;

            // Create input based on whether object is shared or owned
            let object_input = if let Some(shared_version) = initial_shared_version {
                if retry_count > 0 {
                    info!(
                        "Retry {}/{}: Using updated shared object version {} for object {}",
                        retry_count, MAX_RETRIES, shared_version, object_id_str
                    );
                }
                debug!(
                    "Using shared object input for object {} with initial_shared_version={}",
                    object_id_str, shared_version
                );
                sui_transaction_builder::unresolved::Input::shared(
                    object_id,
                    shared_version,
                    true, // mutable
                )
            } else {
                debug!("Using owned object input for object {}", object_id_str);
                sui_transaction_builder::unresolved::Input::owned(
                    *object_ref.object_id(),
                    object_ref.version(),
                    *object_ref.digest(),
                )
            };

            let object_arg = tb.input(object_input);
            object_args.insert(object_id_str.clone(), object_arg);
        }

        // Clock object (shared)
        let clock_input =
            sui_transaction_builder::unresolved::Input::shared(clock_object_id, 1, false);
        let clock_arg = tb.input(clock_input);

        // If publish options are provided, publish the modules first
        let published_package = if let Some(ref publish_opts) = publish_options {
            debug!(
                "Publishing {} modules with {} dependencies",
                publish_opts.modules.len(),
                publish_opts.dependencies.len()
            );

            // Decode base64 modules
            use base64::{Engine, engine::general_purpose::STANDARD};
            let mut decoded_modules = Vec::new();
            for module_b64 in &publish_opts.modules {
                let decoded = STANDARD
                    .decode(module_b64)
                    .map_err(|e| anyhow!("Failed to decode module: {}", e))?;
                decoded_modules.push(decoded);
            }

            // Parse dependencies as addresses
            let mut parsed_deps = Vec::new();
            for dep in &publish_opts.dependencies {
                debug!("Parsing dependency: {}", dep);
                // Use the same pattern as get_object_id - ensure 0x prefix
                let dep_with_prefix = if dep.starts_with("0x") {
                    dep.to_string()
                } else {
                    format!("0x{}", dep)
                };

                let addr = sui::Address::from_str(&dep_with_prefix)
                    .map_err(|e| anyhow!("Failed to parse dependency address {}: {}", dep, e))?;
                parsed_deps.push(addr);
                debug!("Successfully parsed dependency: {} -> {}", dep, addr);
            }

            // Call tb.publish to publish the modules
            let published = tb.publish(decoded_modules, parsed_deps);
            debug!("Published package, result argument: {:?}", published);
            Some(published)
        } else {
            None
        };

        // Add all function calls to the transaction
        for (object_ids_for_op, module_name, function_name, build_args) in &operations {
            // Collect the object arguments for this operation
            let mut op_object_args = Vec::new();
            for object_id_str in object_ids_for_op {
                let obj_arg = object_args
                    .get(object_id_str)
                    .ok_or_else(|| anyhow!("Object argument not found for: {}", object_id_str))?;
                op_object_args.push(*obj_arg);
            }

            // Build function-specific arguments
            let args = build_args(&mut tb, op_object_args, clock_arg);

            // Function call
            let func = sui_transaction_builder::Function::new(
                package_id,
                module_name
                    .parse()
                    .map_err(|e| anyhow!("Failed to parse module name '{}': {}", module_name, e))?,
                function_name.parse().map_err(|e| {
                    anyhow!("Failed to parse function name '{}': {}", function_name, e)
                })?,
                vec![],
            );
            tb.move_call(func, args);
        }

        // If we published a package, transfer it to the sender
        if let Some(published) = published_package {
            debug!("Transferring published package to sender: {}", sender);
            let sender_arg = tb.input(sui_transaction_builder::Serialized(&sender));
            tb.transfer_objects(vec![published], sender_arg);
        }

        // Always perform dry run on first attempt to estimate or validate gas budget
        if retry_count == 0 {
            // Build temporary transaction for gas estimation/validation
            let temp_tx = tb.clone().finish()?;
            let num_operations = operations.len();
            debug!(
                "Performing gas estimation dry run for {} Move call(s): {}",
                num_operations,
                function_names.join(", ")
            );
            debug!(
                "Initial budget for simulation: {} MIST ({} SUI)",
                gas_budget,
                gas_budget as f64 / 1_000_000_000.0
            );

            // Try gas estimation once - no retries
            let mut execution = client.execution_client();
            let simulate_req = SimulateTransactionRequest {
                transaction: Some(temp_tx.clone().into()),
                read_mask: Some(FieldMask {
                    paths: vec![
                        "transaction.effects.status".into(),
                        "transaction.effects.gas_used".into(),
                    ],
                }),
                checks: Some(simulate_transaction_request::TransactionChecks::Enabled as i32),
                do_gas_selection: Some(false), // We're managing gas ourselves
            };

            match execution.simulate_transaction(simulate_req).await {
                Ok(sim_resp) => {
                    let sim_result = sim_resp.into_inner();

                    // Check if simulation succeeded
                    if let Some(ref transaction) = sim_result.transaction {
                        if let Some(ref effects) = transaction.effects {
                            if let Some(ref status) = effects.status {
                                if status.error.is_none() {
                                    // Simulation succeeded, extract gas usage
                                    if let Some(ref gas_summary) = effects.gas_used {
                                        let computation_cost =
                                            gas_summary.computation_cost.unwrap_or(0);
                                        let storage_cost = gas_summary.storage_cost.unwrap_or(0);
                                        let storage_rebate =
                                            gas_summary.storage_rebate.unwrap_or(0);
                                        let non_refundable_storage_fee =
                                            gas_summary.non_refundable_storage_fee.unwrap_or(0);

                                        // Calculate total gas needed with 100% buffer
                                        // Use saturating_sub to prevent underflow when rebate is larger than costs
                                        let total_gas_used = (computation_cost
                                            + storage_cost
                                            + non_refundable_storage_fee)
                                            .saturating_sub(storage_rebate);
                                        let estimated_budget = (total_gas_used as f64 * 2.0) as u64;

                                        // Ensure minimum budget
                                        let calculated_budget =
                                            estimated_budget.max(MIN_GAS_BUDGET_MIST);

                                        // Calculate average per move call
                                        let num_calls = operations.len() as u64;
                                        let avg_per_call = if num_calls > 0 {
                                            total_gas_used / num_calls
                                        } else {
                                            0
                                        };

                                        debug!("Dry run gas summary:");
                                        debug!("  Computation cost: {} MIST", computation_cost);
                                        debug!("  Storage cost: {} MIST", storage_cost);
                                        debug!("  Storage rebate: {} MIST", storage_rebate);
                                        debug!(
                                            "  Non-refundable fee: {} MIST",
                                            non_refundable_storage_fee
                                        );
                                        debug!("  Total gas used: {} MIST", total_gas_used);
                                        debug!(
                                            "  Estimated budget (with 100% buffer): {} MIST",
                                            estimated_budget
                                        );
                                        debug!(
                                            "  Calculated budget: {} MIST ({} SUI)",
                                            calculated_budget,
                                            calculated_budget as f64 / 1_000_000_000.0
                                        );

                                        // Info log with key metrics
                                        debug!(
                                            "Gas estimation complete: {} Move calls, total gas used: {} MIST ({:.4} SUI), avg per call: {} MIST ({:.6} SUI), calculated budget: {} MIST ({:.4} SUI)",
                                            num_calls,
                                            total_gas_used,
                                            total_gas_used as f64 / 1_000_000_000.0,
                                            avg_per_call,
                                            avg_per_call as f64 / 1_000_000_000.0,
                                            calculated_budget,
                                            calculated_budget as f64 / 1_000_000_000.0
                                        );

                                        let max_comp_cost = max_computation_cost.unwrap_or(MAX_COMPUTATION_COST_MIST);
                                        if computation_cost > max_comp_cost {
                                            info!(
                                                "Calculated computation cost {} MIST ({:.4} SUI) exceeds max computation cost {} MIST ({} SUI)",
                                                computation_cost,
                                                computation_cost as f64 / 1_000_000_000.0,
                                                max_comp_cost,
                                                max_comp_cost as f64 / 1_000_000_000.0
                                            );
                                            return Err(anyhow!(
                                                "Computation cost {} MIST exceeds maximum allowed {} MIST",
                                                computation_cost,
                                                max_comp_cost
                                            ));
                                        }

                                        // Check if calculated budget exceeds maximum
                                        if calculated_budget > MAX_GAS_BUDGET_MIST {
                                            info!(
                                                "Calculated gas budget {} MIST ({:.4} SUI) exceeds MAX_GAS_BUDGET_MIST {} MIST ({} SUI)",
                                                calculated_budget,
                                                calculated_budget as f64 / 1_000_000_000.0,
                                                MAX_GAS_BUDGET_MIST,
                                                MAX_GAS_BUDGET_MIST as f64 / 1_000_000_000.0
                                            );
                                            return Err(anyhow!(
                                                "Gas requirement {} MIST exceeds maximum allowed {} MIST",
                                                calculated_budget,
                                                MAX_GAS_BUDGET_MIST
                                            ));
                                        }

                                        // Check if custom gas budget was provided
                                        if let Some(custom_budget) = custom_gas_budget {
                                            // Compare with the actual calculated budget, not a capped value
                                            if custom_budget < calculated_budget {
                                                error!(
                                                    "Provided gas budget {} MIST ({:.4} SUI) is insufficient. Required: {} MIST ({:.4} SUI) based on dry run",
                                                    custom_budget,
                                                    custom_budget as f64 / 1_000_000_000.0,
                                                    calculated_budget,
                                                    calculated_budget as f64 / 1_000_000_000.0
                                                );
                                                return Err(anyhow!(
                                                    "Insufficient gas budget: provided {} MIST, but dry run indicates {} MIST required",
                                                    custom_budget,
                                                    calculated_budget
                                                ));
                                            }
                                            // Custom budget is sufficient, use it for the actual transaction
                                            info!(
                                                "Custom gas budget {} MIST ({:.4} SUI) is sufficient. Dry run calculated: {} MIST ({:.4} SUI), computation cost: {} MIST ({:.4} SUI)",
                                                custom_budget,
                                                custom_budget as f64 / 1_000_000_000.0,
                                                calculated_budget,
                                                calculated_budget as f64 / 1_000_000_000.0,
                                                computation_cost,
                                                computation_cost as f64 / 1_000_000_000.0
                                            );
                                            // Set the actual requested budget for the transaction
                                            gas_budget = custom_budget;
                                            tb.set_gas_budget(gas_budget);
                                        } else {
                                            // No custom budget provided, use the calculated budget (already validated to be within MAX_GAS_BUDGET_MIST)
                                            if calculated_budget != gas_budget {
                                                let old_budget = gas_budget;
                                                gas_budget = calculated_budget;

                                                debug!(
                                                    "Updating gas budget from simulation: {} MIST ({:.4} SUI) -> {} MIST ({:.4} SUI)",
                                                    old_budget,
                                                    old_budget as f64 / 1_000_000_000.0,
                                                    gas_budget,
                                                    gas_budget as f64 / 1_000_000_000.0
                                                );

                                                // Set the new gas budget on the transaction builder
                                                tb.set_gas_budget(gas_budget);

                                                debug!("Gas budget updated on transaction builder");
                                            } else {
                                                debug!(
                                                    "Gas estimation result: using estimated budget {} MIST ({:.4} SUI)",
                                                    gas_budget,
                                                    gas_budget as f64 / 1_000_000_000.0
                                                );
                                            }
                                        }
                                    } else {
                                        // No gas summary available
                                        // If custom budget was provided, use it for the actual transaction
                                        if let Some(custom_budget) = custom_gas_budget {
                                            warn!(
                                                "Dry run succeeded but no gas cost summary available, using custom budget of {} MIST",
                                                custom_budget
                                            );
                                            gas_budget = custom_budget;
                                        } else {
                                            warn!(
                                                "Dry run succeeded but no gas cost summary available, using MAX_GAS_BUDGET_MIST of {} MIST ({} SUI)",
                                                MAX_GAS_BUDGET_MIST,
                                                MAX_GAS_BUDGET_MIST as f64 / 1_000_000_000.0
                                            );
                                            gas_budget = MAX_GAS_BUDGET_MIST;
                                        }
                                        // Update transaction builder with the chosen gas budget
                                        tb.set_gas_budget(gas_budget);
                                    }
                                } else {
                                    // Simulation failed
                                    if let Some(ref error) = status.error {
                                        // Check if it's an InsufficientGas error
                                        let error_str = format!("{:?}", error);
                                        let is_insufficient_gas =
                                            error_str.contains("InsufficientGas");

                                        // If custom budget was provided and dry run shows insufficient gas, fail immediately
                                        if let Some(custom_budget) = custom_gas_budget {
                                            if is_insufficient_gas {
                                                error!(
                                                    "Dry run failed with InsufficientGas even with {} MIST ({} SUI) simulation budget. Custom budget {} MIST ({:.4} SUI) is definitely insufficient",
                                                    SIMULATION_GAS_BUDGET_MIST,
                                                    SIMULATION_GAS_BUDGET_MIST as f64
                                                        / 1_000_000_000.0,
                                                    custom_budget,
                                                    custom_budget as f64 / 1_000_000_000.0
                                                );
                                                return Err(anyhow!(
                                                    "Insufficient gas budget: dry run failed with InsufficientGas for {} MIST",
                                                    custom_budget
                                                ));
                                            }
                                            warn!(
                                                "Dry run failed with error: {:?}, keeping custom budget of {} MIST",
                                                error, custom_budget
                                            );
                                            // Keep using the custom budget for non-gas errors
                                            gas_budget = custom_budget;
                                        } else {
                                            warn!(
                                                "Dry run failed with error: {:?}, using MAX_GAS_BUDGET_MIST of {} MIST ({} SUI)",
                                                error,
                                                MAX_GAS_BUDGET_MIST,
                                                MAX_GAS_BUDGET_MIST as f64 / 1_000_000_000.0
                                            );
                                            gas_budget = MAX_GAS_BUDGET_MIST;
                                        }
                                    } else {
                                        // If custom budget was provided, don't change it even if dry run fails
                                        if let Some(custom_budget) = custom_gas_budget {
                                            warn!(
                                                "Dry run failed with unknown error, keeping custom budget of {} MIST",
                                                custom_budget
                                            );
                                            // Keep using the custom budget
                                            gas_budget = custom_budget;
                                        } else {
                                            warn!(
                                                "Dry run failed with unknown error, using MAX_GAS_BUDGET_MIST of {} MIST ({} SUI)",
                                                MAX_GAS_BUDGET_MIST,
                                                MAX_GAS_BUDGET_MIST as f64 / 1_000_000_000.0
                                            );
                                            gas_budget = MAX_GAS_BUDGET_MIST;
                                        }
                                    }
                                    // Update transaction builder with the chosen gas budget
                                    tb.set_gas_budget(gas_budget);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    // Dry run request failed entirely
                    // If custom budget was provided, keep it
                    if let Some(custom_budget) = custom_gas_budget {
                        warn!(
                            "Failed to perform dry run: {}, keeping custom budget of {} MIST",
                            e, custom_budget
                        );
                        gas_budget = custom_budget;
                    } else {
                        warn!(
                            "Failed to perform dry run: {}, using MAX_GAS_BUDGET_MIST of {} MIST ({} SUI)",
                            e,
                            MAX_GAS_BUDGET_MIST,
                            MAX_GAS_BUDGET_MIST as f64 / 1_000_000_000.0
                        );
                        gas_budget = MAX_GAS_BUDGET_MIST;
                    }
                    // Update transaction builder with the chosen gas budget
                    tb.set_gas_budget(gas_budget);
                }
            }
        }

        // Finalize and sign transaction with the final gas budget
        let tx = tb.finish()?;
        let sig = sk.sign_transaction(&tx)?;

        // Log final gas budget before execution
        debug!(
            "Executing transaction with gas budget: {} MIST ({} SUI)",
            gas_budget,
            gas_budget as f64 / 1_000_000_000.0
        );

        // Execute transaction via gRPC
        let mut exec = client.execution_client();
        let req = proto::ExecuteTransactionRequest {
            transaction: Some(tx.into()),
            signatures: vec![sig.into()],
            read_mask: Some(FieldMask::from_paths([
                "transaction",
            ])),
        };

        let functions_str = function_names.join(", ");
        debug!(
            "Sending batch transaction [{}] (attempt {}/{})...",
            functions_str,
            retry_count + 1,
            MAX_RETRIES + 1
        );
        let tx_start = std::time::Instant::now();
        let exec_result = exec.execute_transaction(req).await;
        let tx_elapsed_ms = tx_start.elapsed().as_millis();

        let resp = match exec_result {
            Ok(r) => r,
            Err(e) => {
                let error_str = e.to_string();

                // Clean up error message - remove binary details
                let clean_error = if error_str.contains("grpc-status header missing") {
                    // Handle grpc-status header missing error specially
                    if error_str.contains("HTTP status code 400") {
                        format!(
                            "Transaction rejected by server (HTTP 400): Likely invalid transaction parameters or object state. Original error: {}",
                            if let Some(idx) = error_str.find(", details: [") {
                                &error_str[..idx]
                            } else {
                                &error_str
                            }
                        )
                    } else if error_str.contains("HTTP status code 503") {
                        "Service temporarily unavailable (HTTP 503) - server may be overloaded"
                            .to_string()
                    } else {
                        format!(
                            "Server communication error: {}",
                            if let Some(idx) = error_str.find(", details: [") {
                                &error_str[..idx]
                            } else {
                                &error_str
                            }
                        )
                    }
                } else if error_str.contains("Object ID")
                    && error_str.contains("is not available for consumption")
                {
                    // Extract just the relevant object version conflict info
                    if let Some(obj_start) = error_str.find("Object ID") {
                        if let Some(version_info) = error_str.find("current version:") {
                            let end_idx =
                                error_str[version_info..].find('.').unwrap_or(50) + version_info;
                            format!(
                                "Object version conflict - {}",
                                &error_str[obj_start..end_idx]
                            )
                        } else {
                            "Object version conflict - transaction inputs are outdated".to_string()
                        }
                    } else {
                        "Transaction failed due to outdated object versions".to_string()
                    }
                } else if error_str.contains("details: [") {
                    // Remove binary details array
                    if let Some(idx) = error_str.find(", details: [") {
                        error_str[..idx].to_string()
                    } else {
                        error_str
                    }
                } else {
                    error_str
                };

                // Check if this is an error that we should retry
                let should_retry = (clean_error.contains("version conflict")
                    || clean_error.contains("not available for consumption")
                    || clean_error.contains("Service temporarily unavailable")
                    || (clean_error.contains("grpc-status header missing")
                        && clean_error.contains("HTTP status code 503")))
                    && retry_count < MAX_RETRIES;

                if should_retry {
                    retry_count += 1;

                    // Different retry strategies for different errors
                    let (delay_ms, log_msg) = if clean_error
                        .contains("Service temporarily unavailable")
                        || clean_error.contains("HTTP status code 503")
                    {
                        // Longer delay for service unavailable
                        (
                            2000 * (2_u64.pow(retry_count - 1)),
                            format!(
                                "service unavailability on attempt {}/{}",
                                retry_count,
                                MAX_RETRIES + 1
                            ),
                        )
                    } else if clean_error.contains("version conflict") {
                        // Standard exponential backoff for version conflicts
                        (
                            1000 * (2_u64.pow(retry_count - 1)),
                            format!(
                                "version conflict on attempt {}/{}",
                                retry_count,
                                MAX_RETRIES + 1
                            ),
                        )
                    } else {
                        // Shorter delay for other retryable errors
                        (
                            500 * (2_u64.pow(retry_count - 1)),
                            format!(
                                "transient error on attempt {}/{}",
                                retry_count,
                                MAX_RETRIES + 1
                            ),
                        )
                    };

                    info!(
                        "Batch transaction [{}] failed with {}. Retrying after {}ms. Error: {}",
                        functions_str, log_msg, delay_ms, clean_error
                    );

                    // Add exponential backoff delay before retry
                    let delay = Duration::from_millis(delay_ms);
                    sleep(delay).await;

                    continue; // Retry the transaction
                }

                // Log appropriate error level based on error type
                if clean_error.contains("HTTP 400") {
                    // This is likely a permanent error - invalid transaction
                    // Log additional diagnostic information
                    error!(
                        "Batch transaction [{}] failed with invalid request after {} attempts: {}",
                        functions_str,
                        retry_count + 1,
                        clean_error
                    );

                    // Log transaction details for debugging
                    debug!("Failed transaction details:");
                    debug!("  Package ID: {}", package_id);
                    debug!("  Operations count: {}", operations.len());
                    for (i, (obj_ids, module, func, _)) in operations.iter().enumerate() {
                        debug!(
                            "  Operation {}: {}::{} with {} objects",
                            i,
                            module,
                            func,
                            obj_ids.len()
                        );
                        for (j, obj_id) in obj_ids.iter().enumerate() {
                            debug!("    Object {}: {}", j, obj_id);
                        }
                    }
                    debug!("  Gas budget: {} MIST", gas_budget);
                    debug!("  Sender address: {}", sender);
                } else if clean_error.contains("version conflict")
                    || clean_error.contains("not available for consumption")
                {
                    error!(
                        "Batch transaction [{}] failed after {} retries with version conflict: {}",
                        functions_str,
                        retry_count + 1,
                        clean_error
                    );
                } else {
                    error!(
                        "Batch transaction [{}] failed: {}",
                        functions_str, clean_error
                    );
                }

                return Err(anyhow!(
                    "Failed to execute batch transaction [{}]: {}",
                    functions_str,
                    clean_error
                ));
            }
        };
        let tx_resp = resp.into_inner();

        // Check transaction effects for errors
        check_transaction_effects(&tx_resp, &functions_str)?;

        let tx_digest = tx_resp
            .transaction
            .as_ref()
            .and_then(|t| t.digest.as_ref())
            .context("Failed to get transaction digest")?
            .to_string();

        if retry_count > 0 {
            info!(
                "Batch transaction [{}] succeeded after {} retries: {} (took {}ms)",
                functions_str, retry_count, tx_digest, tx_elapsed_ms
            );
        } else {
            debug!(
                "Batch transaction [{}] executed successfully: {} (took {}ms)",
                functions_str, tx_digest, tx_elapsed_ms
            );
        }

        // Wait for the transaction to be available in the ledger
        // Pass the gas_guard to keep the coin locked until transaction is confirmed
        if let Err(e) = wait_for_transaction(&tx_digest, None, gas_guard).await {
            warn!(
                "Failed to wait for batch transaction [{}] to be available: {}",
                functions_str, e
            );
            // Continue anyway, the transaction was successful
        }

        // Release all object locks after transaction is confirmed
        for (i, guard) in object_guards.into_iter().enumerate() {
            let object_id_str = &unique_object_ids[i];
            drop(guard);
            debug!("Released object lock: {}", object_id_str);
        }

        return Ok(tx_digest);
    }
}

/// Fetch and parse events from a transaction
/// Returns a vector of event strings for analysis
pub async fn fetch_transaction_events(tx_digest: &str) -> Result<Vec<String>> {
    debug!("Fetching events for transaction: {}", tx_digest);

    let shared_state = SharedSuiState::get_instance();
    let mut client = shared_state.get_sui_client();

    // Parse transaction digest
    let digest = sui_sdk_types::Digest::from_str(tx_digest)
        .map_err(|e| anyhow!("Failed to parse transaction digest: {}", e))?;

    // Fetch transaction with events
    let mut ledger = client.ledger_client();
    let req = proto::GetTransactionRequest {
        digest: Some(digest.to_string()),
        read_mask: Some(FieldMask {
            paths: vec!["events".into()],
        }),
    };

    let resp = ledger
        .get_transaction(req)
        .await
        .map_err(|e| anyhow!("Failed to fetch transaction: {}", e))?;

    let transaction = resp.into_inner();
    let mut events = Vec::new();

    // Parse events from the transaction
    if let Some(ref tx) = transaction.transaction {
        if let Some(ref tx_events) = tx.events {
            for event in &tx_events.events {
                // Extract event type
                let event_type = event
                    .event_type
                    .as_ref()
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| "Unknown".to_string());

                // Try to parse event contents as a simple string representation
                // The actual BCS data would need proper deserialization based on the event type
                let contents = format!("{}", event_type);

                events.push(contents);
                debug!("Event: {}", events.last().unwrap());
            }
        }
    }

    debug!("Found {} events in transaction {}", events.len(), tx_digest);
    Ok(events)
}

/// Fetch and parse transaction events as JSON
pub async fn fetch_transaction_events_as_json(tx_digest: &str) -> Result<serde_json::Value> {
    use serde_json::json;
    debug!("Fetching events for transaction: {}", tx_digest);

    let shared_state = SharedSuiState::get_instance();
    let mut client = shared_state.get_sui_client();

    // Parse transaction digest
    let digest = sui_sdk_types::Digest::from_str(tx_digest)
        .map_err(|e| anyhow!("Failed to parse transaction digest: {}", e))?;

    // Fetch transaction with events
    let mut ledger = client.ledger_client();
    let req = proto::GetTransactionRequest {
        digest: Some(digest.to_string()),
        read_mask: Some(FieldMask {
            paths: vec!["events".into()],
        }),
    };

    let resp = ledger
        .get_transaction(req)
        .await
        .map_err(|e| anyhow!("Failed to fetch transaction: {}", e))?;

    let transaction = resp.into_inner();
    let mut events_json = Vec::new();

    // Parse events from the transaction
    if let Some(ref tx) = transaction.transaction {
        if let Some(ref tx_events) = tx.events {
            for event in &tx_events.events {
                // Extract event type and basic info
                let event_type = event
                    .event_type
                    .as_ref()
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| "Unknown".to_string());

                // Parse the event data - it's usually in the json field
                let mut event_obj = json!({
                    "event_type": event_type,
                    "package_id": event.package_id.as_ref().map(|p| p.to_string()),
                    "module": &event.module,
                    "sender": event.sender.as_ref().map(|s| s.to_string()),
                });

                // The actual event data is in the json field as a protobuf Value
                if let Some(ref json_value) = event.json {
                    // Convert protobuf Value to JSON
                    if let Some(prost_types::value::Kind::StructValue(struct_val)) =
                        &json_value.kind
                    {
                        // Convert the protobuf struct to JSON using the helper from parse module
                        for (key, value) in &struct_val.fields {
                            event_obj[key] = crate::parse::proto_to_json(value);
                        }
                    }
                }

                events_json.push(event_obj);
            }
        }
    }

    debug!(
        "Found {} events in transaction {}",
        events_json.len(),
        tx_digest
    );
    Ok(json!(events_json))
}
