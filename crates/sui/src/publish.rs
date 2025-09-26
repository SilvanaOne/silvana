//! Move package build and publish utilities

use crate::state::SharedSuiState;
use crate::transactions::{PublishOptions, execute_transaction_block};
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::process::Command;
use tracing::{debug, info};

/// Result of building a Move package
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildResult {
    /// Base64-encoded compiled modules
    pub modules: Vec<String>,
    /// Base64-encoded dependencies
    pub dependencies: Vec<String>,
    /// Package digest
    pub digest: Vec<u8>,
}

/// Build a Move package at the specified path
///
/// This function runs `sui move build` with the `--dump-bytecode-as-base64` flag
/// to get the compiled bytecode in a format suitable for publishing.
///
/// # Arguments
///
/// * `path` - Path to the Move package directory
///
/// # Returns
///
/// Returns a `BuildResult` containing the compiled modules, dependencies, and digest
///
/// # Example
///
/// ```no_run
/// # async fn example() -> anyhow::Result<()> {
/// use sui::publish::build_move_package;
///
/// let result = build_move_package("examples/add/move").await?;
/// println!("Built {} modules", result.modules.len());
/// # Ok(())
/// # }
/// ```
pub async fn build_move_package(path: &str) -> Result<BuildResult> {
    build_move_package_with_retries(path, false, 3).await
}

/// Internal function to build with retry logic
async fn build_move_package_with_retries(
    path: &str,
    with_unpublished: bool,
    max_retries: u32,
) -> Result<BuildResult> {
    let mut last_error = None;

    for attempt in 0..max_retries {
        if attempt > 0 {
            debug!("Retrying build, attempt {}/{}", attempt + 1, max_retries);
            // Small delay before retry
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }

        match build_move_package_internal(path, with_unpublished).await {
            Ok(result) => return Ok(result),
            Err(e) => {
                debug!("Build attempt {} failed: {}", attempt + 1, e);
                last_error = Some(e);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow!("Build failed after {} retries", max_retries)))
}

/// Internal build function
async fn build_move_package_internal(path: &str, with_unpublished: bool) -> Result<BuildResult> {
    info!("Building Move package at: {}", path);

    // Construct the sui move build command
    let mut cmd = Command::new("sui");
    cmd.arg("move")
        .arg("build")
        .arg("--dump-bytecode-as-base64");

    if with_unpublished {
        cmd.arg("--with-unpublished-dependencies");
    }

    cmd.arg("--path").arg(path);

    debug!("Running command: {:?}", cmd);

    // Execute the command
    let output = cmd
        .output()
        .map_err(|e| anyhow!("Failed to execute sui move build: {}", e))?;

    // Check if the command was successful
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "Move build failed with status {}: {}",
            output.status.code().unwrap_or(-1),
            stderr
        ));
    }

    // Parse the JSON output
    let stdout = String::from_utf8_lossy(&output.stdout);
    debug!("Build output: {}", stdout);

    // The output should be a JSON object with modules, dependencies, and digest
    let result: serde_json::Value = serde_json::from_str(&stdout)
        .map_err(|e| anyhow!("Failed to parse build output as JSON: {}", e))?;

    // Extract the fields
    let modules: Vec<String> = result["modules"]
        .as_array()
        .ok_or_else(|| anyhow!("Missing 'modules' field in build output"))?
        .iter()
        .map(|v| v.as_str().ok_or_else(|| anyhow!("Invalid module format")))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    let dependencies: Vec<String> = result["dependencies"]
        .as_array()
        .ok_or_else(|| anyhow!("Missing 'dependencies' field in build output"))?
        .iter()
        .map(|v| {
            v.as_str()
                .ok_or_else(|| anyhow!("Invalid dependency format"))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    let digest: Vec<u8> = result["digest"]
        .as_array()
        .ok_or_else(|| anyhow!("Missing 'digest' field in build output"))?
        .iter()
        .map(|v| {
            v.as_u64()
                .ok_or_else(|| anyhow!("Invalid digest format"))
                .map(|n| n as u8)
        })
        .collect::<Result<Vec<_>>>()?;

    info!(
        "Successfully built Move package: {} modules, {} dependencies",
        modules.len(),
        dependencies.len()
    );

    Ok(BuildResult {
        modules,
        dependencies,
        digest,
    })
}

/// Result of publishing a Move package
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishResult {
    /// The transaction digest
    pub digest: String,
    /// The published package ID
    pub package_id: String,
}

/// Extract the package ID from a publish transaction
async fn extract_package_id_from_digest(digest: &str) -> Result<String> {
    use prost_types::FieldMask;
    use sui_rpc::field::FieldMaskUtil;
    use sui_rpc::proto::sui::rpc::v2beta2 as proto;

    debug!("Extracting package ID from transaction digest: {}", digest);

    // Wait a bit for the transaction to be fully processed
    tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;

    let shared_state = SharedSuiState::get_instance();
    let mut client = shared_state.get_sui_client();
    let mut ledger = client.ledger_client();

    // Request transaction with effects using proper field mask
    // The paths are relative to ExecutedTransaction, not GetTransactionResponse
    let req = proto::GetTransactionRequest {
        digest: Some(digest.to_string()),
        read_mask: Some(FieldMask::from_paths(["digest", "effects"])),
    };

    let resp = ledger
        .get_transaction(req)
        .await
        .map_err(|e| anyhow!("Failed to fetch transaction: {}", e))?;

    let response = resp.into_inner();

    // Check if transaction exists
    if let Some(executed_tx) = &response.transaction {
        debug!("Transaction fetched successfully");

        // Extract package ID from effects
        if let Some(effects) = &executed_tx.effects {
            debug!("Checking effects for changed objects...");
            debug!("  Changed objects count: {}", effects.changed_objects.len());

            // Look for created package in changed objects
            use proto::changed_object;

            for changed_obj in &effects.changed_objects {
                // A newly published package has:
                // - input_state = DoesNotExist (1)
                // - output_state = PackageWrite (3)
                if let (Some(input_state), Some(output_state)) =
                    (changed_obj.input_state, changed_obj.output_state)
                {
                    if input_state == changed_object::InputObjectState::DoesNotExist as i32
                        && output_state == changed_object::OutputObjectState::PackageWrite as i32
                    {
                        if let Some(object_id) = &changed_obj.object_id {
                            info!("Found published package ID: {}", object_id);
                            return Ok(object_id.clone());
                        }
                    }
                }
            }

            // If not found in changed_objects, look for immutable objects
            // Package objects are immutable and have output_owner with kind = Immutable (4)
            for changed_obj in &effects.changed_objects {
                if let Some(input_state) = changed_obj.input_state {
                    if input_state == changed_object::InputObjectState::DoesNotExist as i32 {
                        // This is a newly created object
                        if let Some(output_owner) = &changed_obj.output_owner {
                            use proto::owner;
                            if let Some(kind) = output_owner.kind {
                                if kind == owner::OwnerKind::Immutable as i32 {
                                    if let Some(object_id) = &changed_obj.object_id {
                                        info!(
                                            "Found published package ID (immutable object): {}",
                                            object_id
                                        );
                                        return Ok(object_id.clone());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        } else {
            return Err(anyhow!("Transaction effects not present in response"));
        }
    } else {
        return Err(anyhow!("Transaction not found"));
    }

    Err(anyhow!(
        "Could not find published package ID in transaction effects"
    ))
}

/// Publish a Move package to the blockchain
///
/// This function takes a BuildResult (from build_move_package) and publishes
/// the compiled modules to the blockchain.
///
/// # Arguments
///
/// * `build_result` - The result from building a Move package
/// * `gas_budget` - Optional gas budget for the transaction
///
/// # Returns
///
/// Returns a `PublishResult` containing the transaction digest and package ID
///
/// # Example
///
/// ```no_run
/// # async fn example() -> anyhow::Result<()> {
/// use sui::publish::{build_move_package_ignore_chain, publish_move_package};
///
/// // First build the package
/// let build_result = build_move_package_ignore_chain("move/constants").await?;
///
/// // Then publish it
/// let publish_result = publish_move_package(build_result, None).await?;
/// println!("Published package with digest: {}", publish_result.digest);
/// # Ok(())
/// # }
/// ```
pub async fn publish_move_package(
    build_result: BuildResult,
    gas_budget: Option<u64>,
) -> Result<PublishResult> {
    info!(
        "Publishing Move package with {} modules",
        build_result.modules.len()
    );

    // Get the shared state to access the package ID
    let shared_state = SharedSuiState::get_instance();
    let package_id = shared_state.get_coordination_package_id_required();

    // Create publish options from the build result
    let publish_options = PublishOptions {
        modules: build_result.modules,
        dependencies: build_result.dependencies,
    };

    // Execute the transaction with publish options
    // We pass an empty operations vector since we're only publishing
    let digest = execute_transaction_block::<
        fn(
            &mut sui_transaction_builder::TransactionBuilder,
            Vec<sui_sdk_types::Argument>,
            sui_sdk_types::Argument,
        ) -> Vec<sui_sdk_types::Argument>,
    >(
        package_id,
        vec![], // No additional operations, just publish
        gas_budget,
        Some(publish_options),
        None,
    )
    .await?;

    info!("Successfully published package with digest: {}", digest);

    // Extract the package ID from the transaction
    let package_id = extract_package_id_from_digest(&digest).await?;

    Ok(PublishResult { digest, package_id })
}

/// Update dependencies in a BuildResult
///
/// This allows updating package dependencies after building, useful when
/// dependencies need to be resolved at runtime (e.g., after publishing another package)
///
/// # Arguments
///
/// * `build_result` - The build result to update
/// * `new_dependencies` - New dependency addresses to add or replace
///
/// # Returns
///
/// Returns the updated BuildResult
pub fn update_package_dependencies(
    mut build_result: BuildResult,
    new_dependencies: Vec<String>,
) -> BuildResult {
    info!(
        "Updating package dependencies: {} new dependencies",
        new_dependencies.len()
    );

    // Replace or append dependencies
    // For now, we'll append new dependencies to existing ones
    // In the future, we might want to be smarter about replacing specific ones
    for dep in new_dependencies {
        if !build_result.dependencies.contains(&dep) {
            debug!("Adding dependency: {}", dep);
            build_result.dependencies.push(dep);
        }
    }

    build_result
}

/// Build and publish a Move package in one step
///
/// This is a convenience function that combines building and publishing.
///
/// # Arguments
///
/// * `path` - Path to the Move package directory
/// * `gas_budget` - Optional gas budget for the transaction
///
/// # Returns
///
/// Returns a `PublishResult` containing the transaction digest and package ID
pub async fn build_and_publish_move_package(
    path: &str,
    gas_budget: Option<u64>,
) -> Result<PublishResult> {
    info!("Building and publishing Move package at: {}", path);

    // Build the package
    let build_result = build_move_package(path).await?;

    // Publish it
    publish_move_package(build_result, gas_budget).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_build_move_package() -> Result<()> {
        // Load environment variables
        let _ = dotenvy::dotenv();

        // Initialize tracing for better debug output
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .try_init();

        // Try to build the add example package
        // Use a relative path from the crate root
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("../../examples/add/move");

        // Check if the path exists
        if !path.exists() {
            println!("Skipping test: {} does not exist", path.display());
            return Ok(());
        }

        println!("Testing build_move_package with path: {}", path.display());

        // Build the package with ignore-chain flag (more portable for tests)
        let path_str = path.to_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid path: contains non-UTF8 characters"))?;
        let result = build_move_package(path_str).await?;

        // Verify we got some results
        assert!(
            !result.modules.is_empty(),
            "Should have at least one module"
        );
        assert!(!result.digest.is_empty(), "Should have a digest");

        println!("Built package successfully:");
        println!("  Modules: {}", result.modules.len());
        println!("  Dependencies: {}", result.dependencies.len());
        println!("  Digest length: {}", result.digest.len());

        // Verify the digest is 32 bytes (SHA256)
        assert_eq!(result.digest.len(), 32, "Digest should be 32 bytes");

        // Print first few bytes of the first module to verify it's base64
        if let Some(first_module) = result.modules.first() {
            println!(
                "  First module (first 50 chars): {}...",
                &first_module[..50.min(first_module.len())]
            );
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_build_move_package_with_chain() -> Result<()> {
        // Load environment variables
        let _ = dotenvy::dotenv();

        // Initialize tracing
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .try_init();

        // Use a relative path from the crate root
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("../../examples/add/move");

        // Check if the path exists
        if !path.exists() {
            println!("Skipping test: {} does not exist", path.display());
            return Ok(());
        }

        println!(
            "Testing build_move_package (with chain) with path: {}",
            path.display()
        );

        // Build the package without ignore-chain flag
        let path_str = path.to_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid path: contains non-UTF8 characters"))?;
        let result = build_move_package(path_str).await?;

        // Verify we got some results
        assert!(
            !result.modules.is_empty(),
            "Should have at least one module"
        );
        assert!(!result.digest.is_empty(), "Should have a digest");

        println!("Built package with chain successfully:");
        println!("  Modules: {}", result.modules.len());
        println!("  Dependencies: {}", result.dependencies.len());
        println!("  Digest length: {}", result.digest.len());

        Ok(())
    }
}
