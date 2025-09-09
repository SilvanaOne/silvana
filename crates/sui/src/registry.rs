use anyhow::Result;
use tracing::{debug, info};

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
        sui_sdk_types::Argument, // registry_arg
        sui_sdk_types::Argument, // clock_arg
    ) -> Vec<sui_sdk_types::Argument>,
{
    let shared_state = SharedSuiState::get_instance();
    let package_id = shared_state.get_coordination_package_id();
    
    execute_transaction_block(
        package_id,
        vec![(
            registry_id.to_string(),
            function_name.to_string(),
            build_args,
        )],
        None, // Use default gas budget
    )
    .await
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
        move |tb, registry_arg, clock_arg| {
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
        move |tb, registry_arg, clock_arg| {
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
        move |tb, registry_arg, clock_arg| {
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
        move |tb, registry_arg, clock_arg| {
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
        move |tb, registry_arg, clock_arg| {
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
        move |tb, registry_arg, clock_arg| {
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
        move |tb, registry_arg, clock_arg| {
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
        move |tb, registry_arg, clock_arg| {
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
        move |tb, registry_arg, clock_arg| {
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