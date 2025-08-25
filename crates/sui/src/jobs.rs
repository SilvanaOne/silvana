use anyhow::{Result, Context, anyhow};
use std::env;
use std::str::FromStr;
use sui_rpc::field::FieldMask;
use sui_rpc::proto::sui::rpc::v2beta2 as proto;
use sui_rpc::Client as GrpcClient;
use sui_sdk_types as sui;
use sui_crypto::SuiSigner;
use tracing::{debug, info, warn, error};
use tokio::time::{sleep, Duration};

use crate::chain::{get_reference_gas_price, load_sender_from_env};
use crate::coin::fetch_coin;

/// Helper function to check transaction effects for errors
fn check_transaction_effects(tx_resp: &proto::ExecuteTransactionResponse, operation: &str) -> Result<()> {
    // Check for errors in transaction effects
    if let Some(ref transaction) = tx_resp.transaction {
        if let Some(ref effects) = transaction.effects {
            debug!("{} effects status: {:?}", operation, effects.status);
            if let Some(ref status) = effects.status {
                if status.error.is_some() {
                    let error_msg = status.error.as_ref().unwrap();
                    let error_str = format!("{:?}", error_msg);
                    error!("{} transaction failed with error: {}", operation, error_str);
                    return Err(anyhow!("{} transaction failed: {}", operation, error_str));
                }
            }
        }
    }
    
    // Check transaction was successful
    if tx_resp.finality.is_none() {
        error!("{} transaction did not achieve finality", operation);
        return Err(anyhow!("{} transaction did not achieve finality", operation));
    }
    
    // Check for transaction success in effects
    let tx_successful = tx_resp.transaction
        .as_ref()
        .and_then(|t| t.effects.as_ref())
        .and_then(|e| e.status.as_ref())
        .map(|s| s.error.is_none())
        .unwrap_or(false);
        
    if !tx_successful {
        error!("{} transaction failed despite being executed", operation);
        return Err(anyhow!("{} transaction failed despite being executed", operation));
    }
    
    Ok(())
}

/// Wait for a transaction to be available in the ledger
/// This polls GetTransaction until the transaction is found or timeout occurs
async fn wait_for_transaction(
    client: &mut GrpcClient,
    tx_digest: &str,
    max_wait_ms: Option<u64>,
) -> Result<()> {
    let timeout = max_wait_ms.unwrap_or(5000); // Default to 5000ms if not specified
    let start = std::time::Instant::now();
    let mut ledger = client.ledger_client();
    
    debug!("Waiting for transaction {} to be available in ledger (max {}ms)", tx_digest, timeout);
    
    loop {
        // Check if we've exceeded the maximum wait time
        if start.elapsed().as_millis() > timeout as u128 {
            return Err(anyhow!("Timeout waiting for transaction {} after {}ms", tx_digest, timeout));
        }
        
        // Try to get the transaction - just check if it exists
        let req = proto::GetTransactionRequest {
            digest: Some(tx_digest.to_string()),
            read_mask: Some(FieldMask { 
                paths: vec!["digest".into()] // Just request minimal data to check existence
            }),
        };
        
        match ledger.get_transaction(req).await {
            Ok(_) => {
                // Transaction found! It's available in the ledger
                info!("Transaction {} is now available in ledger (took {}ms)", 
                    tx_digest, start.elapsed().as_millis());
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

/// Get the app instance object ID from the job data
fn get_app_instance_id(app_instance_str: &str) -> Result<sui::Address> {
    let app_instance_id = if app_instance_str.starts_with("0x") {
        app_instance_str.to_string()
    } else {
        format!("0x{}", app_instance_str)
    };
    Ok(sui::Address::from_str(&app_instance_id)?)
}

/// Get the clock object ID (0x6 for system clock)
fn get_clock_object_id() -> sui::Address {
    sui::Address::from_str("0x0000000000000000000000000000000000000000000000000000000000000006")
        .expect("Valid clock object ID")
}

/// Get the coordination package ID from environment variables
fn get_coordination_package_id() -> Result<sui::Address> {
    let package_id = env::var("SILVANA_REGISTRY_PACKAGE")?;
    Ok(sui::Address::from_str(&package_id)?)
}

/// Get RPC URL from environment variables  
fn get_rpc_url() -> Result<String> {
    let rpc_url = env::var("SUI_RPC_URL")
        .map_err(|_| anyhow!("SUI_RPC_URL environment variable not set"))?;
    Ok(rpc_url)
}

/// Create and submit a transaction to start a job
pub async fn start_job_tx(
    client: &mut GrpcClient,
    app_instance_str: &str,
    job_sequence: u64,
) -> Result<String> {
    info!("Creating start_app_job transaction for job_sequence: {}", job_sequence);
    
    // Parse IDs
    let package_id = get_coordination_package_id()
        .context("Failed to get coordination package ID")?;
    let app_instance_id = get_app_instance_id(app_instance_str)
        .context("Failed to parse app instance ID")?;
    let clock_object_id = get_clock_object_id();
    
    debug!("Package ID: {}", package_id);
    debug!("App instance ID: {}", app_instance_id);
    
    // Parse sender and secret key
    let (sender, sk) = load_sender_from_env()?;
    debug!("Sender: {}", sender);

    // Build transaction using TransactionBuilder
    let mut tb = sui_transaction_builder::TransactionBuilder::new();
    tb.set_sender(sender);
    tb.set_gas_budget(100_000_000); // 0.1 SUI - same as working implementation

    // Get gas price and gas object using provided client
    let gas_price = get_reference_gas_price(client).await?;
    tb.set_gas_price(gas_price);
    debug!("Gas price: {}", gas_price);

    // Select gas coin using parallel-safe coin management
    let rpc_url = get_rpc_url()?;
    let (gas_coin, _gas_guard) = match fetch_coin(&rpc_url, sender, 100_000_000).await? {
        Some((coin, guard)) => (coin, guard),
        None => {
            error!("No available coins with sufficient balance for gas");
            return Err(anyhow!("No available coins with sufficient balance for gas"));
        }
    };
    
    let gas_input = sui_transaction_builder::unresolved::Input::owned(
        gas_coin.object_id(),
        gas_coin.object_ref.version(),
        *gas_coin.object_ref.digest(),
    );
    tb.add_gas_objects(vec![gas_input]);
    debug!("Gas coin selected: id={} ver={} digest={} balance={}", 
        gas_coin.object_id(), gas_coin.object_ref.version(), gas_coin.object_ref.digest(), gas_coin.balance);

    // Get current version and ownership info of app_instance object
    let (app_instance_ref, initial_shared_version) = get_object_details(client, app_instance_id).await
        .context("Failed to get app instance details")?;
    
    // Create input based on whether object is shared or owned
    let app_instance_input = if let Some(shared_version) = initial_shared_version {
        debug!("Using shared object input for app_instance with initial_shared_version={}", shared_version);
        sui_transaction_builder::unresolved::Input::shared(
            app_instance_id,
            shared_version,
            true // mutable
        )
    } else {
        debug!("Using owned object input for app_instance");
        sui_transaction_builder::unresolved::Input::owned(
            *app_instance_ref.object_id(),
            app_instance_ref.version(),
            *app_instance_ref.digest(),
        )
    };
    let app_instance_arg = tb.input(app_instance_input);

    // Job ID argument
    let job_sequence_arg = tb.input(sui_transaction_builder::Serialized(&job_sequence));

    // Clock object (shared)
    let clock_input = sui_transaction_builder::unresolved::Input::shared(clock_object_id, 1, false);
    let clock_arg = tb.input(clock_input);

    // Function call: coordination::app_instance::start_app_job
    let func = sui_transaction_builder::Function::new(
        package_id,
        "app_instance".parse()
            .map_err(|e| anyhow!("Failed to parse module name 'app_instance': {}", e))?,
        "start_app_job".parse()
            .map_err(|e| anyhow!("Failed to parse function name 'start_app_job': {}", e))?,
        vec![],
    );
    tb.move_call(func, vec![app_instance_arg, job_sequence_arg, clock_arg]);

    // Finalize and sign
    let tx = tb.finish()?;
    let sig = sk.sign_transaction(&tx)?;

    // Execute transaction via gRPC using provided client
    let mut exec = client.execution_client();
    let req = proto::ExecuteTransactionRequest {
        transaction: Some(tx.into()),
        signatures: vec![sig.into()],
        read_mask: Some(FieldMask { paths: vec!["finality".into(), "transaction".into()] }),
    };

    debug!("Sending start_app_job transaction...");
    let tx_start = std::time::Instant::now();
    let exec_result = exec.execute_transaction(req).await;
    let tx_elapsed_ms = tx_start.elapsed().as_millis();

    let resp = match exec_result {
        Ok(r) => r,
        Err(e) => {
            error!("Transaction execution network error: {}", e);
            return Err(anyhow!("Failed to execute start_job transaction: {}", e));
        }
    };
    let tx_resp = resp.into_inner();

    // Check transaction effects for errors
    check_transaction_effects(&tx_resp, "start_job")?;

    let tx_digest = tx_resp
        .transaction
        .as_ref()
        .and_then(|t| t.digest.as_ref())
        .context("Failed to get transaction digest")?
        .to_string();

    info!(
        "start_job transaction executed successfully: {} (took {}ms)",
        tx_digest, tx_elapsed_ms
    );

    // Wait for the transaction to be available in the ledger
    if let Err(e) = wait_for_transaction(client, &tx_digest, None).await {
        warn!("Failed to wait for submit_proof transaction to be available: {}", e);
        // Continue anyway, the transaction was successful
    }

    Ok(tx_digest)
}

/// Create and submit a transaction to complete a job
pub async fn complete_job_tx(
    client: &mut GrpcClient,
    app_instance_str: &str,
    job_sequence: u64,
) -> Result<String> {
    info!("Creating complete_app_job transaction for job_sequence: {}", job_sequence);
    
    // Parse IDs
    let package_id = get_coordination_package_id()
        .context("Failed to get coordination package ID")?;
    let app_instance_id = get_app_instance_id(app_instance_str)
        .context("Failed to parse app instance ID")?;
    let clock_object_id = get_clock_object_id();

    // Parse sender and secret key
    let (sender, sk) = load_sender_from_env()?;

    // Build transaction using TransactionBuilder
    let mut tb = sui_transaction_builder::TransactionBuilder::new();
    tb.set_sender(sender);
    tb.set_gas_budget(100_000_000); // 0.1 SUI

    // Get gas price and gas coin using provided client
    let gas_price = get_reference_gas_price(client).await?;
    tb.set_gas_price(gas_price);

    // Select gas coin using parallel-safe coin management
    let rpc_url = get_rpc_url()?;
    let (gas_coin, _gas_guard) = match fetch_coin(&rpc_url, sender, 100_000_000).await? {
        Some((coin, guard)) => (coin, guard),
        None => {
            error!("No available coins with sufficient balance for gas");
            return Err(anyhow!("No available coins with sufficient balance for gas"));
        }
    };
    
    let gas_input = sui_transaction_builder::unresolved::Input::owned(
        gas_coin.object_id(),
        gas_coin.object_ref.version(),
        *gas_coin.object_ref.digest(),
    );
    tb.add_gas_objects(vec![gas_input]);

    // Get current version and ownership info of app_instance object
    let (app_instance_ref, initial_shared_version) = get_object_details(client, app_instance_id).await
        .context("Failed to get app instance details")?;
    
    // Create input based on whether object is shared or owned
    let app_instance_input = if let Some(shared_version) = initial_shared_version {
        debug!("Using shared object input for app_instance (complete) with initial_shared_version={}", shared_version);
        sui_transaction_builder::unresolved::Input::shared(
            app_instance_id,
            shared_version,
            true // mutable
        )
    } else {
        debug!("Using owned object input for app_instance (complete)");
        sui_transaction_builder::unresolved::Input::owned(
            *app_instance_ref.object_id(),
            app_instance_ref.version(),
            *app_instance_ref.digest(),
        )
    };
    let app_instance_arg = tb.input(app_instance_input);

    // Job ID argument
    let job_sequence_arg = tb.input(sui_transaction_builder::Serialized(&job_sequence));

    // Clock object (shared)
    let clock_input = sui_transaction_builder::unresolved::Input::shared(clock_object_id, 1, false);
    let clock_arg = tb.input(clock_input);

    // Function call: coordination::app_instance::complete_app_job
    let func = sui_transaction_builder::Function::new(
        package_id,
        "app_instance".parse()
            .map_err(|e| anyhow!("Failed to parse module name 'app_instance': {}", e))?,
        "complete_app_job".parse()
            .map_err(|e| anyhow!("Failed to parse function name 'complete_app_job': {}", e))?,
        vec![],
    );
    tb.move_call(func, vec![app_instance_arg, job_sequence_arg, clock_arg]);

    // Finalize and sign
    let tx = tb.finish()?;
    let sig = sk.sign_transaction(&tx)?;

    // Execute transaction using provided client
    let mut exec = client.execution_client();
    let req = proto::ExecuteTransactionRequest {
        transaction: Some(tx.into()),
        signatures: vec![sig.into()],
        read_mask: Some(FieldMask { paths: vec!["finality".into(), "transaction".into()] }),
    };

    debug!("Sending complete_app_job transaction...");
    let tx_start = std::time::Instant::now();
    let exec_result = exec.execute_transaction(req).await;
    let tx_elapsed_ms = tx_start.elapsed().as_millis();

    let resp = match exec_result {
        Ok(r) => r,
        Err(e) => {
            error!("Transaction execution network error: {}", e);
            return Err(anyhow!("Failed to execute complete_job transaction: {}", e));
        }
    };
    let tx_resp = resp.into_inner();

    // Check transaction effects for errors
    check_transaction_effects(&tx_resp, "complete_job")?;

    let tx_digest = tx_resp
        .transaction
        .as_ref()
        .and_then(|t| t.digest.as_ref())
        .context("Failed to get transaction digest")?
        .to_string();

    info!(
        "complete_job transaction executed successfully: {} (took {}ms)",
        tx_digest, tx_elapsed_ms
    );
    
    // Wait for the transaction to be available in the ledger
    // This is important for sequential operations
    if let Err(e) = wait_for_transaction(client, &tx_digest, None).await {
        warn!("Failed to wait for complete_app_job transaction to be available: {}", e);
        // Continue anyway, the transaction was successful
    }

    Ok(tx_digest)
}

/// Create and submit a transaction to fail a job
pub async fn fail_job_tx(
    client: &mut GrpcClient,
    app_instance_str: &str,
    job_sequence: u64,
    error_message: &str,
) -> Result<String> {
    info!("Creating fail_app_job transaction for job_sequence: {} with error: {}", job_sequence, error_message);
    
    // Debug: Query current job state before attempting to fail it
    let app_instance_id = get_app_instance_id(app_instance_str)
        .context("Failed to parse app instance ID for debug query")?;
    match query_job_status(client, app_instance_id, job_sequence).await {
        Ok(status) => {
            info!("Current job {} status before fail attempt: {:?}", job_sequence, status);
        }
        Err(e) => {
            warn!("Failed to query job {} status before fail: {}", job_sequence, e);
        }
    }
    
    // Parse IDs
    let package_id = get_coordination_package_id()
        .context("Failed to get coordination package ID")?;
    let app_instance_id = get_app_instance_id(app_instance_str)
        .context("Failed to parse app instance ID")?;
    let clock_object_id = get_clock_object_id();

    // Parse sender and secret key
    let (sender, sk) = load_sender_from_env()?;

    // Build transaction using TransactionBuilder
    let mut tb = sui_transaction_builder::TransactionBuilder::new();
    tb.set_sender(sender);
    tb.set_gas_budget(100_000_000); // 0.1 SUI

    // Get gas price and gas coin using provided client
    let gas_price = get_reference_gas_price(client).await?;
    tb.set_gas_price(gas_price);

    // Select gas coin using parallel-safe coin management
    let rpc_url = get_rpc_url()?;
    let (gas_coin, _gas_guard) = match fetch_coin(&rpc_url, sender, 100_000_000).await? {
        Some((coin, guard)) => (coin, guard),
        None => {
            error!("No available coins with sufficient balance for gas");
            return Err(anyhow!("No available coins with sufficient balance for gas"));
        }
    };
    
    let gas_input = sui_transaction_builder::unresolved::Input::owned(
        gas_coin.object_id(),
        gas_coin.object_ref.version(),
        *gas_coin.object_ref.digest(),
    );
    tb.add_gas_objects(vec![gas_input]);

    // Get current version and ownership info of app_instance object
    let (app_instance_ref, initial_shared_version) = get_object_details(client, app_instance_id).await
        .context("Failed to get app instance details")?;
    
    // Create input based on whether object is shared or owned
    let app_instance_input = if let Some(shared_version) = initial_shared_version {
        debug!("Using shared object input for app_instance (fail) with initial_shared_version={}", shared_version);
        sui_transaction_builder::unresolved::Input::shared(
            app_instance_id,
            shared_version,
            true // mutable
        )
    } else {
        debug!("Using owned object input for app_instance (fail)");
        sui_transaction_builder::unresolved::Input::owned(
            *app_instance_ref.object_id(),
            app_instance_ref.version(),
            *app_instance_ref.digest(),
        )
    };
    let app_instance_arg = tb.input(app_instance_input);

    // Job ID argument
    let job_sequence_arg = tb.input(sui_transaction_builder::Serialized(&job_sequence));

    // Error message argument
    let error_arg = tb.input(sui_transaction_builder::Serialized(&error_message.to_string()));

    // Clock object (shared)
    let clock_input = sui_transaction_builder::unresolved::Input::shared(clock_object_id, 1, false);
    let clock_arg = tb.input(clock_input);

    // Function call: coordination::app_instance::fail_app_job
    let func = sui_transaction_builder::Function::new(
        package_id,
        "app_instance".parse()
            .map_err(|e| anyhow!("Failed to parse module name 'app_instance': {}", e))?,
        "fail_app_job".parse()
            .map_err(|e| anyhow!("Failed to parse function name 'fail_app_job': {}", e))?,
        vec![],
    );
    tb.move_call(func, vec![app_instance_arg, job_sequence_arg, error_arg, clock_arg]);

    // Finalize and sign
    let tx = tb.finish()?;
    let sig = sk.sign_transaction(&tx)?;

    // Execute transaction using provided client
    let mut exec = client.execution_client();
    let req = proto::ExecuteTransactionRequest {
        transaction: Some(tx.into()),
        signatures: vec![sig.into()],
        read_mask: Some(FieldMask { paths: vec!["finality".into(), "transaction".into()] }),
    };

    debug!("Sending fail_app_job transaction...");
    let tx_start = std::time::Instant::now();
    let exec_result = exec.execute_transaction(req).await;
    let tx_elapsed_ms = tx_start.elapsed().as_millis();

    let resp = match exec_result {
        Ok(r) => r,
        Err(e) => {
            error!("Transaction execution error: {:?}", e);
            return Err(anyhow!("Failed to execute transaction: {}", e));
        }
    };
    let tx_resp = resp.into_inner();

    // Debug: Log full transaction response details
    debug!("Transaction response finality: {:?}", tx_resp.finality);
    if let Some(ref transaction) = tx_resp.transaction {
        debug!("Transaction digest: {:?}", transaction.digest);
        debug!("Transaction signatures: {:?}", transaction.signatures);
        debug!("Transaction effects: {:?}", transaction.effects);
        
        // Check for errors in transaction effects
        if let Some(ref effects) = transaction.effects {
            debug!("Effects status: {:?}", effects.status);
            if let Some(ref status) = effects.status {
                if status.error.is_some() {
                    error!("Transaction failed with error: {:?}", status.error);
                    let error_msg = status.error.as_ref().unwrap();
                    return Err(anyhow!("Transaction failed: {:?}", error_msg));
                }
            }
        }
    }

    // Check transaction was successful
    if tx_resp.finality.is_none() {
        error!("Transaction did not achieve finality");
        return Err(anyhow!("Transaction did not achieve finality"));
    }

    // Check for transaction success in effects
    let tx_successful = tx_resp.transaction
        .as_ref()
        .and_then(|t| t.effects.as_ref())
        .and_then(|e| e.status.as_ref())
        .map(|s| s.error.is_none())
        .unwrap_or(false);

    let tx_digest = tx_resp
        .transaction
        .as_ref()
        .and_then(|t| t.digest.as_ref())
        .context("Failed to get transaction digest")?
        .to_string();

    if tx_successful {
        info!(
            "fail_app_job transaction executed successfully: {} (took {}ms)",
            tx_digest, tx_elapsed_ms
        );
    } else {
        error!(
            "fail_app_job transaction failed: {} (took {}ms)",
            tx_digest, tx_elapsed_ms
        );
        return Err(anyhow!("Transaction failed despite being executed"));
    }

    // Wait for the transaction to be available in the ledger
    if let Err(e) = wait_for_transaction(client, &tx_digest, None).await {
        warn!("Failed to wait for fail_app_job transaction to be available: {}", e);
        // Continue anyway, the transaction was successful
    }

    Ok(tx_digest)
}

/// Create and submit a transaction to submit a proof
pub async fn submit_proof_tx(
    client: &mut GrpcClient,
    app_instance_str: &str,
    block_number: u64,
    sequences: Vec<u64>,
    merged_sequences_1: Option<Vec<u64>>,
    merged_sequences_2: Option<Vec<u64>>,
    job_id: String,
    da_hash: String,
    cpu_cores: u8,
    prover_architecture: String,
    prover_memory: u64,
    cpu_time: u64,
) -> Result<String> {
    info!("Creating submit_proof transaction for block_number: {}, job_id: {}", block_number, job_id);
    
    // Parse IDs
    let package_id = get_coordination_package_id()
        .context("Failed to get coordination package ID")?;
    let app_instance_id = get_app_instance_id(app_instance_str)
        .context("Failed to parse app instance ID")?;
    let clock_object_id = get_clock_object_id();

    // Parse sender and secret key
    let (sender, sk) = load_sender_from_env()?;

    // Build transaction using TransactionBuilder
    let mut tb = sui_transaction_builder::TransactionBuilder::new();
    tb.set_sender(sender);
    tb.set_gas_budget(100_000_000); // 0.1 SUI

    // Get gas price and gas coin using provided client
    let gas_price = get_reference_gas_price(client).await?;
    tb.set_gas_price(gas_price);

    // Select gas coin using parallel-safe coin management
    let rpc_url = get_rpc_url()?;
    let (gas_coin, _gas_guard) = match fetch_coin(&rpc_url, sender, 100_000_000).await? {
        Some((coin, guard)) => (coin, guard),
        None => {
            error!("No available coins with sufficient balance for gas");
            return Err(anyhow!("No available coins with sufficient balance for gas"));
        }
    };
    
    let gas_input = sui_transaction_builder::unresolved::Input::owned(
        gas_coin.object_id(),
        gas_coin.object_ref.version(),
        *gas_coin.object_ref.digest(),
    );
    tb.add_gas_objects(vec![gas_input]);

    // Get current version and ownership info of app_instance object
    let (app_instance_ref, initial_shared_version) = get_object_details(client, app_instance_id).await
        .context("Failed to get app instance details")?;
    
    // Create input based on whether object is shared or owned
    let app_instance_input = if let Some(shared_version) = initial_shared_version {
        debug!("Using shared object input for app_instance (submit_proof) with initial_shared_version={}", shared_version);
        sui_transaction_builder::unresolved::Input::shared(
            app_instance_id,
            shared_version,
            true // mutable
        )
    } else {
        debug!("Using owned object input for app_instance (submit_proof)");
        sui_transaction_builder::unresolved::Input::owned(
            *app_instance_ref.object_id(),
            app_instance_ref.version(),
            *app_instance_ref.digest(),
        )
    };
    let app_instance_arg = tb.input(app_instance_input);

    // Arguments
    let block_number_arg = tb.input(sui_transaction_builder::Serialized(&block_number));
    let sequences_arg = tb.input(sui_transaction_builder::Serialized(&sequences));
    let merged_sequences_1_arg = tb.input(sui_transaction_builder::Serialized(&merged_sequences_1));
    let merged_sequences_2_arg = tb.input(sui_transaction_builder::Serialized(&merged_sequences_2));
    let job_id_arg = tb.input(sui_transaction_builder::Serialized(&job_id));
    let da_hash_arg = tb.input(sui_transaction_builder::Serialized(&da_hash));
    let cpu_cores_arg = tb.input(sui_transaction_builder::Serialized(&cpu_cores));
    let prover_architecture_arg = tb.input(sui_transaction_builder::Serialized(&prover_architecture));
    let prover_memory_arg = tb.input(sui_transaction_builder::Serialized(&prover_memory));
    let cpu_time_arg = tb.input(sui_transaction_builder::Serialized(&cpu_time));

    // Clock object (shared)
    let clock_input = sui_transaction_builder::unresolved::Input::shared(clock_object_id, 1, false);
    let clock_arg = tb.input(clock_input);

    // Function call: coordination::app_instance::submit_proof
    let func = sui_transaction_builder::Function::new(
        package_id,
        "app_instance".parse()
            .map_err(|e| anyhow!("Failed to parse module name 'app_instance': {}", e))?,
        "submit_proof".parse()
            .map_err(|e| anyhow!("Failed to parse function name 'submit_proof': {}", e))?,
        vec![],
    );
    tb.move_call(func, vec![
        app_instance_arg,
        block_number_arg,
        sequences_arg,
        merged_sequences_1_arg,
        merged_sequences_2_arg,
        job_id_arg,
        da_hash_arg,
        cpu_cores_arg,
        prover_architecture_arg,
        prover_memory_arg,
        cpu_time_arg,
        clock_arg,
    ]);

    // Finalize and sign
    let tx = tb.finish()?;
    let sig = sk.sign_transaction(&tx)?;

    // Execute transaction using provided client
    let mut exec = client.execution_client();
    let req = proto::ExecuteTransactionRequest {
        transaction: Some(tx.into()),
        signatures: vec![sig.into()],
        read_mask: Some(FieldMask { paths: vec!["finality".into(), "transaction".into()] }),
    };

    debug!("Sending submit_proof transaction...");
    let tx_start = std::time::Instant::now();
    let exec_result = exec.execute_transaction(req).await;
    let tx_elapsed_ms = tx_start.elapsed().as_millis();

    let resp = match exec_result {
        Ok(r) => r,
        Err(e) => {
            error!("Transaction execution network error: {}", e);
            return Err(anyhow!("Failed to execute submit_proof transaction: {}", e));
        }
    };
    let tx_resp = resp.into_inner();

    // Check transaction effects for errors
    check_transaction_effects(&tx_resp, "submit_proof")?;

    let tx_digest = tx_resp
        .transaction
        .as_ref()
        .and_then(|t| t.digest.as_ref())
        .context("Failed to get transaction digest")?
        .to_string();

    info!(
        "submit_proof transaction executed successfully: {} (took {}ms)",
        tx_digest, tx_elapsed_ms
    );

    // Wait for the transaction to be available in the ledger
    if let Err(e) = wait_for_transaction(client, &tx_digest, None).await {
        warn!("Failed to wait for update_state_for_sequence transaction to be available: {}", e);
        // Continue anyway, the transaction was successful
    }

    Ok(tx_digest)
}

/// Create and submit a transaction to update state for a sequence
pub async fn update_state_for_sequence_tx(
    client: &mut GrpcClient,
    app_instance_str: &str,
    sequence: u64,
    new_state_data: Option<Vec<u8>>,
    new_data_availability_hash: Option<String>,
) -> Result<String> {
    info!("Creating update_state_for_sequence transaction for sequence: {}", sequence);
    
    // Parse IDs
    let package_id = get_coordination_package_id()
        .context("Failed to get coordination package ID")?;
    let app_instance_id = get_app_instance_id(app_instance_str)
        .context("Failed to parse app instance ID")?;
    let clock_object_id = get_clock_object_id();

    // Parse sender and secret key
    let (sender, sk) = load_sender_from_env()?;

    // Build transaction using TransactionBuilder
    let mut tb = sui_transaction_builder::TransactionBuilder::new();
    tb.set_sender(sender);
    tb.set_gas_budget(100_000_000); // 0.1 SUI

    // Get gas price and gas coin using provided client
    let gas_price = get_reference_gas_price(client).await?;
    tb.set_gas_price(gas_price);

    // Select gas coin using parallel-safe coin management
    let rpc_url = get_rpc_url()?;
    let (gas_coin, _gas_guard) = match fetch_coin(&rpc_url, sender, 100_000_000).await? {
        Some((coin, guard)) => (coin, guard),
        None => {
            error!("No available coins with sufficient balance for gas");
            return Err(anyhow!("No available coins with sufficient balance for gas"));
        }
    };
    
    let gas_input = sui_transaction_builder::unresolved::Input::owned(
        gas_coin.object_id(),
        gas_coin.object_ref.version(),
        *gas_coin.object_ref.digest(),
    );
    tb.add_gas_objects(vec![gas_input]);

    // Get current version and ownership info of app_instance object
    let (app_instance_ref, initial_shared_version) = get_object_details(client, app_instance_id).await
        .context("Failed to get app instance details")?;
    
    // Create input based on whether object is shared or owned
    let app_instance_input = if let Some(shared_version) = initial_shared_version {
        debug!("Using shared object input for app_instance (update_state_for_sequence) with initial_shared_version={}", shared_version);
        sui_transaction_builder::unresolved::Input::shared(
            app_instance_id,
            shared_version,
            true // mutable
        )
    } else {
        debug!("Using owned object input for app_instance (update_state_for_sequence)");
        sui_transaction_builder::unresolved::Input::owned(
            *app_instance_ref.object_id(),
            app_instance_ref.version(),
            *app_instance_ref.digest(),
        )
    };
    let app_instance_arg = tb.input(app_instance_input);

    // Arguments
    let sequence_arg = tb.input(sui_transaction_builder::Serialized(&sequence));
    let new_state_data_arg = tb.input(sui_transaction_builder::Serialized(&new_state_data));
    let new_data_availability_hash_arg = tb.input(sui_transaction_builder::Serialized(&new_data_availability_hash));

    // Clock object (shared)
    let clock_input = sui_transaction_builder::unresolved::Input::shared(clock_object_id, 1, false);
    let clock_arg = tb.input(clock_input);

    // Function call: coordination::app_instance::update_state_for_sequence
    let func = sui_transaction_builder::Function::new(
        package_id,
        "app_instance".parse()
            .map_err(|e| anyhow!("Failed to parse module name 'app_instance': {}", e))?,
        "update_state_for_sequence".parse()
            .map_err(|e| anyhow!("Failed to parse function name 'update_state_for_sequence': {}", e))?,
        vec![],
    );
    tb.move_call(func, vec![
        app_instance_arg,
        sequence_arg,
        new_state_data_arg,
        new_data_availability_hash_arg,
        clock_arg,
    ]);

    // Finalize and sign
    let tx = tb.finish()?;
    let sig = sk.sign_transaction(&tx)?;

    // Execute transaction using provided client
    let mut exec = client.execution_client();
    let req = proto::ExecuteTransactionRequest {
        transaction: Some(tx.into()),
        signatures: vec![sig.into()],
        read_mask: Some(FieldMask { paths: vec!["finality".into(), "transaction".into()] }),
    };

    debug!("Sending update_state_for_sequence transaction...");
    let tx_start = std::time::Instant::now();
    let exec_result = exec.execute_transaction(req).await;
    let tx_elapsed_ms = tx_start.elapsed().as_millis();

    let resp = match exec_result {
        Ok(r) => r,
        Err(e) => {
            error!("Transaction execution network error: {}", e);
            return Err(anyhow!("Failed to execute update_state_for_sequence transaction: {}", e));
        }
    };
    let tx_resp = resp.into_inner();

    // Check transaction effects for errors
    check_transaction_effects(&tx_resp, "update_state_for_sequence")?;

    let tx_digest = tx_resp
        .transaction
        .as_ref()
        .and_then(|t| t.digest.as_ref())
        .context("Failed to get transaction digest")?
        .to_string();

    info!(
        "update_state_for_sequence transaction executed successfully: {} (took {}ms)",
        tx_digest, tx_elapsed_ms
    );

    Ok(tx_digest)
}

/// Create and submit a transaction to create an app job
/// This is a general function that can create any type of job by specifying method_name and data
pub async fn create_app_job_tx(
    client: &mut GrpcClient,
    app_instance_str: &str,
    method_name: String,
    job_description: Option<String>,
    sequences: Option<Vec<u64>>,
    data: Vec<u8>,
) -> Result<String> {
    info!("Creating app job transaction for method: {}, data size: {} bytes", 
        method_name, data.len());
    
    // Parse IDs
    let package_id = get_coordination_package_id()
        .context("Failed to get coordination package ID")?;
    let app_instance_id = get_app_instance_id(app_instance_str)
        .context("Failed to parse app instance ID")?;
    let clock_object_id = get_clock_object_id();

    // Parse sender and secret key
    let (sender, sk) = load_sender_from_env()?;

    // Build transaction using TransactionBuilder
    let mut tb = sui_transaction_builder::TransactionBuilder::new();
    tb.set_sender(sender);
    tb.set_gas_budget(100_000_000); // 0.1 SUI

    // Get gas price and gas coin using provided client
    let gas_price = get_reference_gas_price(client).await?;
    tb.set_gas_price(gas_price);

    // Select gas coin using parallel-safe coin management
    let rpc_url = get_rpc_url()?;
    let (gas_coin, _gas_guard) = match fetch_coin(&rpc_url, sender, 100_000_000).await? {
        Some((coin, guard)) => (coin, guard),
        None => {
            error!("No available coins with sufficient balance for gas");
            return Err(anyhow!("No available coins with sufficient balance for gas"));
        }
    };
    
    let gas_input = sui_transaction_builder::unresolved::Input::owned(
        gas_coin.object_id(),
        gas_coin.object_ref.version(),
        *gas_coin.object_ref.digest(),
    );
    tb.add_gas_objects(vec![gas_input]);

    // Get current version and ownership info of app_instance object
    let (app_instance_ref, initial_shared_version) = get_object_details(client, app_instance_id).await
        .context("Failed to get app instance details")?;
    
    // Create input based on whether object is shared or owned
    let app_instance_input = if let Some(shared_version) = initial_shared_version {
        debug!("Using shared object input for app_instance (create_app_job) with initial_shared_version={}", shared_version);
        sui_transaction_builder::unresolved::Input::shared(
            app_instance_id,
            shared_version,
            true // mutable
        )
    } else {
        debug!("Using owned object input for app_instance (create_app_job)");
        sui_transaction_builder::unresolved::Input::owned(
            *app_instance_ref.object_id(),
            app_instance_ref.version(),
            *app_instance_ref.digest(),
        )
    };
    let app_instance_arg = tb.input(app_instance_input);

    // Arguments for create_app_job (following the Move function signature exactly)
    let method_name_arg = tb.input(sui_transaction_builder::Serialized(&method_name));
    let job_description_arg = tb.input(sui_transaction_builder::Serialized(&job_description));
    let sequences_arg = tb.input(sui_transaction_builder::Serialized(&sequences));
    let data_arg = tb.input(sui_transaction_builder::Serialized(&data));

    // Clock object (shared)
    let clock_input = sui_transaction_builder::unresolved::Input::shared(clock_object_id, 1, false);
    let clock_arg = tb.input(clock_input);

    // Function call: coordination::app_instance::create_app_job
    let func = sui_transaction_builder::Function::new(
        package_id,
        "app_instance".parse()
            .map_err(|e| anyhow!("Failed to parse module name 'app_instance': {}", e))?,
        "create_app_job".parse()
            .map_err(|e| anyhow!("Failed to parse function name 'create_app_job': {}", e))?,
        vec![],
    );
    tb.move_call(func, vec![
        app_instance_arg,
        method_name_arg,
        job_description_arg,
        sequences_arg,
        data_arg,
        clock_arg,
    ]);

    // Finalize and sign
    let tx = tb.finish()?;
    let sig = sk.sign_transaction(&tx)?;

    // Execute transaction using provided client
    let mut exec = client.execution_client();
    let req = proto::ExecuteTransactionRequest {
        transaction: Some(tx.into()),
        signatures: vec![sig.into()],
        read_mask: Some(FieldMask { paths: vec!["finality".into(), "transaction".into()] }),
    };

    debug!("Sending create_app_job transaction...");
    let tx_start = std::time::Instant::now();
    let exec_result = exec.execute_transaction(req).await;
    let tx_elapsed_ms = tx_start.elapsed().as_millis();

    let resp = match exec_result {
        Ok(r) => r,
        Err(e) => {
            error!("Transaction execution network error: {}", e);
            return Err(anyhow!("Failed to execute create_app_job transaction: {}", e));
        }
    };
    let tx_resp = resp.into_inner();

    // Check transaction effects for errors
    check_transaction_effects(&tx_resp, &format!("create_app_job[{}]", method_name))?;

    let tx_digest = tx_resp
        .transaction
        .as_ref()
        .and_then(|t| t.digest.as_ref())
        .context("Failed to get transaction digest")?
        .to_string();

    info!(
        "create_app_job transaction executed successfully for method '{}': {} (took {}ms)",
        method_name, tx_digest, tx_elapsed_ms
    );

    // Wait for the transaction to be available in the ledger
    if let Err(e) = wait_for_transaction(client, &tx_digest, None).await {
        warn!("Failed to wait for create_app_job transaction to be available: {}", e);
        // Continue anyway, the transaction was successful
    }

    Ok(tx_digest)
}

/// Create and submit a transaction to reject a proof
pub async fn reject_proof_tx(
    client: &mut GrpcClient,
    app_instance_str: &str,
    block_number: u64,
    sequences: Vec<u64>,
) -> Result<String> {
    info!("Creating reject_proof transaction for block_number: {}, sequences: {:?}", block_number, sequences);
    
    // Parse IDs
    let package_id = get_coordination_package_id()
        .context("Failed to get coordination package ID")?;
    let app_instance_id = get_app_instance_id(app_instance_str)
        .context("Failed to parse app instance ID")?;
    let clock_object_id = get_clock_object_id();

    // Parse sender and secret key
    let (sender, sk) = load_sender_from_env()?;

    // Build transaction using TransactionBuilder
    let mut tb = sui_transaction_builder::TransactionBuilder::new();
    tb.set_sender(sender);
    tb.set_gas_budget(100_000_000); // 0.1 SUI

    // Get gas price and gas coin using provided client
    let gas_price = get_reference_gas_price(client).await?;
    tb.set_gas_price(gas_price);

    // Select gas coin using parallel-safe coin management
    let rpc_url = get_rpc_url()?;
    let (gas_coin, _gas_guard) = match fetch_coin(&rpc_url, sender, 100_000_000).await? {
        Some((coin, guard)) => (coin, guard),
        None => {
            error!("No available coins with sufficient balance for gas");
            return Err(anyhow!("No available coins with sufficient balance for gas"));
        }
    };
    
    let gas_input = sui_transaction_builder::unresolved::Input::owned(
        gas_coin.object_id(),
        gas_coin.object_ref.version(),
        *gas_coin.object_ref.digest(),
    );
    tb.add_gas_objects(vec![gas_input]);

    // Get current version and ownership info of app_instance object
    let (app_instance_ref, initial_shared_version) = get_object_details(client, app_instance_id).await
        .context("Failed to get app instance details")?;
    
    // Create input based on whether object is shared or owned
    let app_instance_input = if let Some(shared_version) = initial_shared_version {
        debug!("Using shared object input for app_instance (reject_proof) with initial_shared_version={}", shared_version);
        sui_transaction_builder::unresolved::Input::shared(
            app_instance_id,
            shared_version,
            true // mutable
        )
    } else {
        debug!("Using owned object input for app_instance (reject_proof)");
        sui_transaction_builder::unresolved::Input::owned(
            *app_instance_ref.object_id(),
            app_instance_ref.version(),
            *app_instance_ref.digest(),
        )
    };
    let app_instance_arg = tb.input(app_instance_input);

    // Arguments
    let block_number_arg = tb.input(sui_transaction_builder::Serialized(&block_number));
    let sequences_arg = tb.input(sui_transaction_builder::Serialized(&sequences));

    // Clock object (shared)
    let clock_input = sui_transaction_builder::unresolved::Input::shared(clock_object_id, 1, false);
    let clock_arg = tb.input(clock_input);

    // Function call: coordination::app_instance::reject_proof
    let func = sui_transaction_builder::Function::new(
        package_id,
        "app_instance".parse()
            .map_err(|e| anyhow!("Failed to parse module name 'app_instance': {}", e))?,
        "reject_proof".parse()
            .map_err(|e| anyhow!("Failed to parse function name 'reject_proof': {}", e))?,
        vec![],
    );
    tb.move_call(func, vec![app_instance_arg, block_number_arg, sequences_arg, clock_arg]);

    // Finalize and sign
    let tx = tb.finish()?;
    let sig = sk.sign_transaction(&tx)?;

    // Execute transaction using provided client
    let mut exec = client.execution_client();
    let req = proto::ExecuteTransactionRequest {
        transaction: Some(tx.into()),
        signatures: vec![sig.into()],
        read_mask: Some(FieldMask { paths: vec!["finality".into(), "transaction".into()] }),
    };

    debug!("Sending reject_proof transaction...");
    let tx_start = std::time::Instant::now();
    let exec_result = exec.execute_transaction(req).await;
    let tx_elapsed_ms = tx_start.elapsed().as_millis();

    let resp = match exec_result {
        Ok(r) => r,
        Err(e) => {
            error!("Transaction execution error: {:?}", e);
            return Err(anyhow!("Failed to execute transaction: {}", e));
        }
    };
    let tx_resp = resp.into_inner();

    // Check for errors in transaction effects
    if let Some(ref transaction) = tx_resp.transaction {
        if let Some(ref effects) = transaction.effects {
            if let Some(ref status) = effects.status {
                if status.error.is_some() {
                    error!("Transaction failed with error: {:?}", status.error);
                    let error_msg = status.error.as_ref().unwrap();
                    return Err(anyhow!("Transaction failed: {:?}", error_msg));
                }
            }
        }
    }

    // Check transaction effects for errors
    check_transaction_effects(&tx_resp, "reject_proof")?;

    let tx_digest = tx_resp
        .transaction
        .as_ref()
        .and_then(|t| t.digest.as_ref())
        .context("Failed to get transaction digest")?
        .to_string();

    info!(
        "reject_proof transaction executed successfully: {} (took {}ms)",
        tx_digest, tx_elapsed_ms
    );
    
    // Wait for the transaction to be available in the ledger
    // This is important because start_proving might be called right after
    if let Err(e) = wait_for_transaction(client, &tx_digest, None).await {
        warn!("Failed to wait for reject_proof transaction to be available: {}", e);
        // Continue anyway, the transaction was successful
    }

    Ok(tx_digest)
}

/// Create and submit a transaction to start proving (reserve proofs)
pub async fn start_proving_tx(
    client: &mut GrpcClient,
    app_instance_str: &str,
    block_number: u64,
    sequences: Vec<u64>,
    merged_sequences_1: Option<Vec<u64>>,
    merged_sequences_2: Option<Vec<u64>>,
    job_id: String,
) -> Result<String> {
    info!("Creating start_proving transaction for block_number: {}, sequences: {:?}", block_number, sequences);
    
    // Parse IDs
    let package_id = get_coordination_package_id()
        .context("Failed to get coordination package ID")?;
    let app_instance_id = get_app_instance_id(app_instance_str)
        .context("Failed to parse app instance ID")?;
    let clock_object_id = get_clock_object_id();

    // Parse sender and secret key
    let (sender, sk) = load_sender_from_env()?;

    // Build transaction using TransactionBuilder
    let mut tb = sui_transaction_builder::TransactionBuilder::new();
    tb.set_sender(sender);
    tb.set_gas_budget(100_000_000); // 0.1 SUI

    // Get gas price and gas coin using provided client
    let gas_price = get_reference_gas_price(client).await?;
    tb.set_gas_price(gas_price);

    // Select gas coin using parallel-safe coin management
    let rpc_url = get_rpc_url()?;
    let (gas_coin, _gas_guard) = match fetch_coin(&rpc_url, sender, 100_000_000).await? {
        Some((coin, guard)) => (coin, guard),
        None => {
            error!("No available coins with sufficient balance for gas");
            return Err(anyhow!("No available coins with sufficient balance for gas"));
        }
    };
    
    let gas_input = sui_transaction_builder::unresolved::Input::owned(
        gas_coin.object_id(),
        gas_coin.object_ref.version(),
        *gas_coin.object_ref.digest(),
    );
    tb.add_gas_objects(vec![gas_input]);

    // Get current version and ownership info of app_instance object
    let (app_instance_ref, initial_shared_version) = get_object_details(client, app_instance_id).await
        .context("Failed to get app instance details")?;
    
    // Create input based on whether object is shared or owned
    let app_instance_input = if let Some(shared_version) = initial_shared_version {
        debug!("Using shared object input for app_instance (start_proving) with initial_shared_version={}", shared_version);
        sui_transaction_builder::unresolved::Input::shared(
            app_instance_id,
            shared_version,
            true // mutable
        )
    } else {
        debug!("Using owned object input for app_instance (start_proving)");
        sui_transaction_builder::unresolved::Input::owned(
            *app_instance_ref.object_id(),
            app_instance_ref.version(),
            *app_instance_ref.digest(),
        )
    };
    let app_instance_arg = tb.input(app_instance_input);

    // Arguments
    let block_number_arg = tb.input(sui_transaction_builder::Serialized(&block_number));
    let sequences_arg = tb.input(sui_transaction_builder::Serialized(&sequences));
    let merged_sequences_1_arg = tb.input(sui_transaction_builder::Serialized(&merged_sequences_1));
    let merged_sequences_2_arg = tb.input(sui_transaction_builder::Serialized(&merged_sequences_2));
    let job_id_arg = tb.input(sui_transaction_builder::Serialized(&job_id));

    // Clock object (shared)
    let clock_input = sui_transaction_builder::unresolved::Input::shared(clock_object_id, 1, false);
    let clock_arg = tb.input(clock_input);

    // Function call: coordination::app_instance::start_proving
    let func = sui_transaction_builder::Function::new(
        package_id,
        "app_instance".parse()
            .map_err(|e| anyhow!("Failed to parse module name 'app_instance': {}", e))?,
        "start_proving".parse()
            .map_err(|e| anyhow!("Failed to parse function name 'start_proving': {}", e))?,
        vec![],
    );
    tb.move_call(func, vec![
        app_instance_arg, 
        block_number_arg, 
        sequences_arg, 
        merged_sequences_1_arg,
        merged_sequences_2_arg,
        job_id_arg,
        clock_arg
    ]);

    // Finalize and sign
    let tx = tb.finish()?;
    let sig = sk.sign_transaction(&tx)?;

    // Execute transaction using provided client
    let mut exec = client.execution_client();
    let req = proto::ExecuteTransactionRequest {
        transaction: Some(tx.into()),
        signatures: vec![sig.into()],
        read_mask: Some(FieldMask { paths: vec!["finality".into(), "transaction".into()] }),
    };

    debug!("Sending start_proving transaction...");
    let tx_start = std::time::Instant::now();
    let exec_result = exec.execute_transaction(req).await;
    let tx_elapsed_ms = tx_start.elapsed().as_millis();

    let resp = match exec_result {
        Ok(r) => r,
        Err(e) => {
            error!("Transaction execution error: {:?}", e);
            return Err(anyhow!("Failed to execute transaction: {}", e));
        }
    };
    let tx_resp = resp.into_inner();

    // Check for errors in transaction effects
    if let Some(ref transaction) = tx_resp.transaction {
        if let Some(ref effects) = transaction.effects {
            if let Some(ref status) = effects.status {
                if status.error.is_some() {
                    // This is expected if another coordinator already reserved the proofs
                    warn!("start_proving transaction failed (may be already reserved): {:?}", status.error);
                    let error_msg = status.error.as_ref().unwrap();
                    return Err(anyhow!("Transaction failed: {:?}", error_msg));
                }
            }
        }
    }

    // Check transaction was successful
    if tx_resp.finality.is_none() {
        error!("Transaction did not achieve finality");
        return Err(anyhow!("Transaction did not achieve finality"));
    }

    // Check for transaction success in effects
    let tx_successful = tx_resp.transaction
        .as_ref()
        .and_then(|t| t.effects.as_ref())
        .and_then(|e| e.status.as_ref())
        .map(|s| s.error.is_none())
        .unwrap_or(false);

    let tx_digest = tx_resp
        .transaction
        .as_ref()
        .and_then(|t| t.digest.as_ref())
        .context("Failed to get transaction digest")?
        .to_string();

    if tx_successful {
        info!(
            "start_proving transaction executed successfully: {} (took {}ms)",
            tx_digest, tx_elapsed_ms
        );
        
        // Wait for the transaction to be available in the ledger
        if let Err(e) = wait_for_transaction(client, &tx_digest, None).await {
            warn!("Failed to wait for start_proving transaction to be available: {}", e);
            // Continue anyway, the transaction was successful
        }
    } else {
        warn!(
            "start_proving transaction failed (proofs may be already reserved): {} (took {}ms)",
            tx_digest, tx_elapsed_ms
        );
        return Err(anyhow!("Failed to reserve proofs - may be already reserved by another coordinator"));
    }

    Ok(tx_digest)
}

/// Convenience function to create a merge job
pub async fn create_merge_job_tx(
    client: &mut GrpcClient,
    app_instance_str: &str,
    block_number: u64,
    sequences1: Vec<u64>,
    sequences2: Vec<u64>,
    job_description: Option<String>,
) -> Result<String> {
    // Create ProofMergeData and serialize it with BCS
    use serde::{Serialize, Deserialize};
    
    #[derive(Serialize, Deserialize)]
    struct ProofMergeData {
        block_number: u64,
        sequences1: Vec<u64>,
        sequences2: Vec<u64>,
    }
    
    let merge_data = ProofMergeData {
        block_number,
        sequences1: sequences1.clone(),
        sequences2: sequences2.clone(),
    };
    
    let serialized_data = bcs::to_bytes(&merge_data)
        .context("Failed to serialize ProofMergeData")?;
    
    debug!("Serialized ProofMergeData size: {} bytes", serialized_data.len());
    
    // Combine and sort sequences from both proofs
    let mut combined_sequences = sequences1.clone();
    combined_sequences.extend(sequences2.clone());
    combined_sequences.sort();
    combined_sequences.dedup(); // Remove any duplicates
    
    debug!("Combined sequences for merge job: {:?}", combined_sequences);

    // Call the general create_app_job_tx function
    create_app_job_tx(
        client,
        app_instance_str,
        "merge".to_string(),
        job_description,
        Some(combined_sequences), // Pass the combined sequences
        serialized_data,
    ).await
}

/// Debug function to query job status from the blockchain
async fn query_job_status(
    client: &mut GrpcClient,
    app_instance_id: sui::Address,
    job_sequence: u64,
) -> Result<String> {
    let mut ledger = client.ledger_client();
    
    // Query the app_instance object to get the jobs field
    let response = ledger
        .get_object(proto::GetObjectRequest {
            object_id: Some(app_instance_id.to_string()),
            version: None,
            read_mask: Some(prost_types::FieldMask {
                paths: vec!["data".to_string()],
            }),
        })
        .await
        .context("Failed to get app_instance object")?
        .into_inner();

    if let Some(object) = response.object {
        // For now, just log that we found the object
        debug!("Found app_instance object with version: {:?}", object.version);
        return Ok(format!("Found job {} in app_instance (object version: {:?})", job_sequence, object.version));
    } else {
        return Err(anyhow!("App instance object not found"));
    }
}

/// Get object details including ownership information and initial_shared_version
async fn get_object_details(
    client: &mut GrpcClient,
    object_id: sui::Address,
) -> Result<(sui::ObjectReference, Option<u64>)> {
    let mut ledger = client.ledger_client();
    
    let response = ledger
        .get_object(proto::GetObjectRequest {
            object_id: Some(object_id.to_string()),
            version: None,
            read_mask: Some(prost_types::FieldMask {
                paths: vec![
                    "object_id".to_string(), 
                    "version".to_string(), 
                    "digest".to_string(),
                    "owner".to_string(),
                ],
            }),
        })
        .await
        .context("Failed to get object")?
        .into_inner();

    if let Some(object) = response.object {
        let id = object.object_id
            .context("Missing object_id")?
            .parse()
            .context("Failed to parse object_id")?;
        let version = object.version
            .context("Missing version")?;
        let digest = object.digest
            .context("Missing digest")?
            .parse()
            .context("Failed to parse digest")?;

        let obj_ref = sui::ObjectReference::new(id, version, digest);
        
        // Extract initial_shared_version from owner information
        let initial_shared_version = object.owner
            .and_then(|owner| {
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

/// Try to create a new block for the app instance
/// This function calls the try_create_block Move function on the blockchain
/// which will check if conditions are met to create a new block
pub async fn try_create_block_tx(
    client: &mut GrpcClient,
    app_instance_str: &str,
) -> Result<String> {
    info!("Creating try_create_block transaction for app_instance: {}", app_instance_str);
    
    // Parse IDs
    let package_id = get_coordination_package_id()
        .context("Failed to get coordination package ID")?;
    let app_instance_id = get_app_instance_id(app_instance_str)
        .context("Failed to parse app instance ID")?;
    let clock_object_id = get_clock_object_id();
    
    debug!("Package ID: {}", package_id);
    debug!("App instance ID: {}", app_instance_id);
    
    // Parse sender and secret key
    let (sender, sk) = load_sender_from_env()?;
    debug!("Sender: {}", sender);

    // Build transaction using TransactionBuilder
    let mut tb = sui_transaction_builder::TransactionBuilder::new();
    tb.set_sender(sender);
    tb.set_gas_budget(100_000_000); // 0.1 SUI

    // Get gas price and gas object using provided client
    let gas_price = get_reference_gas_price(client).await?;
    tb.set_gas_price(gas_price);
    debug!("Gas price: {}", gas_price);

    // Select gas coin using parallel-safe coin management
    let rpc_url = get_rpc_url()?;
    let (gas_coin, _gas_guard) = match fetch_coin(&rpc_url, sender, 100_000_000).await? {
        Some((coin, guard)) => (coin, guard),
        None => {
            error!("No available coins with sufficient balance for gas");
            return Err(anyhow!("No available coins with sufficient balance for gas"));
        }
    };
    
    let gas_input = sui_transaction_builder::unresolved::Input::owned(
        gas_coin.object_id(),
        gas_coin.object_ref.version(),
        *gas_coin.object_ref.digest(),
    );
    tb.add_gas_objects(vec![gas_input]);
    debug!("Gas coin selected: id={} ver={} digest={} balance={}", 
        gas_coin.object_id(), gas_coin.object_ref.version(), gas_coin.object_ref.digest(), gas_coin.balance);

    // Get current version and ownership info of app_instance object
    let (app_instance_ref, initial_shared_version) = get_object_details(client, app_instance_id).await
        .context("Failed to get app instance details")?;
    
    // Create input based on whether object is shared or owned
    let app_instance_input = if let Some(shared_version) = initial_shared_version {
        debug!("Using shared object input for app_instance with initial_shared_version={}", shared_version);
        sui_transaction_builder::unresolved::Input::shared(
            app_instance_id,
            shared_version,
            true // mutable
        )
    } else {
        debug!("Using owned object input for app_instance");
        sui_transaction_builder::unresolved::Input::owned(
            *app_instance_ref.object_id(),
            app_instance_ref.version(),
            *app_instance_ref.digest(),
        )
    };
    let app_instance_arg = tb.input(app_instance_input);

    // Clock object (shared)
    let clock_input = sui_transaction_builder::unresolved::Input::shared(clock_object_id, 1, false);
    let clock_arg = tb.input(clock_input);

    // Function call: coordination::app_instance::try_create_block
    let func = sui_transaction_builder::Function::new(
        package_id,
        "app_instance".parse()
            .map_err(|e| anyhow!("Failed to parse module name 'app_instance': {}", e))?,
        "try_create_block".parse()
            .map_err(|e| anyhow!("Failed to parse function name 'try_create_block': {}", e))?,
        vec![],
    );
    tb.move_call(func, vec![app_instance_arg, clock_arg]);

    // Finalize and sign
    let tx = tb.finish()?;
    let sig = sk.sign_transaction(&tx)?;

    // Execute transaction via gRPC using provided client
    let mut exec = client.execution_client();
    let req = proto::ExecuteTransactionRequest {
        transaction: Some(tx.into()),
        signatures: vec![sig.into()],
        read_mask: Some(FieldMask { paths: vec!["finality".into(), "transaction".into()] }),
    };

    debug!("Sending try_create_block transaction...");
    let tx_start = std::time::Instant::now();
    let exec_result = exec.execute_transaction(req).await;
    let tx_elapsed_ms = tx_start.elapsed().as_millis();

    let resp = match exec_result {
        Ok(r) => r,
        Err(e) => {
            error!("Transaction execution network error: {}", e);
            return Err(anyhow!("Failed to execute try_create_block transaction: {}", e));
        }
    };
    let tx_resp = resp.into_inner();

    // Debug: Log full transaction response details
    debug!("Transaction response finality: {:?}", tx_resp.finality);
    if let Some(ref transaction) = tx_resp.transaction {
        debug!("Transaction digest: {:?}", transaction.digest);
        debug!("Transaction effects: {:?}", transaction.effects);
        
        // Check for errors in transaction effects
        if let Some(ref effects) = transaction.effects {
            debug!("Effects status: {:?}", effects.status);
            if let Some(ref status) = effects.status {
                if status.error.is_some() {
                    let error_msg = status.error.as_ref().unwrap();
                    let error_str = format!("{:?}", error_msg);
                    // Check if this is a NonEntryFunctionInvoked error
                    if error_str.contains("NonEntryFunctionInvoked") {
                        error!("try_create_block is not an entry function! Error: {}", error_str);
                        return Err(anyhow!("try_create_block is not an entry function in the Move contract"));
                    }
                    // This might be expected if conditions aren't met (e.g., not enough time passed)
                    debug!("Transaction failed with expected error (conditions not met): {}", error_str);
                    return Err(anyhow!("Block creation conditions not met: {}", error_str));
                }
            }
        }
    }

    // Check transaction was successful
    if tx_resp.finality.is_none() {
        error!("Transaction did not achieve finality");
        return Err(anyhow!("Transaction did not achieve finality"));
    }

    // Check for transaction success in effects
    let tx_successful = tx_resp.transaction
        .as_ref()
        .and_then(|t| t.effects.as_ref())
        .and_then(|e| e.status.as_ref())
        .map(|s| s.error.is_none())
        .unwrap_or(false);

    let tx_digest = tx_resp
        .transaction
        .as_ref()
        .and_then(|t| t.digest.as_ref())
        .context("Failed to get transaction digest")?
        .to_string();

    if tx_successful {
        info!(
            "try_create_block transaction executed successfully: {} (took {}ms)",
            tx_digest, tx_elapsed_ms
        );
    } else {
        error!(
            "try_create_block transaction failed: {} (took {}ms)",
            tx_digest, tx_elapsed_ms
        );
        return Err(anyhow!("Transaction failed - check Move contract for try_create_block entry function"));
    }
    
    // Wait for transaction to be included
    if let Err(e) = wait_for_transaction(client, &tx_digest, Some(5000)).await {
        warn!("Failed to wait for try_create_block transaction to be available: {}", e);
        // Continue anyway if the transaction was successful
    }
    
    Ok(tx_digest)
}

