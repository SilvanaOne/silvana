use crate::coordinator;
use crate::error::{CoordinatorError, Result};
use crate::example;
use anyhow::anyhow;
use tracing::{info, warn};

pub async fn handle_start_command(
    rpc_url: Option<String>,
    package_id: Option<String>,
    use_tee: bool,
    container_timeout: u64,
    grpc_socket_path: String,
    instance: Option<String>,
    settle: bool,
    config: Option<String>,
    chain: &str,
    chain_override: Option<String>,
) -> Result<()> {
    // Config already fetched and injected above
    // Check if credentials are set, generate if not
    let env_file_path = std::path::Path::new(".env");
    let sui_missing =
        std::env::var("SUI_ADDRESS").is_err() || std::env::var("SUI_SECRET_KEY").is_err();
    let eth_missing = std::env::var("ETHEREUM_ADDRESS").is_err()
        || std::env::var("ETHEREUM_PRIVATE_KEY").is_err();

    if sui_missing {
        // Check if .env file exists
        if !env_file_path.exists() {
            println!("🔑 Credentials not found, generating new keypairs...");

            // Generate Sui keypair if missing
            let sui_keypair = if sui_missing {
                let keypair = example::generate_sui_keypair("coordinator")
                    .map_err(|e| anyhow!("Failed to generate Sui keypair: {}", e))?;
                println!("✅ Generated new Sui keypair:");
                println!("   Address: {}", keypair.address);
                Some(keypair)
            } else {
                None
            };

            // Generate Ethereum keypair if missing
            let eth_keypair = if eth_missing {
                let keypair = ethereum::keypair::generate_ethereum_keypair()
                    .map_err(|e| anyhow!("Failed to generate Ethereum keypair: {}", e))?;
                println!("✅ Generated new Ethereum keypair:");
                println!("   Address: {}", keypair.address);
                Some(keypair)
            } else {
                None
            };

            // Set the environment variables for the current session
            unsafe {
                if let Some(ref sui) = sui_keypair {
                    std::env::set_var("SUI_ADDRESS", &sui.address);
                    std::env::set_var("SUI_SECRET_KEY", &sui.private_key);
                }
                if let Some(ref eth) = eth_keypair {
                    std::env::set_var("ETHEREUM_ADDRESS", &eth.address);
                    std::env::set_var("ETHEREUM_PRIVATE_KEY", &eth.private_key);
                }
            }

            // Build .env content
            let mut env_lines = Vec::new();
            env_lines.push("# Auto-generated credentials".to_string());

            if let Some(ref sui) = sui_keypair {
                env_lines.push(format!("SUI_ADDRESS={}", sui.address));
                env_lines.push(format!("SUI_SECRET_KEY={}", sui.private_key));
                env_lines.push(format!("SUI_CHAIN={}", chain));
            }

            if let Some(ref eth) = eth_keypair {
                env_lines.push(format!("ETHEREUM_ADDRESS={}", eth.address));
                env_lines.push(format!("ETHEREUM_PRIVATE_KEY={}", eth.private_key));
            }

            let env_content = env_lines.join("\n") + "\n";

            std::fs::write(env_file_path, env_content)
                .map_err(|e| anyhow!("Failed to write .env file: {}", e))?;

            println!("📝 Saved credentials to .env file");

            // Auto-fund on devnet
            if chain == "devnet" {
                if let Some(ref sui) = sui_keypair {
                    println!("💰 Requesting SUI funds from devnet faucet...");
                    match sui::request_tokens_from_faucet("devnet", &sui.address, Some(10.0)).await
                    {
                        Ok(tx_digest) => {
                            println!("✅ SUI faucet request successful!");
                            if tx_digest != "unknown" {
                                println!("   Transaction: {}", tx_digest);
                                println!(
                                    "   🔗 Explorer: https://suiscan.xyz/devnet/tx/{}",
                                    tx_digest
                                );
                            } else {
                                println!(
                                    "   🔗 Explorer: https://suiscan.xyz/devnet/account/{}",
                                    sui.address
                                );
                            }
                            println!("   Waiting for transaction to be processed...");
                            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                        }
                        Err(e) => {
                            eprintln!("⚠️  Failed to request SUI funds from faucet: {}", e);
                            eprintln!("   Please manually fund the address:");
                            eprintln!(
                                "   silvana faucet sui --address {} --amount 10",
                                sui.address
                            );
                        }
                    }
                }
            } else {
                if let Some(ref sui) = sui_keypair {
                    println!("⚠️  Please fund this SUI address before running operations:");
                    println!("   silvana faucet sui --address {}", sui.address);
                }
            }
        } else {
            // .env file exists but some credentials are missing
            let mut missing = Vec::new();
            if std::env::var("SUI_ADDRESS").is_err() {
                missing.push("SUI_ADDRESS");
            }
            if std::env::var("SUI_SECRET_KEY").is_err() {
                missing.push("SUI_SECRET_KEY");
            }
            if std::env::var("ETHEREUM_ADDRESS").is_err() {
                missing.push("ETHEREUM_ADDRESS");
            }
            if std::env::var("ETHEREUM_PRIVATE_KEY").is_err() {
                missing.push("ETHEREUM_PRIVATE_KEY");
            }

            eprintln!("❌ Missing environment variables, but .env file exists");
            eprintln!("   Missing: {}", missing.join(", "));
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
                eprintln!(
                    "   3. Ensure it's configured on the RPC server for chain: {}",
                    chain
                );
                std::process::exit(1);
            }
        }
    };

    // Resolve the RPC URL using the helper from sui crate
    let rpc_url =
        sui::resolve_rpc_url(rpc_url, chain_override.clone()).map_err(CoordinatorError::Other)?;

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
        config,
    )
    .await
}
