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
    Cli, Commands, KeypairCommands, RegistryCommands,
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
        } => {
            // Initialize minimal logging
            tracing_subscriber::registry()
                .with(tracing_subscriber::EnvFilter::new("warn"))
                .with(tracing_subscriber::fmt::layer())
                .init();

            // Resolve and initialize Sui connection (read-only mode)
            let rpc_url = sui::resolve_rpc_url(rpc_url, chain_override.clone())
                .map_err(CoordinatorError::Other)?;
            sui::SharedSuiState::initialize_read_only(&rpc_url)
                .await
                .map_err(CoordinatorError::Other)?;

            // Fetch the app instance first
            let app_instance = match sui::fetch::fetch_app_instance(&instance).await {
                Ok(instance) => instance,
                Err(e) => {
                    error!("Failed to fetch app instance {}: {}", instance, e);
                    return Err(anyhow::anyhow!("Failed to fetch app instance: {}", e).into());
                }
            };

            // Fetch and display the proof calculation
            match sui::fetch::fetch_proof_calculation(&app_instance, block).await {
                Ok(Some(proof_calc)) => {
                    println!("ProofCalculation {{");
                    println!("    id: \"{}\",", proof_calc.id);
                    println!("    block_number: {},", proof_calc.block_number);
                    println!("    start_sequence: {},", proof_calc.start_sequence);

                    if let Some(end) = proof_calc.end_sequence {
                        println!("    end_sequence: Some({}),", end);
                    } else {
                        println!("    end_sequence: None,");
                    }

                    println!("    proofs: [");

                    // Sort proofs by first sequence number
                    let mut sorted_proofs = proof_calc.proofs.clone();
                    sorted_proofs.sort_by_key(|p| p.sequences.first().cloned().unwrap_or(0));

                    for proof in &sorted_proofs {
                        println!("        Proof {{");

                        // Format status
                        let status_str = match proof.status {
                            sui::fetch::ProofStatus::Used => "Used",
                            sui::fetch::ProofStatus::Calculated => "Calculated",
                            sui::fetch::ProofStatus::Reserved => "Reserved",
                            sui::fetch::ProofStatus::Started => "Started",
                            sui::fetch::ProofStatus::Rejected => "Rejected",
                        };
                        println!("            status: {},", status_str);

                        // Format DA hash
                        if let Some(ref hash) = proof.da_hash {
                            println!("            da_hash: Some(\"{}\"),", hash);
                        } else {
                            println!("            da_hash: None,");
                        }

                        // Format sequence1 and sequence2 for merge proofs
                        if let Some(ref seq1) = proof.sequence1 {
                            println!("            sequence1: Some({:?}),", seq1);
                        } else {
                            println!("            sequence1: None,");
                        }

                        if let Some(ref seq2) = proof.sequence2 {
                            println!("            sequence2: Some({:?}),", seq2);
                        } else {
                            println!("            sequence2: None,");
                        }

                        println!("            rejected_count: {},", proof.rejected_count);
                        println!("            timestamp: {},", proof.timestamp);
                        println!("            prover: \"{}\",", proof.prover);

                        if let Some(ref user) = proof.user {
                            println!("            user: Some(\"{}\"),", user);
                        } else {
                            println!("            user: None,");
                        }

                        println!("            job_id: \"{}\",", proof.job_id);
                        println!("            sequences: {:?},", proof.sequences);
                        println!("        }},");
                    }

                    println!("    ],");

                    if let Some(ref block_proof) = proof_calc.block_proof {
                        println!("    block_proof: Some(\"{}\"),", block_proof);
                    } else {
                        println!("    block_proof: None,");
                    }

                    println!("    is_finished: {},", proof_calc.is_finished);
                    println!("}}");
                }
                Ok(None) => {
                    println!("No proof calculation found for block {}", block);
                }
                Err(e) => {
                    error!(
                        "Failed to fetch proof calculation for block {}: {}",
                        block, e
                    );
                    return Err(anyhow::anyhow!("Failed to fetch proof calculation: {}", e).into());
                }
            }

            Ok(())
        }

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
        Commands::Keypair { subcommand } => {
            // Initialize minimal logging
            tracing_subscriber::registry()
                .with(tracing_subscriber::fmt::layer())
                .with(tracing_subscriber::EnvFilter::from_default_env())
                .init();

            match subcommand {
                KeypairCommands::Sui => {
                    println!("🔑 Generating new Sui Ed25519 keypair...\n");

                    match sui::keypair::generate_ed25519() {
                        Ok(keypair) => {
                            println!("✅ Sui Keypair Generated Successfully!\n");
                            println!(
                                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
                            );
                            println!("🔐 PRIVATE KEY (Keep this secret!):");
                            println!("   {}", keypair.sui_private_key);
                            println!();
                            println!("📍 ADDRESS:");
                            println!("   {}", keypair.address);
                            println!(
                                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
                            );
                            println!();
                            println!("⚠️  IMPORTANT:");
                            println!("   • Save your private key in a secure location");
                            println!("   • Never share your private key with anyone");
                            println!("   • You will need this key to sign transactions");
                            println!();
                            println!("💡 To use this keypair:");
                            println!("   export SUI_SECRET_KEY={}", keypair.sui_private_key);
                            println!("   export SUI_ADDRESS={}", keypair.address);
                        }
                        Err(e) => {
                            eprintln!("❌ Failed to generate keypair: {}", e);
                            return Err(anyhow::anyhow!("Keypair generation failed: {}", e).into());
                        }
                    }
                }

                KeypairCommands::Mina => {
                    println!("🔑 Generating new Mina keypair...");
                    println!();

                    match mina::generate_mina_keypair() {
                        Ok(keypair) => {
                            println!("✅ Mina Keypair Generated Successfully!");
                            println!();
                            println!(
                                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
                            );
                            println!("🔐 PRIVATE KEY (Keep this secret!):");
                            println!("   {}", keypair.private_key);
                            println!();
                            println!("📍 PUBLIC KEY (Address):");
                            println!("   {}", keypair.public_key);
                            println!(
                                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
                            );
                            println!();
                            println!("⚠️  IMPORTANT:");
                            println!("   • Save your private key in a secure location");
                            println!("   • Never share your private key with anyone");
                            println!("   • You will need this key to sign transactions");
                            println!();
                            println!("💡 To use this keypair:");
                            println!("   export MINA_PRIVATE_KEY={}", keypair.private_key);
                            println!("   export MINA_PUBLIC_KEY={}", keypair.public_key);
                        }
                        Err(e) => {
                            eprintln!("❌ Failed to generate Mina keypair: {}", e);
                            return Err(
                                anyhow::anyhow!("Mina keypair generation failed: {}", e).into()
                            );
                        }
                    }
                }

                KeypairCommands::Ethereum => {
                    println!("🔑 Generating new Ethereum keypair...");
                    println!();

                    match ethereum::generate_ethereum_keypair() {
                        Ok(keypair) => {
                            println!("✅ Ethereum Keypair Generated Successfully!");
                            println!();
                            println!(
                                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
                            );
                            println!("🔐 PRIVATE KEY (Keep this secret!):");
                            println!("   {}", keypair.private_key);
                            println!();
                            println!("📍 ADDRESS:");
                            println!("   {}", keypair.address);
                            println!(
                                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
                            );
                            println!();
                            println!("⚠️  IMPORTANT:");
                            println!("   • Save your private key in a secure location");
                            println!("   • Never share your private key with anyone");
                            println!("   • You will need this key to sign transactions");
                            println!();
                            println!("💡 To use this keypair:");
                            println!("   export ETH_PRIVATE_KEY={}", keypair.private_key);
                            println!("   export ETH_ADDRESS={}", keypair.address);
                        }
                        Err(e) => {
                            eprintln!("❌ Failed to generate Ethereum keypair: {}", e);
                            return Err(anyhow::anyhow!(
                                "Ethereum keypair generation failed: {}",
                                e
                            )
                            .into());
                        }
                    }
                }
            }

            Ok(())
        }

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
        } => {
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

            // Create interface
            let mut interface = sui::SilvanaSuiInterface::new();

            match subcommand {
                RegistryCommands::Create { name, package_id } => {
                    println!("📝 Creating new Silvana registry...\n");
                    println!("   Name: {}", name);
                    if let Some(ref pkg) = package_id {
                        println!("   Package: {}", pkg);
                    }
                    println!();

                    match interface
                        .create_silvana_registry(name.clone(), package_id)
                        .await
                    {
                        Ok(result) => {
                            println!("✅ Registry created successfully!\n");
                            println!("   Registry ID: {}", result.registry_id);
                            println!("   Transaction: {}", result.tx_digest);
                            println!();
                            println!("💡 To use this registry, set the environment variable:");
                            println!("   export SILVANA_REGISTRY={}", result.registry_id);
                        }
                        Err(e) => {
                            eprintln!("❌ Failed to create registry: {}", e);
                            return Err(anyhow!(e).into());
                        }
                    }
                }

                RegistryCommands::AddDeveloper {
                    registry,
                    name,
                    github,
                    image,
                    description,
                    site,
                } => {
                    let registry_id = registry.ok_or_else(|| {
                        anyhow!("Registry ID not provided. Set SILVANA_REGISTRY environment variable or use --registry")
                    })?;

                    println!("👤 Adding developer to registry...\n");
                    println!("   Registry: {}", registry_id);
                    println!("   Name: {}", name);
                    println!("   GitHub: {}", github);
                    if let Some(ref img) = image {
                        println!("   Image: {}", img);
                    }
                    if let Some(ref desc) = description {
                        println!("   Description: {}", desc);
                    }
                    if let Some(ref s) = site {
                        println!("   Site: {}", s);
                    }
                    println!();

                    match interface
                        .add_developer_to_registry(
                            &registry_id,
                            name,
                            github,
                            image,
                            description,
                            site,
                        )
                        .await
                    {
                        Ok(tx_digest) => {
                            println!("✅ Developer added successfully!");
                            println!("   Transaction: {}", tx_digest);
                        }
                        Err(e) => {
                            eprintln!("❌ Failed to add developer: {}", e);
                            return Err(anyhow!(e).into());
                        }
                    }
                }

                RegistryCommands::UpdateDeveloper {
                    registry,
                    name,
                    github,
                    image,
                    description,
                    site,
                } => {
                    let registry_id = registry.ok_or_else(|| {
                        anyhow!("Registry ID not provided. Set SILVANA_REGISTRY environment variable or use --registry")
                    })?;

                    println!("📝 Updating developer in registry...\n");
                    println!("   Registry: {}", registry_id);
                    println!("   Name: {}", name);
                    println!("   GitHub: {}", github);
                    println!();

                    match interface
                        .update_developer_in_registry(
                            &registry_id,
                            name,
                            github,
                            image,
                            description,
                            site,
                        )
                        .await
                    {
                        Ok(tx_digest) => {
                            println!("✅ Developer updated successfully!");
                            println!("   Transaction: {}", tx_digest);
                        }
                        Err(e) => {
                            eprintln!("❌ Failed to update developer: {}", e);
                            return Err(anyhow!(e).into());
                        }
                    }
                }

                RegistryCommands::RemoveDeveloper {
                    registry,
                    name,
                    agents,
                } => {
                    let registry_id = registry.ok_or_else(|| {
                        anyhow!("Registry ID not provided. Set SILVANA_REGISTRY environment variable or use --registry")
                    })?;

                    println!("🗑️  Removing developer from registry...\n");
                    println!("   Registry: {}", registry_id);
                    println!("   Name: {}", name);
                    if !agents.is_empty() {
                        println!("   Agents to remove: {}", agents.join(", "));
                    }
                    println!();

                    match interface
                        .remove_developer_from_registry(&registry_id, name, agents)
                        .await
                    {
                        Ok(tx_digest) => {
                            println!("✅ Developer removed successfully!");
                            println!("   Transaction: {}", tx_digest);
                        }
                        Err(e) => {
                            eprintln!("❌ Failed to remove developer: {}", e);
                            return Err(anyhow!(e).into());
                        }
                    }
                }

                RegistryCommands::AddAgent {
                    registry,
                    developer,
                    name,
                    image,
                    description,
                    site,
                    chains,
                } => {
                    let registry_id = registry.ok_or_else(|| {
                        anyhow!("Registry ID not provided. Set SILVANA_REGISTRY environment variable or use --registry")
                    })?;

                    println!("🤖 Adding agent to developer...\n");
                    println!("   Registry: {}", registry_id);
                    println!("   Developer: {}", developer);
                    println!("   Agent: {}", name);
                    if !chains.is_empty() {
                        println!("   Chains: {}", chains.join(", "));
                    }
                    if let Some(ref img) = image {
                        println!("   Image: {}", img);
                    }
                    if let Some(ref desc) = description {
                        println!("   Description: {}", desc);
                    }
                    if let Some(ref s) = site {
                        println!("   Site: {}", s);
                    }
                    println!();

                    match interface
                        .add_agent_to_developer(
                            &registry_id,
                            developer,
                            name,
                            image,
                            description,
                            site,
                            chains,
                        )
                        .await
                    {
                        Ok(tx_digest) => {
                            println!("✅ Agent added successfully!");
                            println!("   Transaction: {}", tx_digest);
                        }
                        Err(e) => {
                            eprintln!("❌ Failed to add agent: {}", e);
                            return Err(anyhow!(e).into());
                        }
                    }
                }

                RegistryCommands::UpdateAgent {
                    registry,
                    developer,
                    name,
                    image,
                    description,
                    site,
                    chains,
                } => {
                    let registry_id = registry.ok_or_else(|| {
                        anyhow!("Registry ID not provided. Set SILVANA_REGISTRY environment variable or use --registry")
                    })?;

                    println!("📝 Updating agent...\n");
                    println!("   Registry: {}", registry_id);
                    println!("   Developer: {}", developer);
                    println!("   Agent: {}", name);
                    if !chains.is_empty() {
                        println!("   Chains: {}", chains.join(", "));
                    }
                    println!();

                    match interface
                        .update_agent_in_registry(
                            &registry_id,
                            developer,
                            name,
                            image,
                            description,
                            site,
                            chains,
                        )
                        .await
                    {
                        Ok(tx_digest) => {
                            println!("✅ Agent updated successfully!");
                            println!("   Transaction: {}", tx_digest);
                        }
                        Err(e) => {
                            eprintln!("❌ Failed to update agent: {}", e);
                            return Err(anyhow!(e).into());
                        }
                    }
                }

                RegistryCommands::RemoveAgent {
                    registry,
                    developer,
                    name,
                } => {
                    let registry_id = registry.ok_or_else(|| {
                        anyhow!("Registry ID not provided. Set SILVANA_REGISTRY environment variable or use --registry")
                    })?;

                    println!("🗑️  Removing agent from developer...\n");
                    println!("   Registry: {}", registry_id);
                    println!("   Developer: {}", developer);
                    println!("   Agent: {}", name);
                    println!();

                    match interface
                        .remove_agent_from_developer(&registry_id, developer, name)
                        .await
                    {
                        Ok(tx_digest) => {
                            println!("✅ Agent removed successfully!");
                            println!("   Transaction: {}", tx_digest);
                        }
                        Err(e) => {
                            eprintln!("❌ Failed to remove agent: {}", e);
                            return Err(anyhow!(e).into());
                        }
                    }
                }

                RegistryCommands::AddApp {
                    registry,
                    name,
                    description,
                } => {
                    let registry_id = registry.ok_or_else(|| {
                        anyhow!("Registry ID not provided. Set SILVANA_REGISTRY environment variable or use --registry")
                    })?;

                    println!("📱 Adding app to registry...\n");
                    println!("   Registry: {}", registry_id);
                    println!("   Name: {}", name);
                    if let Some(ref desc) = description {
                        println!("   Description: {}", desc);
                    }
                    println!();

                    match interface
                        .add_app_to_registry(&registry_id, name, description)
                        .await
                    {
                        Ok(tx_digest) => {
                            println!("✅ App added successfully!");
                            println!("   Transaction: {}", tx_digest);
                        }
                        Err(e) => {
                            eprintln!("❌ Failed to add app: {}", e);
                            return Err(anyhow!(e).into());
                        }
                    }
                }

                RegistryCommands::UpdateApp {
                    registry,
                    name,
                    description,
                } => {
                    let registry_id = registry.ok_or_else(|| {
                        anyhow!("Registry ID not provided. Set SILVANA_REGISTRY environment variable or use --registry")
                    })?;

                    println!("📝 Updating app in registry...\n");
                    println!("   Registry: {}", registry_id);
                    println!("   Name: {}", name);
                    if let Some(ref desc) = description {
                        println!("   Description: {}", desc);
                    }
                    println!();

                    match interface
                        .update_app_in_registry(&registry_id, name, description)
                        .await
                    {
                        Ok(tx_digest) => {
                            println!("✅ App updated successfully!");
                            println!("   Transaction: {}", tx_digest);
                        }
                        Err(e) => {
                            eprintln!("❌ Failed to update app: {}", e);
                            return Err(anyhow!(e).into());
                        }
                    }
                }

                RegistryCommands::RemoveApp { registry, name } => {
                    let registry_id = registry.ok_or_else(|| {
                        anyhow!("Registry ID not provided. Set SILVANA_REGISTRY environment variable or use --registry")
                    })?;

                    println!("🗑️  Removing app from registry...\n");
                    println!("   Registry: {}", registry_id);
                    println!("   Name: {}", name);
                    println!();

                    match interface.remove_app_from_registry(&registry_id, name).await {
                        Ok(tx_digest) => {
                            println!("✅ App removed successfully!");
                            println!("   Transaction: {}", tx_digest);
                        }
                        Err(e) => {
                            eprintln!("❌ Failed to remove app: {}", e);
                            return Err(anyhow!(e).into());
                        }
                    }
                }
            }

            Ok(())
        }

        Commands::Secrets { subcommand } => cli::secrets::handle_secrets_command(subcommand).await
    }
}
