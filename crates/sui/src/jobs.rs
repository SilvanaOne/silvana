use anyhow::{Result, Context, anyhow};
use std::env;
use std::str::FromStr;
use sui_rpc::field::FieldMask;
use sui_rpc::proto::sui::rpc::v2beta2 as proto;
use sui_rpc::Client as GrpcClient;
use sui_sdk_types as sui;
use sui_crypto::SuiSigner;
use tracing::{debug, info, error};

use crate::chain::{get_reference_gas_price, pick_gas_object, load_sender_from_env};

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

/// Create and submit a transaction to start a job
pub async fn start_job_tx(
    client: &mut GrpcClient,
    app_instance_str: &str,
    job_id: u64,
) -> Result<String> {
    info!("Creating start_app_job transaction for job_id: {}", job_id);
    
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

    // Select gas object
    let gas_ref = pick_gas_object(client, sender).await
        .context("Failed to pick gas object")?;
    let gas_input = sui_transaction_builder::unresolved::Input::owned(
        *gas_ref.object_id(),
        gas_ref.version(),
        *gas_ref.digest(),
    );
    tb.add_gas_objects(vec![gas_input]);
    debug!("Gas object added: {:?}", gas_ref);

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
    let job_id_arg = tb.input(sui_transaction_builder::Serialized(&job_id));

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
    tb.move_call(func, vec![app_instance_arg, job_id_arg, clock_arg]);

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
            error!("Transaction execution error: {}", e);
            return Err(anyhow!("Failed to execute transaction: {}", e));
        }
    };
    let tx_resp = resp.into_inner();

    // Check transaction was successful
    if tx_resp.finality.is_none() {
        return Err(anyhow!("Transaction did not achieve finality"));
    }

    let tx_digest = tx_resp
        .transaction
        .as_ref()
        .and_then(|t| t.digest.as_ref())
        .context("Failed to get transaction digest")?
        .to_string();

    info!(
        "start_app_job transaction executed: {} (took {}ms)",
        tx_digest, tx_elapsed_ms
    );

    Ok(tx_digest)
}

/// Create and submit a transaction to complete a job
pub async fn complete_job_tx(
    client: &mut GrpcClient,
    app_instance_str: &str,
    job_id: u64,
) -> Result<String> {
    info!("Creating complete_app_job transaction for job_id: {}", job_id);
    
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

    // Get gas price and gas object using provided client
    let gas_price = get_reference_gas_price(client).await?;
    tb.set_gas_price(gas_price);

    let gas_ref = pick_gas_object(client, sender).await
        .context("Failed to pick gas object")?;
    let gas_input = sui_transaction_builder::unresolved::Input::owned(
        *gas_ref.object_id(),
        gas_ref.version(),
        *gas_ref.digest(),
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
    let job_id_arg = tb.input(sui_transaction_builder::Serialized(&job_id));

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
    tb.move_call(func, vec![app_instance_arg, job_id_arg, clock_arg]);

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
            error!("Transaction execution error: {:?}", e);
            return Err(anyhow!("Failed to execute transaction: {}", e));
        }
    };
    let tx_resp = resp.into_inner();

    // Check transaction was successful
    if tx_resp.finality.is_none() {
        return Err(anyhow!("Transaction did not achieve finality"));
    }

    let tx_digest = tx_resp
        .transaction
        .as_ref()
        .and_then(|t| t.digest.as_ref())
        .context("Failed to get transaction digest")?
        .to_string();

    info!(
        "complete_app_job transaction executed: {} (took {}ms)",
        tx_digest, tx_elapsed_ms
    );

    Ok(tx_digest)
}

/// Create and submit a transaction to fail a job
pub async fn fail_job_tx(
    client: &mut GrpcClient,
    app_instance_str: &str,
    job_id: u64,
    error_message: &str,
) -> Result<String> {
    info!("Creating fail_app_job transaction for job_id: {} with error: {}", job_id, error_message);
    
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

    // Get gas price and gas object using provided client
    let gas_price = get_reference_gas_price(client).await?;
    tb.set_gas_price(gas_price);

    let gas_ref = pick_gas_object(client, sender).await
        .context("Failed to pick gas object")?;
    let gas_input = sui_transaction_builder::unresolved::Input::owned(
        *gas_ref.object_id(),
        gas_ref.version(),
        *gas_ref.digest(),
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
    let job_id_arg = tb.input(sui_transaction_builder::Serialized(&job_id));

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
    tb.move_call(func, vec![app_instance_arg, job_id_arg, error_arg, clock_arg]);

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

    // Check transaction was successful
    if tx_resp.finality.is_none() {
        return Err(anyhow!("Transaction did not achieve finality"));
    }

    let tx_digest = tx_resp
        .transaction
        .as_ref()
        .and_then(|t| t.digest.as_ref())
        .context("Failed to get transaction digest")?
        .to_string();

    info!(
        "fail_app_job transaction executed: {} (took {}ms)",
        tx_digest, tx_elapsed_ms
    );

    Ok(tx_digest)
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

