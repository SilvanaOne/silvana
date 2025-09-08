use crate::config::{Config, load_deployment_data};
use crate::error::{AvsOperatorError, Result};
use ethereum::{AvsConfig, AvsContracts, AvsOperatorClient, Task, avs_parse_address as parse_address};
use tracing::{info, error};
use rand::Rng;

/// Sign and respond to a task
pub async fn sign_and_respond_to_task(
    client: &AvsOperatorClient,
    task_index: u32,
    task_created_block: u32,
    task_name: String,
) -> Result<()> {
    info!("Signing and responding to task {}: {}", task_index, task_name);
    
    // Sign the task response using the simple approach
    let signature = client
        .sign_task_message(&task_name)
        .await
        .map_err(|e| AvsOperatorError::TaskResponse(format!("Failed to sign task: {}", e)))?;
    
    // Get current block number
    let current_block = client.get_current_block()
        .await
        .map_err(|e| AvsOperatorError::TaskResponse(format!("Failed to get current block: {}", e)))?;
    
    // Encode the signature data
    let operators = vec![client.address()];
    let signatures = vec![signature.clone()];
    let signature_data = ethereum::encode_task_response(
        operators,
        signatures,
        ethereum::U256::from(current_block),
    );
    
    // Create task struct
    let task = Task {
        name: task_name.clone(),
        task_created_block,
    };
    
    // Submit the response to the service manager contract
    match client.respond_to_task(task, task_index, signature_data).await {
        Ok(tx_hash) => {
            info!("Successfully responded to task {}. Tx: {}", task_name, tx_hash);
        }
        Err(e) => {
            error!("Failed to submit task response: {}", e);
            return Err(AvsOperatorError::TaskResponse(format!("Failed to submit response: {}", e)));
        }
    }
    
    Ok(())
}


/// Monitor new tasks and respond to them
pub async fn monitor_new_tasks(config: &Config) -> Result<()> {
    info!("Starting task monitoring...");
    
    // Use polling implementation for now
    // WebSocket implementation requires more complex setup
    monitor_new_tasks_polling(config).await
}

/// Monitor tasks using polling instead of WebSocket
pub async fn monitor_new_tasks_polling(config: &Config) -> Result<()> {
    info!("Starting task monitoring with polling...");
    
    // Load deployment data
    let deployment = load_deployment_data(config.chain_id)?;
    
    // Create AVS config and contracts
    let avs_config = AvsConfig {
        rpc_url: config.rpc_url.clone(),
        ws_url: config.ws_url.clone(),
        private_key: config.operator_private_key.clone(),
        chain_id: config.chain_id,
    };
    
    let avs_contracts = AvsContracts {
        delegation_manager: parse_address(&deployment.delegation_manager)
            .map_err(|e| AvsOperatorError::Parse(format!("Invalid delegation manager address: {}", e)))?,
        avs_directory: parse_address(&deployment.avs_directory)
            .map_err(|e| AvsOperatorError::Parse(format!("Invalid AVS directory address: {}", e)))?,
        silvana_service_manager: parse_address(&deployment.silvana_service_manager)
            .map_err(|e| AvsOperatorError::Parse(format!("Invalid service manager address: {}", e)))?,
        stake_registry: parse_address(&deployment.stake_registry)
            .map_err(|e| AvsOperatorError::Parse(format!("Invalid stake registry address: {}", e)))?,
    };
    
    // Create operator client
    let client = AvsOperatorClient::new(avs_config, avs_contracts.clone())
        .await
        .map_err(|e| AvsOperatorError::Other(e))?;
    
    let mut latest_processed_block = client.get_current_block()
        .await
        .map_err(|e| AvsOperatorError::Rpc(format!("Failed to get block number: {}", e)))?;
    
    info!("Operator address: {}", client.address());
    info!("Monitoring for new tasks on service manager: {}", avs_contracts.silvana_service_manager);
    info!("Starting from block: {}", latest_processed_block);
    
    // Polling loop - runs until interrupted
    loop {
        let current_block = client.get_current_block()
            .await
            .map_err(|e| AvsOperatorError::Rpc(format!("Failed to get block number: {}", e)))?;
        
        if current_block > latest_processed_block {
            info!("Checking for new tasks from block {} to {}", 
                latest_processed_block, current_block);
            
            // Fetch and decode logs using the client method
            match client.get_logs(latest_processed_block, current_block).await {
                Ok(events) => {
                    if events.is_empty() {
                        info!("No new tasks found in blocks {} to {}", latest_processed_block, current_block);
                    } else {
                        info!("Found {} task(s) in blocks {} to {}", events.len(), latest_processed_block, current_block);
                        
                        for event in events {
                            info!("📋 New task {} detected: {} (created at block {})", 
                                event.task_index, event.task.name, event.task.task_created_block);
                            
                            // Check if operator should respond based on configured percentage
                            use rand::SeedableRng;
                            let mut rng = rand::rngs::StdRng::from_entropy();
                            let should_respond = rng.gen_bool(config.operator_response_percentage / 100.0);
                            
                            if should_respond {
                                info!("✅ Operator will respond to task {} ({}% chance)", 
                                    event.task_index, config.operator_response_percentage);
                                
                                if let Err(e) = sign_and_respond_to_task(
                                    &client,
                                    event.task_index,
                                    event.task.task_created_block,
                                    event.task.name,
                                ).await {
                                    error!("Failed to respond to task: {}", e);
                                }
                            } else {
                                info!("⏭️  Operator will not respond to task {} (random selection)", event.task_index);
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to fetch logs: {}", e);
                }
            }
            
            latest_processed_block = current_block;
        }
        
        // Wait before next poll (12 seconds for Holesky block time)
        tokio::time::sleep(tokio::time::Duration::from_secs(12)).await;
    }
}