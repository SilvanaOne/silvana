use crate::coordinator;
use crate::error::{CoordinatorError, Result};
use crate::example;
use anyhow::anyhow;
use std::collections::HashMap;
use tracing::{info, warn};

pub async fn handle_start_command(
    rpc_url: Option<String>,
    package_id: Option<String>,
    use_tee: bool,
    container_timeout: u64,
    grpc_socket_path: String,
    instance: Option<String>,
    settle: bool,
    chain: &str,
    chain_override: Option<String>,
    config_map: &HashMap<String, String>,
) -> Result<()> {
    // Config already fetched and injected above
    // Special handling for SILVANA_REGISTRY_PACKAGE if package_id arg is provided
    if package_id.is_some() {
        // Override the environment variable if it was set from config
        if config_map.contains_key("SILVANA_REGISTRY_PACKAGE") {
            println!("  ⏩ Overriding SILVANA_REGISTRY_PACKAGE with --package-id argument");
        }
    }

    // Check if SUI_ADDRESS and SUI_SECRET_KEY are set, generate if not
    let env_file_path = std::path::Path::new(".env");
    if std::env::var("SUI_ADDRESS").is_err() || std::env::var("SUI_SECRET_KEY").is_err() {
        // Check if .env file exists
        if !env_file_path.exists() {
            println!("🔑 SUI credentials not found, generating new keypair...");

            // Generate a new Sui keypair
            let sui_keypair = example::generate_sui_keypair("coordinator")
                .map_err(|e| anyhow!("Failed to generate Sui keypair: {}", e))?;

            println!("✅ Generated new Sui keypair:");
            println!("   Address: {}", sui_keypair.address);

            // Set the environment variables for the current session
            unsafe {
                std::env::set_var("SUI_ADDRESS", &sui_keypair.address);
                std::env::set_var("SUI_SECRET_KEY", &sui_keypair.private_key);
            }

            // Write to .env file
            let env_content = format!(
                "# Auto-generated Sui credentials\n\
                SUI_ADDRESS={}\n\
                SUI_SECRET_KEY={}\n\
                SUI_CHAIN={}\n",
                sui_keypair.address, sui_keypair.private_key, chain
            );

            std::fs::write(env_file_path, env_content)
                .map_err(|e| anyhow!("Failed to write .env file: {}", e))?;

            println!("📝 Saved credentials to .env file");

            // Auto-fund on devnet
            if chain == "devnet" {
                println!("💰 Requesting funds from devnet faucet...");
                match sui::request_tokens_from_faucet(
                    "devnet",
                    &sui_keypair.address,
                    Some(10.0),
                )
                .await
                {
                    Ok(tx_digest) => {
                        println!("✅ Faucet request successful!");
                        if tx_digest != "unknown" {
                            println!("   Transaction: {}", tx_digest);
                            println!(
                                "   🔗 Explorer: https://suiscan.xyz/devnet/tx/{}",
                                tx_digest
                            );
                        } else {
                            println!(
                                "   🔗 Explorer: https://suiscan.xyz/devnet/account/{}",
                                sui_keypair.address
                            );
                        }
                        println!("   Waiting for transaction to be processed...");
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    }
                    Err(e) => {
                        eprintln!("⚠️  Failed to request funds from faucet: {}", e);
                        eprintln!("   Please manually fund the address:");
                        eprintln!(
                            "   silvana faucet sui --address {} --amount 10",
                            sui_keypair.address
                        );
                    }
                }
            } else {
                println!("⚠️  Please fund this address before running operations:");
                println!("   silvana faucet sui --address {}", sui_keypair.address);
            }
        } else {
            eprintln!("❌ SUI_ADDRESS or SUI_SECRET_KEY not set, but .env file exists");
            eprintln!("   Please check your .env file or set these environment variables");
            std::process::exit(1);
        }
    }

    // Resolve the package_id: use arg if provided, otherwise check environment variable
    let final_package_id = if let Some(pid) = package_id {
        // Package ID provided as argument
        pid
    } else {
        // Try to get from environment variable (may have been injected from config)
        match std::env::var("SILVANA_REGISTRY_PACKAGE") {
            Ok(pid) if !pid.is_empty() => pid,
            _ => {
                eprintln!("❌ Error: Registry package ID not provided");
                eprintln!("   Please provide it using one of these methods:");
                eprintln!("   1. Use --package-id argument");
                eprintln!("   2. Set SILVANA_REGISTRY_PACKAGE environment variable");
                eprintln!("   3. Ensure it's configured on the RPC server for chain: {}", chain);
                std::process::exit(1);
            }
        }
    };

    // Resolve the RPC URL using the helper from sui crate
    let rpc_url = sui::resolve_rpc_url(rpc_url, chain_override.clone())
        .map_err(CoordinatorError::Other)?;
    
    // Initialize logging with New Relic tracing layer
    monitoring::init_logging_with_newrelic()
        .await
        .map_err(CoordinatorError::Other)?;
    info!("✅ Logging initialized with New Relic support");

    // Initialize New Relic OpenTelemetry exporters
    if let Err(e) = monitoring::newrelic::init_newrelic().await {
        warn!("⚠️  Failed to initialize New Relic exporters: {}", e);
        warn!("   Continuing without New Relic metrics/traces");
    } else {
        info!("✅ New Relic exporters initialized");
    }

    // Initialize monitoring system
    monitoring::init_monitoring().map_err(CoordinatorError::Other)?;

    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    // Start the coordinator
    coordinator::start_coordinator(
        rpc_url,
        final_package_id,
        use_tee,
        container_timeout,
        grpc_socket_path,
        instance,
        settle,
    )
    .await
}