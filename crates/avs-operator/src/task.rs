use crate::config::{Config, load_deployment_data};
use crate::error::{AvsOperatorError, Result};
use ethereum::{AvsConfig, AvsContracts, AvsOperatorClient, avs_parse_address as parse_address};
use tracing::{info, error};
use tokio::time::{sleep, Duration};
use rand::Rng;

/// Generate a random task name for testing
pub fn generate_random_task_name() -> String {
    let adjectives = ["Quick", "Lazy", "Sleepy", "Noisy", "Hungry"];
    let nouns = ["Fox", "Dog", "Cat", "Mouse", "Bear"];
    
    let mut rng = rand::thread_rng();
    let adjective = adjectives[rng.gen_range(0..adjectives.len())];
    let noun = nouns[rng.gen_range(0..nouns.len())];
    let number = rng.gen_range(0..1000);
    
    format!("{}{}{}", adjective, noun, number)
}

/// Create a new task on the AVS
pub async fn create_task(config: &Config, task_name: String, use_aggregator: bool) -> Result<()> {
    info!("Creating new task: {}", task_name);
    
    // Load deployment data
    let deployment = load_deployment_data(config.chain_id)?;
    
    // Create AVS config and contracts - use appropriate key based on flag
    let private_key = if use_aggregator {
        config.admin_private_key.clone()
    } else {
        config.operator_private_key.clone()
    };
    
    let avs_config = AvsConfig {
        rpc_url: config.rpc_url.clone(),
        ws_url: config.ws_url.clone(),
        private_key,
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
    let client = AvsOperatorClient::new(avs_config, avs_contracts)
        .await
        .map_err(|e| AvsOperatorError::Other(e))?;
    
    info!("Creating task from address: {}", client.address());
    
    // Create the task on the service manager contract
    match client.create_new_task(task_name.clone()).await {
        Ok(tx_hash) => {
            info!("Task '{}' created successfully. Tx: {}", task_name, tx_hash);
        }
        Err(e) => {
            error!("Failed to create task: {}", e);
            return Err(AvsOperatorError::TaskCreation(format!("Failed to create task: {}", e)));
        }
    }
    
    Ok(())
}

/// Continuously create tasks at regular intervals
pub async fn start_creating_tasks(config: &Config, interval_seconds: u64) -> Result<()> {
    info!("Starting continuous task creation with interval of {} seconds", interval_seconds);
    
    loop {
        let task_name = generate_random_task_name();
        info!("Creating new task with name: {}", task_name);
        
        match create_task(config, task_name.clone(), false).await {
            Ok(_) => {
                info!("Task created with name: {}", task_name);
            }
            Err(e) => {
                error!("Failed to create task: {}", e);
                // Continue even if one task fails
            }
        }
        
        // Wait for the specified interval
        sleep(Duration::from_secs(interval_seconds)).await;
    }
}

/// Create a single task with a specific name
pub async fn create_single_task(config: &Config, task_name: Option<String>) -> Result<()> {
    let name = task_name.unwrap_or_else(generate_random_task_name);
    create_task(config, name, false).await
}