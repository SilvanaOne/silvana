//! Move package build and publish utilities

use anyhow::{anyhow, Result};
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
async fn build_move_package_with_retries(path: &str, with_unpublished: bool, max_retries: u32) -> Result<BuildResult> {
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
    
    cmd.arg("--path")
        .arg(path);
    
    debug!("Running command: {:?}", cmd);
    
    // Execute the command
    let output = cmd.output()
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
        .map(|v| v.as_str().ok_or_else(|| anyhow!("Invalid dependency format")))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .map(|s| s.to_string())
        .collect();
    
    let digest: Vec<u8> = result["digest"]
        .as_array()
        .ok_or_else(|| anyhow!("Missing 'digest' field in build output"))?
        .iter()
        .map(|v| v.as_u64().ok_or_else(|| anyhow!("Invalid digest format")).map(|n| n as u8))
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

/// Build a Move package without chain-specific dependencies
///
/// This is useful for building packages that will be deployed to multiple chains
/// or for testing purposes. This will also include unpublished dependencies.
///
/// # Arguments
///
/// * `path` - Path to the Move package directory
///
/// # Returns
///
/// Returns a `BuildResult` containing the compiled modules, dependencies, and digest
pub async fn build_move_package_ignore_chain(path: &str) -> Result<BuildResult> {
    build_move_package_with_retries(path, true, 3).await
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
        let result = build_move_package_ignore_chain(path.to_str().unwrap()).await?;
        
        // Verify we got some results
        assert!(!result.modules.is_empty(), "Should have at least one module");
        assert!(!result.digest.is_empty(), "Should have a digest");
        
        println!("Built package successfully:");
        println!("  Modules: {}", result.modules.len());
        println!("  Dependencies: {}", result.dependencies.len());
        println!("  Digest length: {}", result.digest.len());
        
        // Verify the digest is 32 bytes (SHA256)
        assert_eq!(result.digest.len(), 32, "Digest should be 32 bytes");
        
        // Print first few bytes of the first module to verify it's base64
        if let Some(first_module) = result.modules.first() {
            println!("  First module (first 50 chars): {}...", &first_module[..50.min(first_module.len())]);
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
        
        println!("Testing build_move_package (with chain) with path: {}", path.display());
        
        // Build the package without ignore-chain flag
        let result = build_move_package(path.to_str().unwrap()).await?;
        
        // Verify we got some results
        assert!(!result.modules.is_empty(), "Should have at least one module");
        assert!(!result.digest.is_empty(), "Should have a digest");
        
        println!("Built package with chain successfully:");
        println!("  Modules: {}", result.modules.len());
        println!("  Dependencies: {}", result.dependencies.len());
        println!("  Digest length: {}", result.digest.len());
        
        Ok(())
    }
}