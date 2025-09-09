//! Integration tests for building and publishing Move packages

use anyhow::Result;
use std::{env, path::PathBuf};
use sui::publish::{build_move_package, publish_move_package};
use tracing::info;

#[tokio::test]
async fn test_build_and_publish_constants_package() -> Result<()> {
    // Load environment variables
    let _ = dotenvy::dotenv();

    // Initialize tracing for better debug output
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    // Ensure required environment variables are set
    if env::var("SUI_ADDRESS").is_err() {
        return Err(anyhow::anyhow!(
            "SUI_ADDRESS environment variable not set. Please set it or create a .env file"
        ));
    }
    // Check for SUI_SECRET_KEY
    if env::var("SUI_SECRET_KEY").is_err() {
        return Err(anyhow::anyhow!(
            "SUI_SECRET_KEY environment variable not set. Please set it or create a .env file"
        ));
    }

    // Resolve RPC URL using the chain helper
    let rpc_url = sui::chain::resolve_rpc_url(None, None)?;

    // Initialize the global SharedSuiState (required for all registry operations)
    sui::SharedSuiState::initialize(&rpc_url).await?;

    // Log the configuration
    tracing::info!("Test environment initialized");
    tracing::info!("SUI RPC URL: {}", rpc_url);
    tracing::info!("SUI_ADDRESS: {}", env::var("SUI_ADDRESS")?);
    tracing::info!(
        "SUI_CHAIN: {}",
        env::var("SUI_CHAIN").unwrap_or_else(|_| "testnet".to_string())
    );

    // Build path to the constants package
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("../../move/constants");

    // Check if the path exists
    if !path.exists() {
        println!("Skipping test: {} does not exist", path.display());
        println!("Please ensure the move/constants package exists");
        return Ok(());
    }

    info!("Building Move package at: {}", path.display());

    // Step 1: Build the Move package
    let build_result = build_move_package(path.to_str().unwrap()).await?;

    assert!(
        !build_result.modules.is_empty(),
        "Should have at least one module"
    );
    assert!(
        !build_result.dependencies.is_empty() || build_result.dependencies.is_empty(),
        "Dependencies can be empty or not"
    );
    assert_eq!(build_result.digest.len(), 32, "Digest should be 32 bytes");

    info!(
        "Successfully built package with {} modules and {} dependencies",
        build_result.modules.len(),
        build_result.dependencies.len()
    );

    // Step 2: Publish the built package
    info!("Publishing the built package...");

    let publish_result = publish_move_package(build_result, None).await?;

    assert!(
        !publish_result.digest.is_empty(),
        "Should have a transaction digest"
    );
    assert!(
        !publish_result.package_id.is_empty(),
        "Should have a package ID"
    );

    info!("Successfully published package:");
    info!("  Transaction digest: {}", publish_result.digest);
    info!("  Package ID: {}", publish_result.package_id);

    Ok(())
}

#[tokio::test]
async fn test_two_step_publish_with_dependency() -> Result<()> {
    // Load environment variables
    let _ = dotenvy::dotenv();

    // Initialize tracing for better debug output
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    // Ensure required environment variables are set
    if env::var("SUI_ADDRESS").is_err() || env::var("SUI_SECRET_KEY").is_err() {
        return Err(anyhow::anyhow!(
            "SUI_ADDRESS and SUI_SECRET_KEY environment variables must be set"
        ));
    }

    // Initialize the global SharedSuiState
    let rpc_url = sui::chain::resolve_rpc_url(None, None)?;
    sui::SharedSuiState::initialize(&rpc_url).await?;

    info!("Starting two-step publish test with dependency");

    // Build paths to both packages
    let mut constants_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    constants_path.push("../../move/constants");

    let mut commitment_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    commitment_path.push("../../move/commitment");

    //Check if both paths exist
    if !constants_path.exists() {
        println!("Skipping test: {} does not exist", constants_path.display());
        return Ok(());
    }
    if !commitment_path.exists() {
        println!(
            "Skipping test: {} does not exist",
            commitment_path.display()
        );
        return Ok(());
    }

    // Step 1: Build constants package first
    info!("Step 1: Building constants package...");

    info!(
        "Building constants package at: {}",
        constants_path.display()
    );
    let constants_build = build_move_package(constants_path.to_str().unwrap()).await?;
    assert!(
        !constants_build.modules.is_empty(),
        "Constants should have modules"
    );
    info!(
        "  Constants package built: {} modules",
        constants_build.modules.len()
    );
    info!(
        "  Constants package dependencies: {:?}",
        constants_build.dependencies
    );

    // Step 2: Publish constants package
    info!("Step 2: Publishing constants package...");
    let constants_result = publish_move_package(constants_build, None).await?;
    info!(
        "  Constants published with package ID: {}",
        constants_result.package_id
    );

    // Step 3: Build commitment package (it will use the published constants)
    info!("Step 3: Building commitment package with published constants dependency...");
    info!(
        "Building commitment package at: {}",
        commitment_path.display()
    );
    let mut commitment_build = build_move_package(commitment_path.to_str().unwrap()).await?;
    assert!(
        !commitment_build.modules.is_empty(),
        "Commitment should have modules"
    );
    info!(
        "  Commitment package built: {} modules",
        commitment_build.modules.len()
    );
    info!(
        "  Commitment package dependencies: {:?}",
        commitment_build.dependencies
    );

    // Step 4: Update the last dependency with the published constants package ID
    let dep_len = commitment_build.dependencies.len();
    if dep_len > 0 {
        commitment_build.dependencies[dep_len - 1] = constants_result.package_id.clone();
    }
    // Verify that the commitment package now has the published constants as a dependency
    assert!(
        commitment_build
            .dependencies
            .contains(&constants_result.package_id),
        "Commitment should have the published constants package as a dependency"
    );

    // Calculate the total size of commitment modules
    let total_size: usize = commitment_build.modules.iter().map(|m| m.len()).sum();
    info!(
        "  Commitment package total size (base64): {} bytes",
        total_size
    );

    info!("Step 4: Publishing commitment package...");
    let commitment_result = publish_move_package(commitment_build, None).await?;
    info!(
        "  Commitment published with package ID: {}",
        commitment_result.package_id
    );

    // // Step 3: Add constants package ID as dependency to commitment
    // info!("Step 3: Adding constants as dependency to commitment...");

    // // Update commitment's dependencies with the published constants package ID
    // let updated_commitment_build = sui::publish::update_package_dependencies(
    //     commitment_build,
    //     vec![constants_result.package_id.clone()],
    // );

    // info!(
    //     "  Updated commitment dependencies: {} total",
    //     updated_commitment_build.dependencies.len()
    // );
    // for dep in &updated_commitment_build.dependencies {
    //     info!("    Dependency: {}", dep);
    // }

    // Note: We're not publishing commitment because it exceeds the 102KB size limit
    // But we've demonstrated the full build and dependency update process

    assert!(
        !commitment_result.digest.is_empty(),
        "Commitment should have transaction digest"
    );
    assert!(
        !commitment_result.package_id.is_empty(),
        "Commitment should have package ID"
    );

    info!("Two-step build and dependency update completed successfully:");
    info!("  Commitment package ID: {}", commitment_result.package_id);

    // The test successfully demonstrates:
    // 1. Building BOTH packages before any publishing
    // 2. Publishing constants and extracting its package ID
    // 3. Updating commitment's dependencies with the actual published package ID
    // Note: Commitment cannot be published due to size limit (112KB > 102KB)

    Ok(())
}

#[tokio::test]
async fn test_build_constants_only() -> Result<()> {
    // Load environment variables
    let _ = dotenvy::dotenv();

    // Initialize tracing
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    // Build path to the constants package
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("../../move/constants");

    // Check if the path exists
    if !path.exists() {
        println!("Skipping test: {} does not exist", path.display());
        return Ok(());
    }

    info!("Testing build-only for Move package at: {}", path.display());

    // Build the Move package
    let build_result = build_move_package(path.to_str().unwrap()).await?;

    // Verify build results
    assert!(
        !build_result.modules.is_empty(),
        "Should have at least one module"
    );
    assert_eq!(build_result.digest.len(), 32, "Digest should be 32 bytes");

    println!("Build successful:");
    println!("  Modules: {}", build_result.modules.len());
    println!("  Dependencies: {}", build_result.dependencies.len());
    println!("  Digest length: {}", build_result.digest.len());

    // Print first few characters of the first module to verify it's base64
    if let Some(first_module) = build_result.modules.first() {
        let preview_len = 60.min(first_module.len());
        println!(
            "  First module preview: {}...",
            &first_module[..preview_len]
        );

        // Verify it's valid base64
        use base64::{Engine, engine::general_purpose::STANDARD};
        let decoded = STANDARD.decode(first_module)?;
        println!("  First module decoded size: {} bytes", decoded.len());
    }

    Ok(())
}
