use anyhow::Result;
use std::path::PathBuf;
use sui::publish::build_move_package;

/// Helper to get the path to the example Move package
fn get_example_package_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("../../examples/add/move");
    path
}

#[tokio::test]
async fn test_build_move_package_with_dependencies() -> Result<()> {
    // Load environment variables
    let _ = dotenvy::dotenv();

    // Initialize tracing
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    let path = get_example_package_path();

    // Skip test if the example package doesn't exist
    if !path.exists() {
        eprintln!(
            "Skipping test: example package not found at {}",
            path.display()
        );
        return Ok(());
    }

    println!("Building Move package at: {}", path.display());

    // Build with unpublished dependencies
    let result = build_move_package(path.to_str().unwrap()).await?;

    // Verify the result
    assert!(
        !result.modules.is_empty(),
        "Should have at least one module"
    );
    assert_eq!(
        result.modules.len(),
        1,
        "The 'add' package should have exactly one module"
    );

    // Verify dependencies (should include Sui stdlib and our local deps)
    assert!(!result.dependencies.is_empty(), "Should have dependencies");
    assert!(
        result.dependencies.len() >= 3,
        "Should have at least 3 dependencies (Sui, MoveStdlib, and local deps)"
    );

    // Verify digest
    assert_eq!(
        result.digest.len(),
        32,
        "Digest should be 32 bytes (SHA256)"
    );

    // Verify the module is base64 encoded
    let first_module = &result.modules[0];
    assert!(!first_module.is_empty(), "Module should not be empty");

    // Base64 strings should only contain valid base64 characters
    assert!(
        first_module
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '='),
        "Module should be valid base64"
    );

    println!("✅ Package built successfully:");
    println!("  - Modules: {}", result.modules.len());
    println!("  - Dependencies: {}", result.dependencies.len());
    println!("  - Digest length: {} bytes", result.digest.len());
    println!("  - First module length: {} chars", first_module.len());

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

    let path = get_example_package_path();

    // Skip test if the example package doesn't exist
    if !path.exists() {
        eprintln!(
            "Skipping test: example package not found at {}",
            path.display()
        );
        return Ok(());
    }

    println!("Building Move package with chain at: {}", path.display());

    // Build with chain dependencies
    let result = build_move_package(path.to_str().unwrap()).await?;

    // Verify the result
    assert!(
        !result.modules.is_empty(),
        "Should have at least one module"
    );
    assert!(!result.dependencies.is_empty(), "Should have dependencies");
    assert_eq!(result.digest.len(), 32, "Digest should be 32 bytes");

    println!("✅ Package built with chain successfully:");
    println!("  - Modules: {}", result.modules.len());
    println!("  - Dependencies: {}", result.dependencies.len());
    println!("  - Digest length: {} bytes", result.digest.len());

    Ok(())
}

#[tokio::test]
async fn test_build_nonexistent_package() -> Result<()> {
    // Load environment variables
    let _ = dotenvy::dotenv();

    // Try to build a non-existent package
    let result = build_move_package("/path/that/does/not/exist").await;

    // Should fail
    assert!(result.is_err(), "Building non-existent package should fail");

    if let Err(e) = result {
        println!("Expected error for non-existent package: {}", e);
        assert!(
            e.to_string().contains("Failed to execute sui move build")
                || e.to_string().contains("Move build failed"),
            "Error should mention build failure"
        );
    }

    Ok(())
}

#[tokio::test]
async fn test_build_output_structure() -> Result<()> {
    // Load environment variables
    let _ = dotenvy::dotenv();

    let path = get_example_package_path();

    // Skip test if the example package doesn't exist
    if !path.exists() {
        eprintln!(
            "Skipping test: example package not found at {}",
            path.display()
        );
        return Ok(());
    }

    // Build the package
    let result = build_move_package(path.to_str().unwrap()).await?;

    // Verify the structure of the output
    assert_eq!(
        result.modules.len(),
        1,
        "The 'add' package should have exactly one module"
    );
    assert_eq!(
        result.digest.len(),
        32,
        "Digest should be 32 bytes (SHA256)"
    );

    // Verify dependencies are all valid Sui addresses
    for dep in &result.dependencies {
        assert!(
            dep.starts_with("0x"),
            "Each dependency should be a hex address"
        );
        // Remove 0x prefix and check it's a valid hex string
        let hex_str = &dep[2..];
        assert!(
            hex_str.chars().all(|c| c.is_ascii_hexdigit()),
            "Dependency should be a valid hex string"
        );
    }

    // Print digest as hex for debugging
    let digest_hex: String = result.digest.iter().map(|b| format!("{:02x}", b)).collect();
    println!("Package digest (hex): {}", digest_hex);
    println!("✅ Build output structure verified");

    Ok(())
}

#[tokio::test]
async fn test_module_content_format() -> Result<()> {
    // Load environment variables
    let _ = dotenvy::dotenv();

    let path = get_example_package_path();

    // Skip test if the example package doesn't exist
    if !path.exists() {
        eprintln!(
            "Skipping test: example package not found at {}",
            path.display()
        );
        return Ok(());
    }

    let result = build_move_package(path.to_str().unwrap()).await?;

    // Check that we can decode the base64
    use base64::{Engine, engine::general_purpose::STANDARD};
    for (i, module) in result.modules.iter().enumerate() {
        match STANDARD.decode(module) {
            Ok(bytes) => {
                println!("Module {} decoded successfully: {} bytes", i, bytes.len());
                // Move bytecode should start with a magic number
                // The actual magic number may vary, but it should be non-empty
                assert!(!bytes.is_empty(), "Decoded module should not be empty");
            }
            Err(e) => {
                panic!("Failed to decode module {} as base64: {}", i, e);
            }
        }
    }

    // Check dependencies are also valid base64
    for (i, dep) in result.dependencies.iter().enumerate() {
        // Dependencies might be addresses (hex strings starting with 0x)
        if dep.starts_with("0x") {
            println!("Dependency {} is an address: {}", i, dep);
        } else {
            match STANDARD.decode(dep) {
                Ok(bytes) => {
                    println!(
                        "Dependency {} decoded successfully: {} bytes",
                        i,
                        bytes.len()
                    );
                }
                Err(_) => {
                    // Some dependencies might be plain strings
                    println!("Dependency {} is a plain string: {}", i, dep);
                }
            }
        }
    }

    println!("✅ Module content format verified");

    Ok(())
}
