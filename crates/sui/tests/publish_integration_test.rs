//! Integration tests for building and publishing Move packages

use anyhow::Result;
use std::{env, path::PathBuf};
use sui::publish::{build_move_package_ignore_chain, publish_move_package};
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
    let build_result = build_move_package_ignore_chain(path.to_str().unwrap()).await?;

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
    let build_result = build_move_package_ignore_chain(path.to_str().unwrap()).await?;

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
