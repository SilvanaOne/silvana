mod agent;
mod block;
mod cli;
mod config;
mod constants;
mod coordinator;
mod docker;
mod error;
mod events;
mod example;
mod grpc;
mod hardware;
mod job_id;
mod job_lock;
mod job_searcher;
mod jobs;
mod jobs_cache;
mod merge;
mod metrics;
mod multicall;
mod processor;
mod proof;
mod proofs_storage;
mod session_id;
mod settlement;
mod state;
mod stuck_jobs;

use clap::Parser;
use dotenvy::dotenv;
use tracing::{error, info, warn};
use tracing_subscriber::prelude::*;

use crate::cli::{
    Cli, Commands,
};
use crate::error::{CoordinatorError, Result};
use anyhow::anyhow;
use std::collections::HashMap;

// Current version from Cargo.toml
const SILVANA_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Helper function to fetch config and inject environment variables for all commands
async fn fetch_and_inject_config(chain: &str) -> HashMap<String, String> {
    //println!("🔄 Fetching configuration for chain: {}", chain);
    
    match config::fetch_config(chain).await {
        Ok(config_map) => {
            // println!(
            //     "✅ Successfully fetched {} configuration items",
            //     config_map.len()
            // );

            // Check for new version
            if let Some(last_version) = config_map.get("LAST_VERSION") {
                check_version_update(last_version);
            }

            // Inject configuration as environment variables (without overriding existing ones)
            //let mut injected_count = 0;
            for (key, value) in config_map.iter() {
                // Check if the environment variable already exists
                if std::env::var(key).is_err() {
                    // Setting environment variables is safe in this context
                    // as we're doing it early in startup before any threads are spawned
                    unsafe {
                        std::env::set_var(key, value);
                    }
                    //injected_count += 1;
                    //println!("  ✓ Set env var: {}", key);
                } else {
                    //println!("  ⏩ Skipped (already set): {}", key);
                }
            }

            //println!("📋 Injected {} new environment variables", injected_count);
            config_map
        }
        Err(e) => {
            // Log the error but continue - config fetching is not critical
            eprintln!("⚠️  Failed to fetch configuration from RPC server: {}", e);
            eprintln!("   Continuing with local environment variables only");
            HashMap::new()
        }
    }
}

/// Check if a new version is available and print upgrade message
fn check_version_update(last_version: &str) {
    // Parse versions for comparison
    let current_parts: Vec<u32> = SILVANA_VERSION
        .split('.')
        .filter_map(|s| s.parse().ok())
        .collect();
    let last_parts: Vec<u32> = last_version
        .split('.')
        .filter_map(|s| s.parse().ok())
        .collect();

    // Compare versions (major, minor, patch)
    let mut newer_available = false;
    for i in 0..3 {
        let current = current_parts.get(i).unwrap_or(&0);
        let last = last_parts.get(i).unwrap_or(&0);
        
        if last > current {
            newer_available = true;
            break;
        } else if last < current {
            break;
        }
    }

    if newer_available {
        println!();
        println!("🆕 New version available!");
        println!("   Current version: {}", SILVANA_VERSION);
        println!("   Latest version:  {}", last_version);
        println!();
        println!("   Please upgrade to the latest version:");
        println!("   curl -sSL https://raw.githubusercontent.com/SilvanaOne/silvana/main/install.sh | bash");
        println!();
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    main_impl().await
}

async fn main_impl() -> Result<()> {
    // Load .env file from current directory
    dotenv().ok();

    let cli = Cli::parse();

    // Capture the chain value for use in command handlers
    let chain_override = cli.chain.clone();

    // Override chain if specified
    if let Some(chain) = &cli.chain {
        // Validate chain value
        match chain.to_lowercase().as_str() {
            "devnet" | "testnet" | "mainnet" => {
                // SAFETY: We're setting an environment variable early in main before any threads are spawned
                unsafe {
                    std::env::set_var("SUI_CHAIN", chain.to_lowercase());
                }
            }
            _ => {
                eprintln!(
                    "Error: Invalid chain '{}'. Must be one of: devnet, testnet, mainnet",
                    chain
                );
                std::process::exit(1);
            }
        }
    }

    // Determine the chain for config fetching
    let chain = chain_override
        .clone()
        .or_else(|| std::env::var("SUI_CHAIN").ok())
        .unwrap_or_else(|| "devnet".to_string());

    // Fetch and inject configuration for all commands
    let config_map = fetch_and_inject_config(&chain).await;

    // Process commands
    match cli.command {
        Commands::Start {
            rpc_url,
            package_id,
            use_tee,
            container_timeout,
            log_level: _,
            grpc_socket_path,
            instance,
            settle,
        } => {
            cli::start::handle_start_command(
                rpc_url,
                package_id,
                use_tee,
                container_timeout,
                grpc_socket_path,
                instance,
                settle,
                &chain,
                chain_override.clone(),
                &config_map,
            )
            .await
        }

        Commands::New { name, force } => {
            // Initialize minimal logging
            tracing_subscriber::registry()
                .with(tracing_subscriber::EnvFilter::new("info"))
                .with(tracing_subscriber::fmt::layer())
                .init();

            // Validate project name
            if name.trim().is_empty() {
                error!("Project name cannot be empty");
                return Err(anyhow::anyhow!("Project name cannot be empty").into());
            }

            // Check for invalid characters in project name
            if name.contains('/') || name.contains('\\') || name.contains("..") {
                error!("Invalid project name: {}", name);
                return Err(anyhow::anyhow!("Project name contains invalid characters").into());
            }

            let current_dir = std::env::current_dir()
                .map_err(|e| anyhow::anyhow!("Failed to get current directory: {}", e))?;
            let project_path = current_dir.join(&name);

            // Check if folder exists
            if project_path.exists() && !force {
                error!(
                    "Folder '{}' already exists. Use --force to overwrite.",
                    name
                );
                return Err(anyhow::anyhow!(
                    "Folder '{}' already exists. Use --force to overwrite.",
                    name
                )
                .into());
            }

            info!("🚀 Creating new Silvana project: {}", name);

            // Create the project directory
            if !project_path.exists() {
                std::fs::create_dir(&project_path)
                    .map_err(|e| anyhow::anyhow!("Failed to create project directory: {}", e))?;
            } else if force {
                warn!("⚠️  Overwriting existing folder: {}", name);
                // Clean the existing directory
                std::fs::remove_dir_all(&project_path)
                    .map_err(|e| anyhow::anyhow!("Failed to remove existing directory: {}", e))?;
                std::fs::create_dir(&project_path)
                    .map_err(|e| anyhow::anyhow!("Failed to recreate project directory: {}", e))?;
            }

            info!("📥 Downloading project template...");

            // Get RPC endpoint from environment or use default
            let rpc_endpoint = std::env::var("SILVANA_RPC_SERVER")
                .unwrap_or_else(|_| "https://rpc.silvana.dev".to_string());
            
            // Initialize RPC client
            let mut rpc_client = rpc_client::SilvanaRpcClient::new(
                rpc_client::RpcClientConfig::new(&rpc_endpoint)
            )
            .await
            .map_err(|e| anyhow::anyhow!("Failed to connect to RPC server: {}", e))?;
            
            // Fixed S3 key for the example archive
            let s3_key = "examples/add.tar.zst";
            
            // Download the template binary via RPC
            info!("   Fetching from: {}", s3_key);
            match rpc_client.read_binary(s3_key).await {
                Ok(response) if response.success => {
                    info!("✅ Template downloaded successfully!");
                    info!("   Archive size: {} bytes", response.data.len());
                    info!("   SHA256: {}", response.sha256);
                    
                    // Write the binary data to a temporary file
                    let temp_path = project_path.join(".download.tar.zst");
                    std::fs::write(&temp_path, &response.data)
                        .map_err(|e| anyhow::anyhow!("Failed to write temporary archive: {}", e))?;
                    
                    // Unpack the archive using the storage utility
                    match storage::unpack_local_archive(&temp_path, &project_path) {
                        Ok(_) => {
                            info!("✅ Template extracted successfully!");
                            // Clean up temporary file
                            let _ = std::fs::remove_file(&temp_path);
                        }
                        Err(e) => {
                            error!("Failed to extract template: {}", e);
                            // Clean up
                            let _ = std::fs::remove_file(&temp_path);
                            let _ = std::fs::remove_dir_all(&project_path);
                            return Err(anyhow::anyhow!("Failed to extract template: {}", e).into());
                        }
                    }

                    // Setup the example project (generate keys, fund accounts, etc.)
                    match example::setup_example_project(&project_path, &name).await {
                        Ok(_) => {
                            info!("");
                            info!("🎉 Project '{}' is ready!", name);
                            info!("");
                            info!("📁 Project structure:");
                            info!("   {}/", name);
                            info!("   ├── agent/     # TypeScript agent implementation");
                            info!("   ├── move/      # Move smart contracts");
                            info!("   └── silvana/   # Silvana coordinator configuration");
                            info!("");
                            info!("🚀 Next steps:");
                            info!("   cd {}", name);
                            info!("   cd agent && npm install    # Install agent dependencies");
                            info!("   cd move && sui move build  # Build Move contracts");
                            info!("");
                            info!("📖 Check the README file for more information.");
                        }
                        Err(e) => {
                            warn!("Failed to complete project setup: {}", e);
                            warn!("");
                            warn!("⚠️  Project template was downloaded but setup is incomplete.");
                            warn!("   Error: {}", e);
                            warn!("");
                            warn!("   You can manually set up keys and funding later.");
                            warn!("   Check the README file for instructions.");
                        }
                    }
                }
                Ok(response) => {
                    error!("Failed to download template: {}", response.message);
                    // Clean up the created directory
                    let _ = std::fs::remove_dir_all(&project_path);
                    return Err(anyhow::anyhow!("Failed to download template: {}", response.message).into());
                }
                Err(e) => {
                    error!("Failed to download template: {}", e);
                    // Clean up the created directory
                    let _ = std::fs::remove_dir_all(&project_path);
                    return Err(anyhow::anyhow!("Failed to download template: {}", e).into());
                }
            }

            Ok(())
        }

        Commands::Instance { rpc_url, instance } => {
            cli::instance::handle_instance_command(rpc_url, instance, chain_override.clone()).await
        }

        Commands::Object { rpc_url, object } => {
            cli::object::handle_object_command(rpc_url, object, chain_override.clone()).await
        }

        Commands::Block {
            rpc_url,
            instance,
            block,
        } => cli::block::handle_block_command(rpc_url, instance, block, chain_override).await,

        Commands::Proofs {
            rpc_url,
            instance,
            block,
        } => cli::proofs::handle_proofs_command(rpc_url, instance, block, chain_override).await,

        Commands::Job {
            rpc_url,
            instance,
            job,
            failed,
        } => {
            cli::jobs::handle_job_command(rpc_url, instance, job, failed, chain_override.clone()).await
        }

        Commands::Jobs {
            rpc_url,
            instance,
            failed,
        } => {
            cli::jobs::handle_jobs_command(rpc_url, instance, failed, chain_override.clone()).await
        }

        Commands::Transaction {
            rpc_url,
            private_key,
            tx_type,
        } => {
            cli::transaction::handle_transaction_command(
                rpc_url,
                private_key,
                tx_type,
                chain_override.clone(),
            )
            .await
        }

        Commands::Balance { subcommand } => {
            cli::balance::handle_balance_command(subcommand, chain_override).await
        }

        Commands::Split { rpc_url } => {
            // Initialize minimal logging
            tracing_subscriber::registry()
                .with(tracing_subscriber::EnvFilter::new("info"))
                .with(tracing_subscriber::fmt::layer())
                .init();

            // Resolve and initialize Sui connection
            let rpc_url = sui::resolve_rpc_url(rpc_url, chain_override.clone())
                .map_err(CoordinatorError::Other)?;
            sui::SharedSuiState::initialize(&rpc_url)
                .await
                .map_err(CoordinatorError::Other)?;

            println!("Checking gas coin pool and splitting if needed...");

            match sui::coin_management::ensure_gas_coin_pool().await {
                Ok(()) => {
                    println!("✅ Gas coin pool check complete");

                    // Show updated balance info
                    println!("\nUpdated balance:");
                    sui::print_balance_info(None)
                        .await
                        .map_err(CoordinatorError::Other)?;
                }
                Err(e) => {
                    error!("Failed to manage gas coin pool: {}", e);
                    return Err(anyhow::anyhow!("Failed to manage gas coin pool: {}", e).into());
                }
            }

            Ok(())
        }

        Commands::Network { rpc_url } => {
            // Initialize minimal logging
            tracing_subscriber::registry()
                .with(tracing_subscriber::EnvFilter::new("info"))
                .with(tracing_subscriber::fmt::layer())
                .init();

            // Resolve and initialize Sui connection
            let rpc_url = sui::resolve_rpc_url(rpc_url, chain_override.clone())
                .map_err(CoordinatorError::Other)?;
            sui::SharedSuiState::initialize(&rpc_url)
                .await
                .map_err(CoordinatorError::Other)?;

            let network_name = sui::get_network_name();
            let address = sui::get_current_address();

            println!("🌐 Network: {}", network_name);
            println!("👤 Address: {}", address);

            // Print detailed network info
            match sui::print_network_info().await {
                Ok(()) => {}
                Err(e) => {
                    error!("Failed to fetch network info: {}", e);
                    return Err(anyhow::anyhow!("Failed to fetch network info: {}", e).into());
                }
            }

            Ok(())
        }

        Commands::Config { endpoint, json } => {
            // Initialize minimal logging
            tracing_subscriber::registry()
                .with(tracing_subscriber::EnvFilter::new("info"))
                .with(tracing_subscriber::fmt::layer())
                .init();

            // Determine which chain to fetch config for
            let chain = chain_override
                .or_else(|| std::env::var("SUI_CHAIN").ok())
                .unwrap_or_else(|| "testnet".to_string());

            println!("📥 Fetching configuration for chain: {}", chain);

            // Fetch configuration
            let config_map = if let Some(endpoint) = endpoint.as_ref() {
                config::fetch_config_with_endpoint(&chain, Some(endpoint)).await
            } else {
                config::fetch_config(&chain).await
            };

            match config_map {
                Ok(config) => {
                    if json {
                        // Output as JSON
                        match serde_json::to_string_pretty(&config) {
                            Ok(json_str) => println!("{}", json_str),
                            Err(e) => {
                                error!("Failed to serialize config to JSON: {}", e);
                                return Err(anyhow::anyhow!(
                                    "Failed to serialize config to JSON: {}",
                                    e
                                )
                                .into());
                            }
                        }
                    } else {
                        // Use the display function from config module
                        config::fetch_and_display_config(&chain)
                            .await
                            .map_err(|e| anyhow::anyhow!("Failed to display config: {}", e))?;
                    }
                    Ok(())
                }
                Err(e) => {
                    error!("Failed to fetch configuration: {}", e);
                    Err(anyhow::anyhow!("Failed to fetch configuration: {}", e).into())
                }
            }
        }

        Commands::Faucet { subcommand } => cli::faucet::handle_faucet_command(subcommand).await,
        Commands::Keypair { subcommand } => cli::keypair::handle_keypair_command(subcommand).await,

        Commands::Avs { subcommand } => {
            // Initialize tracing for AVS commands
            tracing_subscriber::registry()
                .with(tracing_subscriber::fmt::layer())
                .with(tracing_subscriber::EnvFilter::from_default_env())
                .init();

            // Execute the AVS command
            Ok(avs_operator::cli::execute_avs_command(subcommand).await?)
        }

        Commands::Registry {
            rpc_url,
            subcommand,
        } => cli::registry::handle_registry_command(rpc_url, subcommand, chain_override).await,

        Commands::Secrets { subcommand } => cli::secrets::handle_secrets_command(subcommand).await
    }
}
