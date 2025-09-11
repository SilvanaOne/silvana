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

use chrono::{DateTime, Utc};
use clap::Parser;
use dotenvy::dotenv;
use tracing::{error, info, warn};
use tracing_subscriber::prelude::*;

use crate::cli::{
    BalanceCommands, Cli, Commands, FaucetCommands, KeypairCommands, RegistryCommands,
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

            // Fetch and display the app instance
            match sui::fetch::fetch_app_instance(&instance).await {
                Ok(app_instance) => {
                    // Convert timestamps to ISO format
                    let previous_block_timestamp_iso = DateTime::<Utc>::from_timestamp_millis(
                        app_instance.previous_block_timestamp as i64,
                    )
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_else(|| app_instance.previous_block_timestamp.to_string());

                    let created_at_iso =
                        DateTime::<Utc>::from_timestamp_millis(app_instance.created_at as i64)
                            .map(|dt| dt.to_rfc3339())
                            .unwrap_or_else(|| app_instance.created_at.to_string());

                    // Print formatted app instance
                    println!("AppInstance {{");
                    println!("    id: \"{}\",", app_instance.id);
                    println!(
                        "    silvana_app_name: \"{}\",",
                        app_instance.silvana_app_name
                    );

                    if let Some(ref desc) = app_instance.description {
                        println!("    description: Some(\"{}\"),", desc);
                    } else {
                        println!("    description: None,");
                    }

                    // Print metadata
                    if !app_instance.metadata.is_empty() {
                        println!("    metadata: {{");
                        for (key, value) in &app_instance.metadata {
                            println!("        \"{}\": \"{}\",", key, value);
                        }
                        println!("    }},");
                    } else {
                        println!("    metadata: {{}},");
                    }

                    // Print kv
                    if !app_instance.kv.is_empty() {
                        println!("    kv: {{");
                        for (key, value) in &app_instance.kv {
                            println!("        \"{}\": \"{}\",", key, value);
                        }
                        println!("    }},");
                    } else {
                        println!("    kv: {{}},");
                    }

                    // Print methods (as JSON)
                    println!(
                        "    methods: {},",
                        serde_json::to_string(&app_instance.methods)
                            .unwrap_or_else(|_| "{}".to_string())
                    );

                    // Print state (as JSON)
                    println!(
                        "    state: {},",
                        serde_json::to_string(&app_instance.state)
                            .unwrap_or_else(|_| "{}".to_string())
                    );

                    println!("    blocks_table_id: \"{}\",", app_instance.blocks_table_id);
                    println!(
                        "    proof_calculations_table_id: \"{}\",",
                        app_instance.proof_calculations_table_id
                    );

                    // Print sequence_state_manager (as JSON)
                    println!(
                        "    sequence_state_manager: {},",
                        serde_json::to_string(&app_instance.sequence_state_manager)
                            .unwrap_or_else(|_| "{}".to_string())
                    );

                    // Print jobs if present
                    if let Some(ref jobs) = app_instance.jobs {
                        println!("    jobs: Some(Jobs {{");
                        println!("        id: \"{}\",", jobs.id);
                        println!("        jobs_table_id: \"{}\",", jobs.jobs_table_id);
                        println!(
                            "        total_jobs_count: {}, // Total jobs in ObjectTable (pending + running + failed)",
                            jobs.total_jobs_count
                        );
                        // Note: failed_jobs_table_id no longer exists - failed jobs are in main jobs table
                        println!("        failed_jobs_count: {},", jobs.failed_jobs_count);
                        println!("        failed_jobs_index: {:?},", jobs.failed_jobs_index);
                        println!("        pending_jobs: {:?},", jobs.pending_jobs);
                        println!("        pending_jobs_count: {},", jobs.pending_jobs_count);

                        // Calculate and display running jobs count
                        let running_jobs_count = jobs
                            .total_jobs_count
                            .saturating_sub(jobs.pending_jobs_count + jobs.failed_jobs_count);
                        println!(
                            "        running_jobs_count: {}, // Calculated: total - pending - failed",
                            running_jobs_count
                        );

                        // Display pending_jobs_indexes in a readable format
                        println!("        pending_jobs_indexes: {{");
                        for (developer, agents) in &jobs.pending_jobs_indexes {
                            println!("            \"{}\": {{", developer);
                            for (agent, methods) in agents {
                                println!("                \"{}\": {{", agent);
                                for (method, job_ids) in methods {
                                    println!("                    \"{}\": {:?},", method, job_ids);
                                }
                                println!("                }},");
                            }
                            println!("            }},");
                        }
                        println!("        }},");

                        println!("        next_job_sequence: {},", jobs.next_job_sequence);
                        println!("        max_attempts: {},", jobs.max_attempts);
                        // Settlement jobs are now per-chain in settlements
                        println!("    }}),");
                    } else {
                        println!("    jobs: None,");
                    }

                    println!("    sequence: {},", app_instance.sequence);
                    println!("    admin: \"{}\",", app_instance.admin);
                    println!("    block_number: {},", app_instance.block_number);
                    println!(
                        "    previous_block_timestamp: \"{}\",",
                        previous_block_timestamp_iso
                    );
                    println!(
                        "    previous_block_last_sequence: {},",
                        app_instance.previous_block_last_sequence
                    );
                    println!(
                        "    previous_block_actions_state: \"{}\",",
                        app_instance.previous_block_actions_state
                    );
                    println!(
                        "    last_proved_block_number: {},",
                        app_instance.last_proved_block_number
                    );
                    println!(
                        "    last_settled_block_number: {},",
                        app_instance.last_settled_block_number
                    );
                    // Print settlement data for every chain
                    println!("    settlements: {{");
                    for (chain, settlement) in &app_instance.settlements {
                        println!("        \"{}\": Settlement {{", chain);
                        println!("            chain: \"{}\",", settlement.chain);
                        println!(
                            "            last_settled_block_number: {},",
                            settlement.last_settled_block_number
                        );
                        if let Some(ref addr) = settlement.settlement_address {
                            println!("            settlement_address: Some(\"{}\"),", addr);
                        } else {
                            println!("            settlement_address: None,");
                        }
                        if let Some(job_id) = settlement.settlement_job {
                            println!("            settlement_job: Some({}),", job_id);
                        } else {
                            println!("            settlement_job: None,");
                        }
                        println!(
                            "            block_settlements_table_id: {:?},",
                            settlement.block_settlements_table_id
                        );
                        println!("        }},");
                    }
                    println!("    }},");

                    // Print settlement chains
                    if !app_instance.settlements.is_empty() {
                        println!("    settlement_chains: {{");
                        for (chain, settlement) in &app_instance.settlements {
                            println!("        \"{}\": {{", chain);
                            println!(
                                "            last_settled_block_number: {},",
                                settlement.last_settled_block_number
                            );
                            if let Some(ref addr) = settlement.settlement_address {
                                println!("            settlement_address: Some(\"{}\"),", addr);
                            } else {
                                println!("            settlement_address: None,");
                            }
                            println!("        }},");
                        }
                        println!("    }},");
                    } else {
                        println!("    settlement_chains: None,");
                    }

                    println!("    created_at: \"{}\",", created_at_iso);
                    println!("}}");
                }
                Err(e) => {
                    error!("Failed to fetch app instance {}: {}", instance, e);
                    return Err(anyhow::anyhow!("Failed to fetch app instance: {}", e).into());
                }
            }

            Ok(())
        }

        Commands::Object { rpc_url, object } => {
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

            // Fetch and display the raw object
            match sui::fetch::fetch_object(&object).await {
                Ok(json_value) => {
                    // Pretty print the JSON
                    let pretty_json =
                        serde_json::to_string_pretty(&json_value).map_err(|e| anyhow!(e))?;
                    println!("{}", pretty_json);
                }
                Err(e) => {
                    error!("Failed to fetch object {}: {}", object, e);
                    return Err(anyhow!(e).into());
                }
            }

            Ok(())
        }

        Commands::Block {
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

            // Fetch and display the block
            match sui::fetch::fetch_block_info(&app_instance, block).await {
                Ok(Some(block_info)) => {
                    // Convert commitments to hex
                    let actions_commitment_hex = hex::encode(&block_info.actions_commitment);
                    let state_commitment_hex = hex::encode(&block_info.state_commitment);
                    let start_actions_commitment_hex =
                        hex::encode(&block_info.start_actions_commitment);
                    let end_actions_commitment_hex =
                        hex::encode(&block_info.end_actions_commitment);

                    // Convert timestamps to ISO format
                    let created_iso =
                        DateTime::<Utc>::from_timestamp_millis(block_info.created_at as i64)
                            .map(|dt| dt.to_rfc3339())
                            .unwrap_or_else(|| block_info.created_at.to_string());

                    let state_calculated_iso = block_info
                        .state_calculated_at
                        .and_then(|ts| DateTime::<Utc>::from_timestamp_millis(ts as i64))
                        .map(|dt| dt.to_rfc3339());

                    let proved_at_iso = block_info
                        .proved_at
                        .and_then(|ts| DateTime::<Utc>::from_timestamp_millis(ts as i64))
                        .map(|dt| dt.to_rfc3339());

                    // Print formatted block
                    println!("Block {{");
                    println!("    name: \"{}\",", block_info.name);
                    println!("    block_number: {},", block_info.block_number);
                    println!("    start_sequence: {},", block_info.start_sequence);
                    println!("    end_sequence: {},", block_info.end_sequence);
                    println!("    actions_commitment: \"{}\",", actions_commitment_hex);
                    println!("    state_commitment: \"{}\",", state_commitment_hex);
                    println!(
                        "    time_since_last_block: {},",
                        block_info.time_since_last_block
                    );
                    println!(
                        "    number_of_transactions: {},",
                        block_info.number_of_transactions
                    );
                    println!(
                        "    start_actions_commitment: \"{}\",",
                        start_actions_commitment_hex
                    );
                    println!(
                        "    end_actions_commitment: \"{}\",",
                        end_actions_commitment_hex
                    );

                    if let Some(ref data_avail) = block_info.state_data_availability {
                        println!("    state_data_availability: Some(\"{}\"),", data_avail);
                    } else {
                        println!("    state_data_availability: None,");
                    }

                    if let Some(ref proof_avail) = block_info.proof_data_availability {
                        println!("    proof_data_availability: Some(\"{}\"),", proof_avail);
                    } else {
                        println!("    proof_data_availability: None,");
                    }

                    println!("    created_at: \"{}\",", created_iso);

                    if let Some(ref iso) = state_calculated_iso {
                        println!("    state_calculated_at: Some(\"{}\"),", iso);
                    } else {
                        println!("    state_calculated_at: None,");
                    }

                    if let Some(ref iso) = proved_at_iso {
                        println!("    proved_at: Some(\"{}\"),", iso);
                    } else {
                        println!("    proved_at: None,");
                    }

                    // Print settlement information for all chains
                    println!("    block_settlements: {{");
                    for (chain, settlement) in &app_instance.settlements {
                        // Fetch the BlockSettlement from the ObjectTable
                        let block_settlement_opt =
                            match sui::fetch::fetch_block_settlement(settlement, block).await {
                                Ok(bs) => bs,
                                Err(e) => {
                                    eprintln!(
                                        "Error fetching block settlement for chain {}: {}",
                                        chain, e
                                    );
                                    None
                                }
                            };

                        if let Some(block_settlement) = block_settlement_opt {
                            println!("        \"{}\": BlockSettlement {{", chain);
                            println!(
                                "            block_number: {},",
                                block_settlement.block_number
                            );

                            if let Some(ref tx_hash) = block_settlement.settlement_tx_hash {
                                println!("            settlement_tx_hash: Some(\"{}\"),", tx_hash);
                            } else {
                                println!("            settlement_tx_hash: None,");
                            }

                            println!(
                                "            settlement_tx_included_in_block: {},",
                                block_settlement.settlement_tx_included_in_block
                            );

                            if let Some(sent_at) = block_settlement.sent_to_settlement_at {
                                let sent_iso =
                                    DateTime::<Utc>::from_timestamp_millis(sent_at as i64)
                                        .map(|dt| dt.to_rfc3339())
                                        .unwrap_or_else(|| sent_at.to_string());
                                println!(
                                    "            sent_to_settlement_at: Some(\"{}\"),",
                                    sent_iso
                                );
                            } else {
                                println!("            sent_to_settlement_at: None,");
                            }

                            if let Some(settled_at) = block_settlement.settled_at {
                                let settled_iso =
                                    DateTime::<Utc>::from_timestamp_millis(settled_at as i64)
                                        .map(|dt| dt.to_rfc3339())
                                        .unwrap_or_else(|| settled_at.to_string());
                                println!("            settled_at: Some(\"{}\"),", settled_iso);
                            } else {
                                println!("            settled_at: None,");
                            }

                            println!("        }},");
                        } else {
                            println!(
                                "        \"{}\": None, // No settlement record for this block",
                                chain
                            );
                        }
                    }
                    println!("    }},");

                    println!("}}");
                }
                Ok(None) => {
                    println!("Block {} not found", block);
                }
                Err(e) => {
                    error!("Failed to fetch block {}: {}", block, e);
                    return Err(anyhow::anyhow!("Failed to fetch block: {}", e).into());
                }
            }

            Ok(())
        }

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

            // Fetch and display the job
            // Note: Failed jobs are now in the main jobs table, not a separate table
            let jobs_table_id = if let Some(ref jobs) = app_instance.jobs {
                &jobs.jobs_table_id
            } else {
                error!("App instance has no jobs object");
                return Err(anyhow::anyhow!("App instance has no jobs").into());
            };

            match sui::fetch::fetch_job_by_id(jobs_table_id, job).await {
                Ok(Some(job)) => {
                    // Convert data to hex
                    let data_hex = hex::encode(&job.data);

                    // Convert timestamps to ISO format
                    let created_iso = DateTime::<Utc>::from_timestamp_millis(job.created_at as i64)
                        .map(|dt| dt.to_rfc3339())
                        .unwrap_or_else(|| job.created_at.to_string());
                    let updated_iso = DateTime::<Utc>::from_timestamp_millis(job.updated_at as i64)
                        .map(|dt| dt.to_rfc3339())
                        .unwrap_or_else(|| job.updated_at.to_string());

                    // Print formatted job
                    println!("Job {{");
                    println!("    id: \"{}\",", job.id);
                    println!("    job_sequence: {},", job.job_sequence);

                    if let Some(ref desc) = job.description {
                        println!("    description: Some(\"{}\"),", desc);
                    } else {
                        println!("    description: None,");
                    }

                    println!("    developer: \"{}\",", job.developer);
                    println!("    agent: \"{}\",", job.agent);
                    println!("    agent_method: \"{}\",", job.agent_method);
                    println!("    app: \"{}\",", job.app);
                    println!("    app_instance: \"{}\",", job.app_instance);
                    println!("    app_instance_method: \"{}\",", job.app_instance_method);

                    if let Some(block) = job.block_number {
                        println!("    block_number: Some({}),", block);
                    } else {
                        println!("    block_number: None,");
                    }

                    // Print sequences on one line
                    if let Some(ref seqs) = job.sequences {
                        print!("    sequences: Some([");
                        for (i, seq) in seqs.iter().enumerate() {
                            if i > 0 {
                                print!(", ");
                            }
                            print!("{}", seq);
                        }
                        println!("]),");
                    } else {
                        println!("    sequences: None,");
                    }

                    if let Some(ref seqs1) = job.sequences1 {
                        print!("    sequences1: Some([");
                        for (i, seq) in seqs1.iter().enumerate() {
                            if i > 0 {
                                print!(", ");
                            }
                            print!("{}", seq);
                        }
                        println!("]),");
                    } else {
                        println!("    sequences1: None,");
                    }

                    if let Some(ref seqs2) = job.sequences2 {
                        print!("    sequences2: Some([");
                        for (i, seq) in seqs2.iter().enumerate() {
                            if i > 0 {
                                print!(", ");
                            }
                            print!("{}", seq);
                        }
                        println!("]),");
                    } else {
                        println!("    sequences2: None,");
                    }

                    println!("    data: \"0x{}\",", data_hex);
                    println!("    status: {:?},", job.status);
                    println!("    attempts: {},", job.attempts);

                    if let Some(interval) = job.interval_ms {
                        println!("    interval_ms: Some({}),", interval);
                    } else {
                        println!("    interval_ms: None,");
                    }

                    if let Some(next_at) = job.next_scheduled_at {
                        let next_iso = DateTime::<Utc>::from_timestamp_millis(next_at as i64)
                            .map(|dt| dt.to_rfc3339())
                            .unwrap_or_else(|| next_at.to_string());
                        println!("    next_scheduled_at: Some(\"{}\"),", next_iso);
                    } else {
                        println!("    next_scheduled_at: None,");
                    }

                    println!("    created_at: \"{}\",", created_iso);
                    println!("    updated_at: \"{}\",", updated_iso);
                    println!("}}");
                }
                Ok(None) => {
                    if failed {
                        println!("Failed job {} not found", job);
                    } else {
                        println!("Job {} not found", job);
                    }
                }
                Err(e) => {
                    error!("Failed to fetch job {}: {}", job, e);
                    return Err(anyhow::anyhow!("Failed to fetch job: {}", e).into());
                }
            }

            Ok(())
        }

        Commands::Jobs {
            rpc_url,
            instance,
            failed,
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

            // Check if the app instance has jobs
            if app_instance.jobs.is_none() {
                println!("App instance has no jobs object");
                return Ok(());
            }

            // Fetch all jobs from the app instance (failed or active based on flag)
            let fetch_result = if failed {
                sui::fetch::fetch_failed_jobs_from_app_instance(&app_instance).await
            } else {
                sui::fetch::fetch_all_jobs_from_app_instance(&app_instance).await
            };

            match fetch_result {
                Ok(all_jobs) => {
                    if all_jobs.is_empty() {
                        if failed {
                            println!("No failed jobs found in app instance");
                        } else {
                            println!("No active jobs found in app instance");
                        }
                        return Ok(());
                    }

                    // Print all jobs with job_sequence as key and data as hex
                    for job in all_jobs {
                        println!("{}", job.job_sequence);

                        // Convert data to hex string
                        let data_hex = hex::encode(&job.data);

                        // Print job with hex data
                        println!("Job {{");
                        println!("    id: \"{}\",", job.id);
                        println!("    job_sequence: {},", job.job_sequence);
                        if let Some(ref desc) = job.description {
                            println!("    description: Some(\"{}\"),", desc);
                        } else {
                            println!("    description: None,");
                        }
                        println!("    developer: \"{}\",", job.developer);
                        println!("    agent: \"{}\",", job.agent);
                        println!("    agent_method: \"{}\",", job.agent_method);
                        println!("    app: \"{}\",", job.app);
                        println!("    app_instance: \"{}\",", job.app_instance);
                        println!("    app_instance_method: \"{}\",", job.app_instance_method);
                        if let Some(block) = job.block_number {
                            println!("    block_number: Some({}),", block);
                        } else {
                            println!("    block_number: None,");
                        }
                        if let Some(ref seqs) = job.sequences {
                            println!("    sequences: Some({:?}),", seqs);
                        } else {
                            println!("    sequences: None,");
                        }
                        if let Some(ref seqs1) = job.sequences1 {
                            println!("    sequences1: Some({:?}),", seqs1);
                        } else {
                            println!("    sequences1: None,");
                        }
                        if let Some(ref seqs2) = job.sequences2 {
                            println!("    sequences2: Some({:?}),", seqs2);
                        } else {
                            println!("    sequences2: None,");
                        }
                        println!("    data: \"{}\",", data_hex);
                        println!("    status: {:?},", job.status);
                        println!("    attempts: {},", job.attempts);
                        if let Some(interval) = job.interval_ms {
                            println!("    interval_ms: Some({}),", interval);
                        } else {
                            println!("    interval_ms: None,");
                        }
                        if let Some(next_at) = job.next_scheduled_at {
                            // Convert milliseconds timestamp to ISO format
                            let datetime = DateTime::<Utc>::from_timestamp_millis(next_at as i64)
                                .map(|dt| dt.to_rfc3339())
                                .unwrap_or_else(|| next_at.to_string());
                            println!("    next_scheduled_at: Some(\"{}\"),", datetime);
                        } else {
                            println!("    next_scheduled_at: None,");
                        }
                        // Convert milliseconds timestamps to ISO format
                        let created_iso =
                            DateTime::<Utc>::from_timestamp_millis(job.created_at as i64)
                                .map(|dt| dt.to_rfc3339())
                                .unwrap_or_else(|| job.created_at.to_string());
                        let updated_iso =
                            DateTime::<Utc>::from_timestamp_millis(job.updated_at as i64)
                                .map(|dt| dt.to_rfc3339())
                                .unwrap_or_else(|| job.updated_at.to_string());
                        println!("    created_at: \"{}\",", created_iso);
                        println!("    updated_at: \"{}\",", updated_iso);
                        println!("}}");
                    }
                }
                Err(e) => {
                    let job_type = if failed { "failed jobs" } else { "jobs" };
                    error!("Failed to fetch {}: {}", job_type, e);
                    return Err(anyhow::anyhow!("Failed to fetch {}: {}", job_type, e).into());
                }
            }

            Ok(())
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
            // Initialize minimal logging
            tracing_subscriber::registry()
                .with(tracing_subscriber::EnvFilter::new("info"))
                .with(tracing_subscriber::fmt::layer())
                .init();

            match subcommand {
                BalanceCommands::Sui { rpc_url, address } => {
                    // Resolve and initialize Sui connection
                    let rpc_url = sui::resolve_rpc_url(rpc_url, chain_override.clone())
                        .map_err(CoordinatorError::Other)?;
                    sui::SharedSuiState::initialize(&rpc_url)
                        .await
                        .map_err(CoordinatorError::Other)?;

                    // Show the balance, passing the optional address
                    sui::print_balance_info(address.as_deref())
                        .await
                        .map_err(CoordinatorError::Other)?;
                }

                BalanceCommands::Mina { address, network } => {
                    // Validate address format
                    if !mina::validate_mina_address(&address) {
                        eprintln!("❌ Error: Invalid Mina address format");
                        eprintln!(
                            "   Mina addresses should start with 'B62' and be at least 55 characters"
                        );
                        return Err(anyhow!("Invalid Mina address format").into());
                    }

                    println!("📊 Checking balance for {} on {}", address, network);
                    println!();

                    // Check if account exists first
                    match mina::account_exists(&address, &network).await {
                        Ok(exists) => {
                            if !exists {
                                println!("⚠️  Account not found on {}", network);
                                println!(
                                    "   The account may not have been created yet or may not exist on this network"
                                );

                                // Show explorer link if available
                                if let Some(network_config) =
                                    mina::MinaNetwork::get_network(&network)
                                {
                                    if let Some(explorer_url) = network_config.explorer_account_url
                                    {
                                        println!("   Check explorer: {}{}", explorer_url, address);
                                    }
                                }
                                return Ok(());
                            }
                        }
                        Err(e) => {
                            eprintln!("⚠️  Warning: Could not verify account existence: {}", e);
                        }
                    }

                    // Fetch balance
                    match mina::get_balance_in_mina(&address, &network).await {
                        Ok(balance) => {
                            println!("💰 Balance: {} MINA", balance);

                            // Also fetch account info for more details
                            match mina::get_account_info(&address, &network).await {
                                Ok(info) => {
                                    println!("   Nonce: {}", info.nonce);
                                    if let Some(symbol) = info.token_symbol {
                                        println!("   Token Symbol: {}", symbol);
                                    }
                                }
                                Err(_) => {
                                    // Ignore error, we already have the balance
                                }
                            }

                            // Show explorer link if available
                            if let Some(network_config) = mina::MinaNetwork::get_network(&network) {
                                if let Some(explorer_url) = network_config.explorer_account_url {
                                    println!();
                                    println!("🔍 View on explorer:");
                                    println!("   {}{}", explorer_url, address);
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("❌ Failed to fetch balance: {}", e);
                            return Err(e.into());
                        }
                    }
                }

                BalanceCommands::Ethereum { address, network } => {
                    println!("📊 Checking balance for {} on {}", address, network);
                    println!();

                    // Get network configuration to display native currency
                    let network_config = ethereum::EthereumNetwork::get_network(&network);
                    let symbol = network_config
                        .map(|n| n.native_currency.symbol.clone())
                        .unwrap_or_else(|| "ETH".to_string());

                    // Fetch balance
                    match ethereum::get_balance(&address, &network).await {
                        Ok(balance) => {
                            println!("💰 Balance: {} {}", balance, symbol);

                            // Also fetch account info for more details
                            match ethereum::get_account_info(&address, &network).await {
                                Ok(info) => {
                                    println!("   Nonce: {}", info.nonce);
                                    println!("   Network: {}", info.network);
                                }
                                Err(_) => {
                                    // Ignore error, we already have the balance
                                }
                            }

                            // Show explorer link if available
                            if let Some(network_config) = network_config {
                                if let Some(explorer_url) = &network_config.explorer {
                                    println!();
                                    println!("🔍 View on explorer:");
                                    println!("   {}/address/{}", explorer_url, address);
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("❌ Failed to fetch balance: {}", e);
                            eprintln!();
                            eprintln!("   Available networks:");
                            for net in ethereum::EthereumNetwork::list_networks() {
                                eprintln!("   • {}", net);
                            }
                            return Err(e.into());
                        }
                    }
                }
            }

            Ok(())
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

        Commands::Faucet { subcommand } => {
            // Initialize minimal logging
            tracing_subscriber::registry()
                .with(tracing_subscriber::EnvFilter::new("info"))
                .with(tracing_subscriber::fmt::layer())
                .init();

            match subcommand {
                FaucetCommands::Sui { address, amount } => {
                    // Get the chain from environment
                    let chain = std::env::var("SUI_CHAIN")
                        .unwrap_or_else(|_| "devnet".to_string())
                        .to_lowercase();

                    // Check if mainnet (no faucet available)
                    if chain == "mainnet" {
                        error!("❌ Faucet is not available for mainnet");
                        error!("   Please acquire SUI tokens through an exchange or other means");
                        return Err(anyhow!("Faucet not available for mainnet").into());
                    }

                    // Validate amount
                    if amount > 10.0 {
                        error!("❌ Amount exceeds maximum of 10 SUI");
                        error!("   Maximum faucet amount is 10 SUI per request");
                        return Err(anyhow!("Amount exceeds maximum of 10 SUI").into());
                    }

                    if amount <= 0.0 {
                        error!("❌ Amount must be greater than 0");
                        return Err(anyhow!("Invalid amount").into());
                    }

                    // Get the address to fund
                    let target_address = address.unwrap_or_else(|| {
                        std::env::var("SUI_ADDRESS").unwrap_or_else(|_| {
                            error!("❌ No address provided and SUI_ADDRESS not set");
                            std::process::exit(1);
                        })
                    });

                    // Validate address format
                    if !target_address.starts_with("0x") || target_address.len() != 66 {
                        error!("❌ Invalid SUI address format: {}", target_address);
                        error!("   Address should start with '0x' and be 66 characters long");
                        return Err(anyhow!("Invalid address format").into());
                    }

                    info!("💧 Requesting {} SUI from {} faucet...", amount, chain);
                    info!("📍 Target address: {}", target_address);

                    // Get RPC URL based on chain using the resolver
                    let rpc_url = sui::resolve_rpc_url(None, Some(chain.clone()))?;

                    // Initialize Sui connection to check balance
                    sui::SharedSuiState::initialize(&rpc_url)
                        .await
                        .map_err(CoordinatorError::Other)?;

                    // Check balance before faucet
                    info!("📊 Balance before faucet:");
                    let balance_before = sui::get_balance_in_sui(&target_address)
                        .await
                        .map_err(CoordinatorError::Other)?;
                    info!("   {:.4} SUI", balance_before);

                    // Call the faucet
                    info!("🚰 Calling faucet...");
                    let faucet_result =
                        sui::request_tokens_from_faucet(&chain, &target_address, Some(amount))
                            .await;

                    match faucet_result {
                        Ok(tx_digest) => {
                            info!("✅ Faucet request sent!");

                            // Handle the case where transaction digest is not immediately available
                            if tx_digest != "unknown" {
                                info!("   Transaction: {}", tx_digest);
                                info!(
                                    "   🔗 Explorer: https://suiscan.xyz/{}/tx/{}",
                                    chain, tx_digest
                                );
                            } else {
                                info!(
                                    "   🔗 Explorer: https://suiscan.xyz/{}/account/{}",
                                    chain, target_address
                                );
                            }

                            // Wait for transaction to be processed
                            info!(
                                "⏳ Waiting {} seconds for transaction to be processed...",
                                constants::CLI_TRANSACTION_WAIT_SECS
                            );
                            tokio::time::sleep(tokio::time::Duration::from_secs(
                                constants::CLI_TRANSACTION_WAIT_SECS,
                            ))
                            .await;

                            // Check balance after faucet
                            info!("📊 Balance after faucet:");
                            let balance_after = sui::get_balance_in_sui(&target_address)
                                .await
                                .map_err(CoordinatorError::Other)?;
                            info!("   {:.4} SUI", balance_after);

                            let received = balance_after - balance_before;
                            if received > 0.0 {
                                info!("💰 Received: {:.4} SUI", received);
                            }
                        }
                        Err(e) => {
                            error!("❌ Faucet failed: {}", e);
                            return Err(e.into());
                        }
                    }
                }

                FaucetCommands::Mina { address, network } => {
                    // Validate the address format
                    if !mina::validate_mina_address(&address) {
                        error!("❌ Invalid Mina address format");
                        error!(
                            "   Mina addresses should start with 'B62' and be at least 55 characters"
                        );
                        return Err(anyhow!("Invalid Mina address format").into());
                    }

                    // Validate network - the network module will handle validation
                    if mina::MinaNetwork::get_network(&network).is_none() {
                        error!("❌ Invalid network '{}'", network);
                        error!(
                            "   Supported networks: mina:devnet, zeko:testnet (or devnet, zeko for short)"
                        );
                        return Err(anyhow!("Invalid network").into());
                    }

                    info!("💧 Requesting MINA from {} faucet...", network);
                    info!("📍 Target address: {}", address);

                    // Call the Mina faucet
                    match mina::request_mina_from_faucet(&address, &network).await {
                        Ok(response) => {
                            if let Some(status) = &response.status {
                                if status == "rate-limit" {
                                    error!("⚠️  Rate limited by faucet");
                                    error!("   Please wait 30 minutes before trying again");
                                    return Err(anyhow!("Rate limited by faucet").into());
                                }
                            }

                            if let Some(error) = &response.error {
                                error!("❌ Faucet error: {}", error);
                                return Err(anyhow!("Faucet error: {}", error).into());
                            }

                            info!("✅ Faucet request successful!");
                            if let Some(message) = &response.message {
                                info!("   Response: {}", message);
                            }

                            info!(
                                "⏳ Note: It may take a few minutes for the funds to appear in your account"
                            );

                            // Get explorer URL from network config
                            if let Some(network_config) = mina::MinaNetwork::get_network(&network) {
                                if let Some(explorer_url) = network_config.explorer_account_url {
                                    info!("   Check your balance at:");
                                    info!("   {}{}", explorer_url, address);
                                }
                            }
                        }
                        Err(e) => {
                            error!("❌ Faucet request failed: {}", e);
                            return Err(e.into());
                        }
                    }
                }
            }

            Ok(())
        }

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

        Commands::Secrets { subcommand } => {
            // Initialize minimal logging
            tracing_subscriber::registry()
                .with(tracing_subscriber::EnvFilter::new("info"))
                .with(tracing_subscriber::fmt::layer())
                .init();

            // Get default endpoint if not specified
            let get_endpoint = || {
                std::env::var("SILVANA_RPC_SERVER")
                    .unwrap_or_else(|_| "https://rpc.silvana.dev".to_string())
            };

            match subcommand {
                cli::SecretsCommands::Store {
                    endpoint,
                    developer,
                    agent,
                    name,
                    secret,
                    app,
                    app_instance,
                } => {
                    let endpoint = endpoint.unwrap_or_else(get_endpoint);

                    info!("🔗 Connecting to RPC endpoint: {}", endpoint);

                    let mut client = match secrets_client::SecretsClient::new(&endpoint).await {
                        Ok(client) => client,
                        Err(e) => {
                            error!("Failed to connect to RPC endpoint {}: {}", endpoint, e);
                            return Err(anyhow!("Failed to connect to RPC endpoint").into());
                        }
                    };

                    info!("📦 Storing secret...");
                    info!("  Developer: {}", developer);
                    info!("  Agent: {}", agent);
                    info!("  Name: {}", name);
                    if let Some(ref app) = app {
                        info!("  App: {}", app);
                    }
                    if let Some(ref app_instance) = app_instance {
                        info!("  App Instance: {}", app_instance);
                    }
                    info!("  Secret length: {} characters", secret.len());

                    // TODO: For now using empty signature - will be replaced with actual signature validation
                    let placeholder_signature = vec![];

                    match client
                        .store_secret(
                            &developer,
                            &agent,
                            app.as_deref(),
                            app_instance.as_deref(),
                            Some(&name),
                            &secret,
                            &placeholder_signature,
                        )
                        .await
                    {
                        Ok(()) => {
                            info!("✅ Secret stored successfully!");
                        }
                        Err(e) => {
                            error!("Failed to store secret: {}", e);
                            return Err(anyhow!("Failed to store secret").into());
                        }
                    }
                }

                cli::SecretsCommands::Retrieve {
                    endpoint,
                    developer,
                    agent,
                    name,
                    app,
                    app_instance,
                } => {
                    let endpoint = endpoint.unwrap_or_else(get_endpoint);

                    info!("🔗 Connecting to RPC endpoint: {}", endpoint);

                    let mut client = match secrets_client::SecretsClient::new(&endpoint).await {
                        Ok(client) => client,
                        Err(e) => {
                            error!("Failed to connect to RPC endpoint {}: {}", endpoint, e);
                            return Err(anyhow!("Failed to connect to RPC endpoint").into());
                        }
                    };

                    info!("🔍 Retrieving secret...");
                    info!("  Developer: {}", developer);
                    info!("  Agent: {}", agent);
                    info!("  Name: {}", name);
                    if let Some(ref app) = app {
                        info!("  App: {}", app);
                    }
                    if let Some(ref app_instance) = app_instance {
                        info!("  App Instance: {}", app_instance);
                    }

                    // TODO: For now using empty signature - will be replaced with actual signature validation
                    let placeholder_signature = vec![];

                    match client
                        .retrieve_secret(
                            &developer,
                            &agent,
                            app.as_deref(),
                            app_instance.as_deref(),
                            Some(&name),
                            &placeholder_signature,
                        )
                        .await
                    {
                        Ok(secret_value) => {
                            info!("✅ Secret retrieved successfully!");
                            info!("📋 Secret value: {}", secret_value);
                            info!("📏 Secret length: {} characters", secret_value.len());
                        }
                        Err(secrets_client::SecretsClientError::SecretNotFound) => {
                            error!("❌ Secret not found with the specified parameters");
                            info!(
                                "💡 Try checking if the secret exists with different scope parameters (app, app-instance)"
                            );
                            return Err(anyhow!("Secret not found").into());
                        }
                        Err(e) => {
                            error!("Failed to retrieve secret: {}", e);
                            return Err(anyhow!("Failed to retrieve secret").into());
                        }
                    }
                }
            }

            Ok(())
        }
    }
}
