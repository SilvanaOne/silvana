use anyhow::Result;
use std::env;
use std::str::FromStr;
use sui_sdk_types as sui;
use tracing::{debug, info, warn};

use crate::state::SharedSuiState;
use crate::transactions::execute_transaction_block;

/// Define Move types for serialization
#[derive(serde::Serialize)]
struct MoveString {
    bytes: Vec<u8>,
}

#[derive(serde::Serialize)]
struct MoveOption<T> {
    vec: Vec<T>,
}

/// Result from creating a registry
pub struct CreateRegistryResult {
    pub registry_id: String,
    pub tx_digest: String,
}

/// Common helper function to execute registry transactions
/// This follows the same pattern as app_instance.rs
async fn execute_registry_function<F>(
    registry_id: &str,
    function_name: &str,
    build_args: F,
) -> Result<String>
where
    F: Fn(
        &mut sui_transaction_builder::TransactionBuilder,
        Vec<sui_sdk_types::Argument>, // registry_args
        sui_sdk_types::Argument, // clock_arg
    ) -> Vec<sui_sdk_types::Argument>,
{
    let shared_state = SharedSuiState::get_instance();
    let package_id = shared_state.get_coordination_package_id();
    
    execute_transaction_block(
        package_id,
        vec![(
            vec![registry_id.to_string()],
            "registry".to_string(),
            function_name.to_string(),
            build_args,
        )],
        None, // Use default gas budget
    )
    .await
}

/// Create a new Silvana registry
/// 
/// # Arguments
/// * `name` - Name for the registry
/// * `package_id` - Optional package ID (uses env var SILVANA_REGISTRY_PACKAGE if not provided)
/// 
/// # Returns
/// CreateRegistryResult containing the registry ID and transaction digest
pub async fn create_registry(
    name: String,
    package_id: Option<String>,
) -> Result<CreateRegistryResult> {
    info!("Creating registry '{}'", name);
    
    // Get package ID from parameter or environment variable
    let package_id_str = package_id.unwrap_or_else(|| {
        env::var("SILVANA_REGISTRY_PACKAGE")
            .unwrap_or_else(|_| {
                // Fall back to the coordination package if registry package not set
                let shared_state = SharedSuiState::get_instance();
                shared_state.get_coordination_package_id().to_string()
            })
    });
    
    let package_id = sui::Address::from_str(&package_id_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse package ID '{}': {}", package_id_str, e))?;
    
    debug!("Using package ID: {}", package_id);
    
    // Use execute_transaction_block with empty object list for create_registry
    let tx_digest = execute_transaction_block(
        package_id,
        vec![(
            vec![], // No objects needed for create_registry
            "registry".to_string(),
            "create_registry".to_string(),
            move |tb: &mut sui_transaction_builder::TransactionBuilder, _object_args, _clock_arg| {
                // Create the string argument for the registry name
                let name_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                    bytes: name.clone().into_bytes(),
                }));
                
                // Function call only needs the name argument
                vec![name_arg]
            },
        )],
        None, // Use default gas budget
    )
    .await?;
    
    // Fetch the created registry object ID from the transaction
    let registry_id = fetch_created_object_from_transaction(&tx_digest).await
        .unwrap_or_else(|e| {
            warn!("Failed to extract created object from transaction {}: {}. Using transaction digest as fallback.", tx_digest, e);
            // For backwards compatibility, use a deterministic object ID based on tx digest
            // This will be replaced once we implement proper object extraction
            format!("0x{:0>64}", &tx_digest[..64.min(tx_digest.len())])
        });
    
    // Wait for the registry object to be available
    // This is important because the object might not be immediately queryable after creation
    wait_for_object_availability(&registry_id).await?;
    
    info!(
        "Registry created with ID: {} (tx: {})",
        registry_id, tx_digest
    );
    
    Ok(CreateRegistryResult {
        registry_id,
        tx_digest,
    })
}

/// Wait for an object to be available for querying
/// This is useful after creating new objects that need to be used immediately
async fn wait_for_object_availability(object_id: &str) -> Result<()> {
    use crate::state::SharedSuiState;
    use sui_rpc::proto::sui::rpc::v2beta2 as proto;
    use prost_types::FieldMask;
    use std::time::Duration;
    use tokio::time::sleep;
    
    debug!("Waiting for object {} to be available", object_id);
    
    let object_address = sui::Address::from_str(object_id)
        .map_err(|e| anyhow::anyhow!("Failed to parse object ID '{}': {}", object_id, e))?;
    
    let max_retries = 10;
    let mut retry_count = 0;
    
    while retry_count < max_retries {
        let mut client = SharedSuiState::get_instance().get_sui_client();
        let mut ledger = client.ledger_client();
        
        let response = ledger
            .get_object(proto::GetObjectRequest {
                object_id: Some(object_address.to_string()),
                version: None,
                read_mask: Some(FieldMask {
                    paths: vec!["object_id".to_string()],
                }),
            })
            .await;
        
        match response {
            Ok(resp) => {
                let inner = resp.into_inner();
                if inner.object.is_some() {
                    debug!("Object {} is now available", object_id);
                    return Ok(());
                }
            }
            Err(e) => {
                if retry_count == 0 {
                    debug!("Object {} not yet available: {}", object_id, e);
                }
            }
        }
        
        retry_count += 1;
        if retry_count < max_retries {
            let wait_ms = 500 * retry_count; // Exponential backoff: 500ms, 1s, 1.5s, etc.
            debug!("Waiting {}ms before retry {}/{}", wait_ms, retry_count, max_retries);
            sleep(Duration::from_millis(wait_ms as u64)).await;
        }
    }
    
    Err(anyhow::anyhow!(
        "Object {} not available after {} retries",
        object_id,
        max_retries
    ))
}

/// Fetch the created registry ID from transaction events
async fn fetch_created_object_from_transaction(tx_digest: &str) -> Result<String> {
    debug!("Fetching created registry ID from transaction events: {}", tx_digest);
    
    // Fetch transaction events as JSON
    let events_json = crate::transactions::fetch_transaction_events_as_json(tx_digest).await?;
    
    // Look for RegistryCreatedEvent in the events
    if let Some(events_array) = events_json.as_array() {
        for event in events_array {
            if let Some(event_type) = event["event_type"].as_str() {
                // Check if this is a RegistryCreatedEvent
                if event_type.contains("RegistryCreatedEvent") {
                    debug!("Found RegistryCreatedEvent");
                    
                    // The event data could be in parsed_json, contents, or direct fields
                    let event_data = if event["parsed_json"].is_object() && !event["parsed_json"]["id"].is_null() {
                        &event["parsed_json"]
                    } else if event["contents"].is_object() && !event["contents"]["id"].is_null() {
                        &event["contents"]
                    } else if !event["id"].is_null() {
                        event
                    } else {
                        warn!("RegistryCreatedEvent found but couldn't locate event data");
                        continue;
                    };
                    
                    // Extract the registry ID from the event
                    if let Some(registry_id) = event_data["id"].as_str() {
                        info!("Found registry ID from RegistryCreatedEvent: {}", registry_id);
                        return Ok(registry_id.to_string());
                    }
                }
            }
        }
    }
    
    // Fallback: try to get from output_objects if events don't work
    warn!("RegistryCreatedEvent not found, falling back to output objects");
    fetch_created_object_from_output_objects(tx_digest).await
}

/// Fallback method to fetch created object from output_objects
async fn fetch_created_object_from_output_objects(tx_digest: &str) -> Result<String> {
    use crate::state::SharedSuiState;
    use sui_rpc::proto::sui::rpc::v2beta2 as proto;
    use prost_types::FieldMask;
    
    let shared_state = SharedSuiState::get_instance();
    let mut client = shared_state.get_sui_client();
    
    // Parse transaction digest
    let digest = sui_sdk_types::Digest::from_str(tx_digest)
        .map_err(|e| anyhow::anyhow!("Failed to parse transaction digest: {}", e))?;
    
    // Fetch transaction with output_objects
    let mut ledger = client.ledger_client();
    let req = proto::GetTransactionRequest {
        digest: Some(digest.to_string()),
        read_mask: Some(FieldMask {
            paths: vec!["transaction.output_objects".into()],
        }),
    };
    
    let resp = ledger
        .get_transaction(req)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch transaction: {}", e))?;
    
    let response = resp.into_inner();
    
    if let Some(ref executed_tx) = response.transaction {
        if !executed_tx.output_objects.is_empty() {
            for output_object in &executed_tx.output_objects {
                // Check if this is a registry object by looking at the type
                if let Some(ref object_type) = output_object.object_type {
                    if object_type.contains("::registry::SilvanaRegistry") || 
                       object_type.contains("::registry::AgentRegistry") {
                        if let Some(ref object_id) = output_object.object_id {
                            return Ok(object_id.clone());
                        }
                    }
                }
            }
        }
    }
    
    Err(anyhow::anyhow!("No created objects found in transaction"))
}

/// Add a developer to the registry
/// 
/// # Arguments
/// * `registry_id` - The registry object ID
/// * `name` - Developer name
/// * `github` - GitHub username
/// * `image` - Optional image URL
/// * `description` - Optional description
/// * `site` - Optional website URL
/// 
/// # Returns
/// Transaction digest on success
pub async fn add_developer(
    registry_id: &str,
    name: String,
    github: String,
    image: Option<String>,
    description: Option<String>,
    site: Option<String>,
) -> Result<String> {
    info!(
        "Adding developer '{}' to registry '{}'",
        name, registry_id
    );
    
    execute_registry_function(
        registry_id,
        "add_developer",
        move |tb, object_args, clock_arg| {
            let registry_arg = *object_args.get(0).expect("Registry argument required");
            
            // Create string arguments
            let name_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: name.clone().into_bytes(),
            }));
            let github_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: github.clone().into_bytes(),
            }));
            
            // Create optional string arguments
            let image_arg = tb.input(sui_transaction_builder::Serialized(&MoveOption {
                vec: image
                    .clone()
                    .map(|s| MoveString {
                        bytes: s.into_bytes(),
                    })
                    .into_iter()
                    .collect(),
            }));
            let description_arg = tb.input(sui_transaction_builder::Serialized(&MoveOption {
                vec: description
                    .clone()
                    .map(|s| MoveString {
                        bytes: s.into_bytes(),
                    })
                    .into_iter()
                    .collect(),
            }));
            let site_arg = tb.input(sui_transaction_builder::Serialized(&MoveOption {
                vec: site
                    .clone()
                    .map(|s| MoveString {
                        bytes: s.into_bytes(),
                    })
                    .into_iter()
                    .collect(),
            }));
            
            // Return arguments in the order expected by the Move function:
            // registry, name, github, image, description, site, clock
            vec![
                registry_arg,
                name_arg,
                github_arg,
                image_arg,
                description_arg,
                site_arg,
                clock_arg,
            ]
        },
    )
    .await
    .map_err(|e| {
        debug!("Failed to add developer: {}", e);
        e
    })
}

/// Update a developer in the registry
pub async fn update_developer(
    registry_id: &str,
    name: String,
    github: String,
    image: Option<String>,
    description: Option<String>,
    site: Option<String>,
) -> Result<String> {
    info!(
        "Updating developer '{}' in registry '{}'",
        name, registry_id
    );
    
    execute_registry_function(
        registry_id,
        "update_developer",
        move |tb, object_args, clock_arg| {
            let registry_arg = *object_args.get(0).expect("Registry argument required");
            
            let name_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: name.clone().into_bytes(),
            }));
            let github_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: github.clone().into_bytes(),
            }));
            let image_arg = tb.input(sui_transaction_builder::Serialized(&MoveOption {
                vec: image
                    .clone()
                    .map(|s| MoveString {
                        bytes: s.into_bytes(),
                    })
                    .into_iter()
                    .collect(),
            }));
            let description_arg = tb.input(sui_transaction_builder::Serialized(&MoveOption {
                vec: description
                    .clone()
                    .map(|s| MoveString {
                        bytes: s.into_bytes(),
                    })
                    .into_iter()
                    .collect(),
            }));
            let site_arg = tb.input(sui_transaction_builder::Serialized(&MoveOption {
                vec: site
                    .clone()
                    .map(|s| MoveString {
                        bytes: s.into_bytes(),
                    })
                    .into_iter()
                    .collect(),
            }));
            
            vec![
                registry_arg,
                name_arg,
                github_arg,
                image_arg,
                description_arg,
                site_arg,
                clock_arg,
            ]
        },
    )
    .await
    .map_err(|e| {
        debug!("Failed to update developer: {}", e);
        e
    })
}

/// Remove a developer from the registry
pub async fn remove_developer(
    registry_id: &str,
    name: String,
    agent_names: Vec<String>,
) -> Result<String> {
    info!(
        "Removing developer '{}' from registry '{}'",
        name, registry_id
    );
    
    execute_registry_function(
        registry_id,
        "remove_developer",
        move |tb, object_args, clock_arg| {
            let registry_arg = *object_args.get(0).expect("Registry argument required");
            
            let name_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: name.clone().into_bytes(),
            }));
            let agent_names_arg = tb.input(sui_transaction_builder::Serialized(
                &agent_names
                    .clone()
                    .into_iter()
                    .map(|s| MoveString {
                        bytes: s.into_bytes(),
                    })
                    .collect::<Vec<_>>(),
            ));
            
            vec![registry_arg, name_arg, agent_names_arg, clock_arg]
        },
    )
    .await
    .map_err(|e| {
        debug!("Failed to remove developer: {}", e);
        e
    })
}

/// Add an agent to a developer in the registry
pub async fn add_agent(
    registry_id: &str,
    developer: String,
    name: String,
    image: Option<String>,
    description: Option<String>,
    site: Option<String>,
    chains: Vec<String>,
) -> Result<String> {
    info!(
        "Adding agent '{}' to developer '{}' in registry '{}'",
        name, developer, registry_id
    );
    
    execute_registry_function(
        registry_id,
        "add_agent",
        move |tb, object_args, clock_arg| {
            let registry_arg = *object_args.get(0).expect("Registry argument required");
            
            let developer_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: developer.clone().into_bytes(),
            }));
            let name_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: name.clone().into_bytes(),
            }));
            let image_arg = tb.input(sui_transaction_builder::Serialized(&MoveOption {
                vec: image
                    .clone()
                    .map(|s| MoveString {
                        bytes: s.into_bytes(),
                    })
                    .into_iter()
                    .collect(),
            }));
            let description_arg = tb.input(sui_transaction_builder::Serialized(&MoveOption {
                vec: description
                    .clone()
                    .map(|s| MoveString {
                        bytes: s.into_bytes(),
                    })
                    .into_iter()
                    .collect(),
            }));
            let site_arg = tb.input(sui_transaction_builder::Serialized(&MoveOption {
                vec: site
                    .clone()
                    .map(|s| MoveString {
                        bytes: s.into_bytes(),
                    })
                    .into_iter()
                    .collect(),
            }));
            let chains_arg = tb.input(sui_transaction_builder::Serialized(
                &chains
                    .clone()
                    .into_iter()
                    .map(|s| MoveString {
                        bytes: s.into_bytes(),
                    })
                    .collect::<Vec<_>>(),
            ));
            
            vec![
                registry_arg,
                developer_arg,
                name_arg,
                image_arg,
                description_arg,
                site_arg,
                chains_arg,
                clock_arg,
            ]
        },
    )
    .await
    .map_err(|e| {
        debug!("Failed to add agent: {}", e);
        e
    })
}

/// Update an agent in the registry
pub async fn update_agent(
    registry_id: &str,
    developer: String,
    name: String,
    image: Option<String>,
    description: Option<String>,
    site: Option<String>,
    chains: Vec<String>,
) -> Result<String> {
    info!(
        "Updating agent '{}' for developer '{}' in registry '{}'",
        name, developer, registry_id
    );
    
    execute_registry_function(
        registry_id,
        "update_agent",
        move |tb, object_args, clock_arg| {
            let registry_arg = *object_args.get(0).expect("Registry argument required");
            
            let developer_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: developer.clone().into_bytes(),
            }));
            let name_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: name.clone().into_bytes(),
            }));
            let image_arg = tb.input(sui_transaction_builder::Serialized(&MoveOption {
                vec: image
                    .clone()
                    .map(|s| MoveString {
                        bytes: s.into_bytes(),
                    })
                    .into_iter()
                    .collect(),
            }));
            let description_arg = tb.input(sui_transaction_builder::Serialized(&MoveOption {
                vec: description
                    .clone()
                    .map(|s| MoveString {
                        bytes: s.into_bytes(),
                    })
                    .into_iter()
                    .collect(),
            }));
            let site_arg = tb.input(sui_transaction_builder::Serialized(&MoveOption {
                vec: site
                    .clone()
                    .map(|s| MoveString {
                        bytes: s.into_bytes(),
                    })
                    .into_iter()
                    .collect(),
            }));
            let chains_arg = tb.input(sui_transaction_builder::Serialized(
                &chains
                    .clone()
                    .into_iter()
                    .map(|s| MoveString {
                        bytes: s.into_bytes(),
                    })
                    .collect::<Vec<_>>(),
            ));
            
            vec![
                registry_arg,
                developer_arg,
                name_arg,
                image_arg,
                description_arg,
                site_arg,
                chains_arg,
                clock_arg,
            ]
        },
    )
    .await
    .map_err(|e| {
        debug!("Failed to update agent: {}", e);
        e
    })
}

/// Remove an agent from the registry
pub async fn remove_agent(
    registry_id: &str,
    developer: String,
    name: String,
) -> Result<String> {
    info!(
        "Removing agent '{}' from developer '{}' in registry '{}'",
        name, developer, registry_id
    );
    
    execute_registry_function(
        registry_id,
        "remove_agent",
        move |tb, object_args, clock_arg| {
            let registry_arg = *object_args.get(0).expect("Registry argument required");
            
            let developer_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: developer.clone().into_bytes(),
            }));
            let name_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: name.clone().into_bytes(),
            }));
            
            vec![registry_arg, developer_arg, name_arg, clock_arg]
        },
    )
    .await
    .map_err(|e| {
        debug!("Failed to remove agent: {}", e);
        e
    })
}

/// Add an app to the registry
pub async fn add_app(
    registry_id: &str,
    name: String,
    description: Option<String>,
) -> Result<String> {
    info!("Adding app '{}' to registry '{}'", name, registry_id);
    
    execute_registry_function(
        registry_id,
        "add_app",
        move |tb, object_args, clock_arg| {
            let registry_arg = *object_args.get(0).expect("Registry argument required");
            
            let name_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: name.clone().into_bytes(),
            }));
            let description_arg = tb.input(sui_transaction_builder::Serialized(&MoveOption {
                vec: description
                    .clone()
                    .map(|s| MoveString {
                        bytes: s.into_bytes(),
                    })
                    .into_iter()
                    .collect(),
            }));
            
            vec![registry_arg, name_arg, description_arg, clock_arg]
        },
    )
    .await
    .map_err(|e| {
        debug!("Failed to add app: {}", e);
        e
    })
}

/// Update an app in the registry
pub async fn update_app(
    registry_id: &str,
    name: String,
    description: Option<String>,
) -> Result<String> {
    info!("Updating app '{}' in registry '{}'", name, registry_id);
    
    execute_registry_function(
        registry_id,
        "update_app",
        move |tb, object_args, clock_arg| {
            let registry_arg = *object_args.get(0).expect("Registry argument required");
            
            let name_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: name.clone().into_bytes(),
            }));
            let description_arg = tb.input(sui_transaction_builder::Serialized(&MoveOption {
                vec: description
                    .clone()
                    .map(|s| MoveString {
                        bytes: s.into_bytes(),
                    })
                    .into_iter()
                    .collect(),
            }));
            
            vec![registry_arg, name_arg, description_arg, clock_arg]
        },
    )
    .await
    .map_err(|e| {
        debug!("Failed to update app: {}", e);
        e
    })
}

/// Remove an app from the registry
pub async fn remove_app(
    registry_id: &str,
    name: String,
) -> Result<String> {
    info!("Removing app '{}' from registry '{}'", name, registry_id);
    
    execute_registry_function(
        registry_id,
        "remove_app",
        move |tb, object_args, clock_arg| {
            let registry_arg = *object_args.get(0).expect("Registry argument required");
            
            let name_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: name.clone().into_bytes(),
            }));
            
            vec![registry_arg, name_arg, clock_arg]
        },
    )
    .await
    .map_err(|e| {
        debug!("Failed to remove app: {}", e);
        e
    })
}