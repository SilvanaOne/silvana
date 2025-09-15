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
mod session;
mod settlement;
mod state;
mod stuck_jobs;

use clap::Parser;
use dotenvy::dotenv;

use crate::cli::{Cli, Commands};
use crate::error::Result;
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
        println!(
            "   curl -sSL https://raw.githubusercontent.com/SilvanaOne/silvana/main/install.sh | bash"
        );
        println!();
    }
}

#[tokio::main]
async fn main() -> Result<()> {
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
    fetch_and_inject_config(&chain).await;

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
            )
            .await
        }

        Commands::New { name, force } => cli::new::handle_new_command(name, force).await,
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
            cli::jobs::handle_job_command(rpc_url, instance, job, failed, chain_override.clone())
                .await
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
            cli::split::handle_split_command(rpc_url, chain_override).await
        }

        Commands::Network { rpc_url } => {
            cli::network::handle_network_command(rpc_url, chain_override).await
        }

        Commands::Config { endpoint, json } => {
            cli::config_cmd::handle_config_command(endpoint, json, chain_override).await
        }

        Commands::Faucet { subcommand } => cli::faucet::handle_faucet_command(subcommand).await,
        Commands::Keypair { subcommand } => cli::keypair::handle_keypair_command(subcommand).await,

        Commands::Avs { subcommand } => cli::avs::handle_avs_command(subcommand).await,

        Commands::Registry {
            rpc_url,
            subcommand,
        } => cli::registry::handle_registry_command(rpc_url, subcommand, chain_override).await,

        Commands::Secrets { subcommand } => cli::secrets::handle_secrets_command(subcommand).await,
    }
}
