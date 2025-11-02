use anyhow::Result;
use std::env;
use std::str::FromStr;
use sui_rpc::field::FieldMask;
use sui_rpc::proto::sui::rpc::v2::GetObjectRequest;
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

/// Data structures for registry list output
#[derive(Debug, Clone, serde::Serialize)]
pub struct RegistryListData {
    pub registry_id: String,
    pub name: String,
    pub version: u32,
    pub admin: String,
    pub developers: Vec<DeveloperData>,
    pub apps: Vec<AppData>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DeveloperData {
    pub name: String,
    pub github: String,
    pub owner: String,
    pub image: Option<String>,
    pub description: Option<String>,
    pub site: Option<String>,
    pub agents: Vec<AgentData>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AgentData {
    pub name: String,
    pub chains: Vec<String>,
    pub image: Option<String>,
    pub description: Option<String>,
    pub site: Option<String>,
    pub methods: Vec<MethodData>,
    pub default_method: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MethodData {
    pub name: String,
    pub docker_image: String,
    pub docker_sha256: Option<String>,
    pub min_memory_gb: u16,
    pub min_cpu_cores: u16,
    pub requires_tee: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AppData {
    pub name: String,
    pub owner: String,
    pub description: Option<String>,
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
        sui_sdk_types::Argument,      // clock_arg
    ) -> Vec<sui_sdk_types::Argument>,
{
    let shared_state = SharedSuiState::get_instance();
    let package_id = shared_state.get_coordination_package_id_required();

    execute_transaction_block(
        package_id,
        vec![(
            vec![registry_id.to_string()],
            "registry".to_string(),
            function_name.to_string(),
            build_args,
        )],
        None, // Use default gas budget
        None, // No modules to publish
        None, // Use default max computation cost
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
pub(crate) async fn create_registry(
    name: String,
    package_id: Option<String>,
) -> Result<CreateRegistryResult> {
    info!("Creating registry '{}'", name);

    // Get package ID from parameter or environment variable
    let package_id_str = package_id.unwrap_or_else(|| {
        env::var("SILVANA_REGISTRY_PACKAGE").unwrap_or_else(|_| {
            // Fall back to the coordination package if registry package not set
            let shared_state = SharedSuiState::get_instance();
            shared_state
                .get_coordination_package_id_required()
                .to_string()
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
            move |tb: &mut sui_transaction_builder::TransactionBuilder,
                  _object_args,
                  _clock_arg| {
                // Create the name argument
                let name_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                    bytes: name.clone().into_bytes(),
                }));

                vec![name_arg]
            },
        )],
        None, // Use default gas budget
        None, // No modules to publish
        None, // Use default max computation cost
    )
    .await?;

    // Extract the registry ID from created objects
    // For now, we need to query the transaction to get the created object
    // This is a simplification - in practice you'd need to parse the transaction effects
    info!("Registry created successfully with tx: {}", tx_digest);

    // TODO: Extract actual registry ID from transaction effects
    // For now, return a placeholder
    Ok(CreateRegistryResult {
        registry_id: "0x0000000000000000000000000000000000000000000000000000000000000000"
            .to_string(),
        tx_digest,
    })
}

/// Add a developer to the registry
pub(crate) async fn add_developer(
    registry_id: &str,
    developer_owner: String,
    name: String,
    github: Option<String>,
    image: Option<String>,
    description: Option<String>,
    site: Option<String>,
) -> Result<String> {
    info!(
        "Adding developer '{}' to registry '{}'",
        name, registry_id
    );

    // Use empty string if github is not provided
    let github = github.unwrap_or_else(|| String::new());

    let developer_address = sui::Address::from_str(&developer_owner)
        .map_err(|e| anyhow::anyhow!("Invalid developer owner address: {}", e))?;

    execute_registry_function(
        registry_id,
        "add_developer",
        move |tb, object_args, clock_arg| {
            let registry_arg = *object_args.get(0).expect("Registry argument required");

            let developer_owner_arg = tb.input(sui_transaction_builder::Serialized(
                &developer_address,
            ));
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
                developer_owner_arg,
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
pub(crate) async fn update_developer(
    registry_id: &str,
    name: String,
    github: Option<String>,
    image: Option<String>,
    description: Option<String>,
    site: Option<String>,
) -> Result<String> {
    info!(
        "Updating developer '{}' in registry '{}'",
        name, registry_id
    );

    // Use empty string if github is not provided
    let github = github.unwrap_or_else(|| String::new());

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
pub(crate) async fn remove_developer(
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
pub(crate) async fn add_agent(
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
pub(crate) async fn update_agent(
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

/// Remove an agent from a developer in the registry
pub(crate) async fn remove_agent(
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

/// Add a method to an agent in the registry
pub(crate) async fn add_method(
    registry_id: &str,
    developer: String,
    agent_name: String,
    method_name: String,
    docker_image: String,
    docker_sha256: Option<String>,
    min_memory_gb: u16,
    min_cpu_cores: u16,
    requires_tee: bool,
) -> Result<String> {
    info!(
        "Adding method '{}' to agent '{}' for developer '{}' in registry '{}'",
        method_name, agent_name, developer, registry_id
    );

    execute_registry_function(
        registry_id,
        "add_method",
        move |tb, object_args, clock_arg| {
            let registry_arg = *object_args.get(0).expect("Registry argument required");

            let developer_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: developer.clone().into_bytes(),
            }));
            let agent_name_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: agent_name.clone().into_bytes(),
            }));
            let method_name_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: method_name.clone().into_bytes(),
            }));
            let docker_image_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: docker_image.clone().into_bytes(),
            }));
            let docker_sha256_arg = tb.input(sui_transaction_builder::Serialized(&MoveOption {
                vec: docker_sha256
                    .clone()
                    .map(|s| MoveString {
                        bytes: s.into_bytes(),
                    })
                    .into_iter()
                    .collect(),
            }));
            let min_memory_gb_arg = tb.input(sui_transaction_builder::Serialized(&min_memory_gb));
            let min_cpu_cores_arg = tb.input(sui_transaction_builder::Serialized(&min_cpu_cores));
            let requires_tee_arg = tb.input(sui_transaction_builder::Serialized(&requires_tee));

            vec![
                registry_arg,
                developer_arg,
                agent_name_arg,
                method_name_arg,
                docker_image_arg,
                docker_sha256_arg,
                min_memory_gb_arg,
                min_cpu_cores_arg,
                requires_tee_arg,
                clock_arg,
            ]
        },
    )
    .await
    .map_err(|e| {
        debug!("Failed to add method: {}", e);
        e
    })
}

/// Update a method in the registry
pub(crate) async fn update_method(
    registry_id: &str,
    developer: String,
    agent_name: String,
    method_name: String,
    docker_image: String,
    docker_sha256: Option<String>,
    min_memory_gb: u16,
    min_cpu_cores: u16,
    requires_tee: bool,
) -> Result<String> {
    info!(
        "Updating method '{}' for agent '{}' for developer '{}' in registry '{}'",
        method_name, agent_name, developer, registry_id
    );

    execute_registry_function(
        registry_id,
        "update_method",
        move |tb, object_args, clock_arg| {
            let registry_arg = *object_args.get(0).expect("Registry argument required");

            let developer_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: developer.clone().into_bytes(),
            }));
            let agent_name_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: agent_name.clone().into_bytes(),
            }));
            let method_name_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: method_name.clone().into_bytes(),
            }));
            let docker_image_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: docker_image.clone().into_bytes(),
            }));
            let docker_sha256_arg = tb.input(sui_transaction_builder::Serialized(&MoveOption {
                vec: docker_sha256
                    .clone()
                    .map(|s| MoveString {
                        bytes: s.into_bytes(),
                    })
                    .into_iter()
                    .collect(),
            }));
            let min_memory_gb_arg = tb.input(sui_transaction_builder::Serialized(&min_memory_gb));
            let min_cpu_cores_arg = tb.input(sui_transaction_builder::Serialized(&min_cpu_cores));
            let requires_tee_arg = tb.input(sui_transaction_builder::Serialized(&requires_tee));

            vec![
                registry_arg,
                developer_arg,
                agent_name_arg,
                method_name_arg,
                docker_image_arg,
                docker_sha256_arg,
                min_memory_gb_arg,
                min_cpu_cores_arg,
                requires_tee_arg,
                clock_arg,
            ]
        },
    )
    .await
    .map_err(|e| {
        debug!("Failed to update method: {}", e);
        e
    })
}

/// Remove a method from an agent in the registry
pub(crate) async fn remove_method(
    registry_id: &str,
    developer: String,
    agent_name: String,
    method_name: String,
) -> Result<String> {
    info!(
        "Removing method '{}' from agent '{}' for developer '{}' in registry '{}'",
        method_name, agent_name, developer, registry_id
    );

    execute_registry_function(
        registry_id,
        "remove_method",
        move |tb, object_args, clock_arg| {
            let registry_arg = *object_args.get(0).expect("Registry argument required");

            let developer_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: developer.clone().into_bytes(),
            }));
            let agent_name_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: agent_name.clone().into_bytes(),
            }));
            let method_name_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: method_name.clone().into_bytes(),
            }));

            vec![
                registry_arg,
                developer_arg,
                agent_name_arg,
                method_name_arg,
                clock_arg,
            ]
        },
    )
    .await
    .map_err(|e| {
        debug!("Failed to remove method: {}", e);
        e
    })
}

/// Set the default method for an agent
pub(crate) async fn set_default_method(
    registry_id: &str,
    developer: String,
    agent_name: String,
    method_name: String,
) -> Result<String> {
    info!(
        "Setting default method '{}' for agent '{}' for developer '{}' in registry '{}'",
        method_name, agent_name, developer, registry_id
    );

    execute_registry_function(
        registry_id,
        "set_default_method",
        move |tb, object_args, clock_arg| {
            let registry_arg = *object_args.get(0).expect("Registry argument required");

            let developer_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: developer.clone().into_bytes(),
            }));
            let agent_name_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: agent_name.clone().into_bytes(),
            }));
            let method_name_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: method_name.clone().into_bytes(),
            }));

            vec![
                registry_arg,
                developer_arg,
                agent_name_arg,
                method_name_arg,
                clock_arg,
            ]
        },
    )
    .await
    .map_err(|e| {
        debug!("Failed to set default method: {}", e);
        e
    })
}

/// Remove the default method from an agent
pub(crate) async fn remove_default_method(
    registry_id: &str,
    developer: String,
    agent_name: String,
) -> Result<String> {
    info!(
        "Removing default method from agent '{}' for developer '{}' in registry '{}'",
        agent_name, developer, registry_id
    );

    execute_registry_function(
        registry_id,
        "remove_default_method",
        move |tb, object_args, clock_arg| {
            let registry_arg = *object_args.get(0).expect("Registry argument required");

            let developer_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: developer.clone().into_bytes(),
            }));
            let agent_name_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: agent_name.clone().into_bytes(),
            }));

            vec![registry_arg, developer_arg, agent_name_arg, clock_arg]
        },
    )
    .await
    .map_err(|e| {
        debug!("Failed to remove default method: {}", e);
        e
    })
}

/// Add an app to the registry
pub(crate) async fn add_app(
    registry_id: &str,
    name: String,
    owner: String,
    description: Option<String>,
) -> Result<String> {
    info!("Adding app '{}' to registry '{}'", name, registry_id);

    let owner_address = sui::Address::from_str(&owner)
        .map_err(|e| anyhow::anyhow!("Invalid owner address: {}", e))?;

    execute_registry_function(
        registry_id,
        "add_app",
        move |tb, object_args, clock_arg| {
            let registry_arg = *object_args.get(0).expect("Registry argument required");

            let name_arg = tb.input(sui_transaction_builder::Serialized(&MoveString {
                bytes: name.clone().into_bytes(),
            }));
            let owner_arg = tb.input(sui_transaction_builder::Serialized(&owner_address));
            let description_arg = tb.input(sui_transaction_builder::Serialized(&MoveOption {
                vec: description
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
                owner_arg,
                description_arg,
                clock_arg,
            ]
        },
    )
    .await
    .map_err(|e| {
        debug!("Failed to add app: {}", e);
        e
    })
}

/// Update an app in the registry
pub(crate) async fn update_app(
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
pub(crate) async fn remove_app(registry_id: &str, name: String) -> Result<String> {
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

/// List all contents of a registry
pub(crate) async fn list_registry(registry_id: &str) -> Result<RegistryListData> {
    info!("Listing registry '{}'", registry_id);

    // Fetch the registry object
    let registry_obj = crate::fetch::object::fetch_object(registry_id).await?;

    // Extract registry metadata
    let name = registry_obj
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let version = registry_obj
        .get("version")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let admin = registry_obj
        .get("admin")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Fetch developers
    let developers = fetch_all_developers(&registry_obj).await?;

    // Fetch apps
    let apps = fetch_all_apps(&registry_obj).await?;

    Ok(RegistryListData {
        registry_id: registry_id.to_string(),
        name,
        version,
        admin,
        developers,
        apps,
    })
}

/// Fetch all developers and their agents/methods from the registry
async fn fetch_all_developers(registry_obj: &serde_json::Value) -> Result<Vec<DeveloperData>> {
    let mut developers = Vec::new();

    // Get the developers dynamic fields
    let developers_fields = registry_obj
        .get("developers")
        .and_then(|v| v.get("fields"))
        .and_then(|v| v.as_array());

    if let Some(fields) = developers_fields {
        for field in fields {
            let developer_name = field
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let field_id = field
                .get("field_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if !field_id.is_empty() {
                // Fetch the developer object
                match fetch_developer(field_id, developer_name).await {
                    Ok(developer) => developers.push(developer),
                    Err(e) => {
                        warn!("Failed to fetch developer '{}': {}", developer_name, e);
                    }
                }
            }
        }
    }

    Ok(developers)
}

/// Fetch a single developer and all their agents
async fn fetch_developer(field_id: &str, developer_name: &str) -> Result<DeveloperData> {
    debug!("Fetching developer: {}", developer_name);

    // Fetch the field wrapper
    let field_obj = crate::fetch::object::fetch_object(field_id).await?;

    // Extract the actual developer object ID from the value field
    let developer_id = field_obj
        .get("value")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Developer field has no value"))?;

    // Fetch the actual developer object
    let dev_obj = crate::fetch::object::fetch_object(developer_id).await?;

    // Extract developer metadata
    let name = dev_obj
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let github = dev_obj
        .get("github")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let owner = dev_obj
        .get("owner")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let image = dev_obj
        .get("image")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let description = dev_obj
        .get("description")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let site = dev_obj
        .get("site")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Fetch agents
    let agents = fetch_all_agents(&dev_obj).await?;

    Ok(DeveloperData {
        name,
        github,
        owner,
        image,
        description,
        site,
        agents,
    })
}

/// Fetch all agents for a developer
async fn fetch_all_agents(dev_obj: &serde_json::Value) -> Result<Vec<AgentData>> {
    let mut agents = Vec::new();

    // Get the agents dynamic fields
    let agents_fields = dev_obj
        .get("agents")
        .and_then(|v| v.get("fields"))
        .and_then(|v| v.as_array());

    if let Some(fields) = agents_fields {
        for field in fields {
            let agent_name = field
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let field_id = field
                .get("field_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if !field_id.is_empty() {
                match fetch_agent(field_id, agent_name).await {
                    Ok(agent) => agents.push(agent),
                    Err(e) => {
                        warn!("Failed to fetch agent '{}': {}", agent_name, e);
                    }
                }
            }
        }
    }

    Ok(agents)
}

/// Fetch a single agent and all their methods
async fn fetch_agent(field_id: &str, agent_name: &str) -> Result<AgentData> {
    debug!("Fetching agent: {}", agent_name);

    // Fetch the field wrapper
    let field_obj = crate::fetch::object::fetch_object(field_id).await?;

    // Extract the actual agent object ID from the value field
    let agent_id = field_obj
        .get("value")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Agent field has no value"))?;

    // Fetch the actual agent object
    let agent_obj = crate::fetch::object::fetch_object(agent_id).await?;

    // Extract agent metadata
    let name = agent_obj
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let image = agent_obj
        .get("image")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let description = agent_obj
        .get("description")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let site = agent_obj
        .get("site")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Extract chains array
    let chains = agent_obj
        .get("chains")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    // Extract default method
    let default_method = agent_obj
        .get("default_method")
        .and_then(|v| parse_method_from_json(v).ok());

    // Extract methods from VecMap
    let methods = parse_methods_vecmap(&agent_obj)?;

    Ok(AgentData {
        name,
        chains,
        image,
        description,
        site,
        methods,
        default_method: default_method.map(|m| m.name),
    })
}

/// Parse methods from VecMap structure
fn parse_methods_vecmap(agent_obj: &serde_json::Value) -> Result<Vec<MethodData>> {
    let mut methods = Vec::new();

    // VecMap is stored as { contents: [...] }
    let contents = agent_obj
        .get("methods")
        .and_then(|v| v.get("contents"))
        .and_then(|v| v.as_array());

    if let Some(contents_arr) = contents {
        for entry in contents_arr {
            // Each entry is { key: "method_name", value: { ... method fields ... } }
            let method_name = entry
                .get("key")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if let Some(value) = entry.get("value") {
                if let Ok(mut method) = parse_method_from_json(value) {
                    method.name = method_name.to_string();
                    methods.push(method);
                } else {
                    warn!("Failed to parse method '{}' from value: {}", method_name, serde_json::to_string_pretty(value).unwrap_or_default());
                }
            }
        }
    }

    Ok(methods)
}

/// Parse method data from JSON
fn parse_method_from_json(value: &serde_json::Value) -> Result<MethodData> {

    let docker_image = value
        .get("docker_image")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let docker_sha256 = value
        .get("docker_sha256")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Try to get min_memory_gb - values come as floats from protobuf
    let min_memory_gb = value
        .get("min_memory_gb")
        .and_then(|v| v.as_f64())
        .map(|f| f.round() as u16)
        .unwrap_or(1);

    // Try to get min_cpu_cores - values come as floats from protobuf
    let min_cpu_cores = value
        .get("min_cpu_cores")
        .and_then(|v| v.as_f64())
        .map(|f| f.round() as u16)
        .unwrap_or(1);

    let requires_tee = value
        .get("requires_tee")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    Ok(MethodData {
        name: String::new(), // Will be filled in by caller
        docker_image,
        docker_sha256,
        min_memory_gb,
        min_cpu_cores,
        requires_tee,
    })
}

/// Fetch all apps from the registry
async fn fetch_all_apps(registry_obj: &serde_json::Value) -> Result<Vec<AppData>> {
    let mut apps = Vec::new();

    // Get the apps dynamic fields
    let apps_fields = registry_obj
        .get("apps")
        .and_then(|v| v.get("fields"))
        .and_then(|v| v.as_array());

    if let Some(fields) = apps_fields {
        for field in fields {
            let app_name = field
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let field_id = field
                .get("field_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if !field_id.is_empty() {
                match fetch_app(field_id, app_name).await {
                    Ok(app) => apps.push(app),
                    Err(e) => {
                        warn!("Failed to fetch app '{}': {}", app_name, e);
                    }
                }
            }
        }
    }

    Ok(apps)
}

/// Fetch a single app
async fn fetch_app(field_id: &str, app_name: &str) -> Result<AppData> {
    debug!("Fetching app: {}", app_name);

    // Fetch the field wrapper
    let field_obj = crate::fetch::object::fetch_object(field_id).await?;

    // Extract the actual app object ID from the value field
    let app_id = field_obj
        .get("value")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("App field has no value"))?;

    // Fetch the actual app object
    let app_obj = crate::fetch::object::fetch_object(app_id).await?;

    // Extract app metadata
    let name = app_obj
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let owner = app_obj
        .get("owner")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let description = app_obj
        .get("description")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Ok(AppData {
        name,
        owner,
        description,
    })
}
