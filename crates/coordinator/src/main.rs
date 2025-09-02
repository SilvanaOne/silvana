mod agent;
mod block;
mod cli;
mod config;
mod coordinator;
mod error;
mod events;
mod failed_jobs_cache;
mod grpc;
mod hardware;
mod job_id;
mod job_searcher;
mod jobs;
mod merge;
mod metrics;
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

use crate::cli::{Cli, Commands, TransactionType};
use crate::error::Result;
use anyhow::anyhow;

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
                eprintln!("Error: Invalid chain '{}'. Must be one of: devnet, testnet, mainnet", chain);
                std::process::exit(1);
            }
        }
    }

    match cli.command {
        Commands::Start {
            rpc_url,
            package_id,
            use_tee,
            container_timeout,
            log_level: _,
            grpc_socket_path,
            instance,
        } => {
            // Resolve the RPC URL using the helper from sui crate
            let rpc_url = sui::resolve_rpc_url(rpc_url, chain_override.clone())?;
            // Initialize logging with New Relic tracing layer
            monitoring::init_logging_with_newrelic().await?;
            info!("✅ Logging initialized with New Relic support");

            // Initialize New Relic OpenTelemetry exporters
            if let Err(e) = monitoring::newrelic::init_newrelic().await {
                warn!("⚠️  Failed to initialize New Relic exporters: {}", e);
                warn!("   Continuing without New Relic metrics/traces");
            } else {
                info!("✅ New Relic exporters initialized");
            }

            // Initialize monitoring system
            monitoring::init_monitoring()?;

            let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

            // Start the coordinator
            coordinator::start_coordinator(
                rpc_url,
                package_id,
                use_tee,
                container_timeout,
                grpc_socket_path,
                instance,
            )
            .await
        }

        Commands::Instance { rpc_url, instance } => {
            // Initialize minimal logging
            tracing_subscriber::registry()
                .with(tracing_subscriber::EnvFilter::new("warn"))
                .with(tracing_subscriber::fmt::layer())
                .init();

            // Resolve and initialize Sui connection
            let rpc_url = sui::resolve_rpc_url(rpc_url, chain_override.clone())?;
            sui::SharedSuiState::initialize(&rpc_url).await?;

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
                        // Note: failed_jobs_table_id no longer exists - failed jobs are in main jobs table
                        println!("        failed_jobs_count: {},", jobs.failed_jobs_count);
                        println!("        failed_jobs_index: {:?},", jobs.failed_jobs_index);
                        println!("        pending_jobs: {:?},", jobs.pending_jobs);
                        println!("        pending_jobs_count: {},", jobs.pending_jobs_count);

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
                            "            block_settlements: {} entries,",
                            settlement.block_settlements.len()
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

            // Resolve and initialize Sui connection
            let rpc_url = sui::resolve_rpc_url(rpc_url, chain_override.clone())?;
            sui::SharedSuiState::initialize(&rpc_url).await?;

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
                        if let Some(block_settlement) = settlement.block_settlements.get(&block) {
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

            // Resolve and initialize Sui connection
            let rpc_url = sui::resolve_rpc_url(rpc_url, chain_override.clone())?;
            sui::SharedSuiState::initialize(&rpc_url).await?;

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
                    println!("ProofCalculation:");
                    println!("{:#?}", proof_calc);
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

            // Resolve and initialize Sui connection
            let rpc_url = sui::resolve_rpc_url(rpc_url, chain_override.clone())?;
            sui::SharedSuiState::initialize(&rpc_url).await?;

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
                    println!("{:#?}", job);
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

            // Resolve and initialize Sui connection
            let rpc_url = sui::resolve_rpc_url(rpc_url, chain_override.clone())?;
            sui::SharedSuiState::initialize(&rpc_url).await?;

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
            // Initialize minimal logging
            tracing_subscriber::registry()
                .with(tracing_subscriber::EnvFilter::new("info"))
                .with(tracing_subscriber::fmt::layer())
                .init();

            // Resolve RPC URL and initialize Sui connection with optional private key
            let rpc_url = sui::resolve_rpc_url(rpc_url, chain_override.clone())?;
            match sui::SharedSuiState::initialize_with_optional_key(
                &rpc_url,
                private_key.as_deref(),
            )
            .await
            {
                Ok(()) => {}
                Err(e) => {
                    error!("Failed to initialize Sui connection: {}", e);
                    println!("❌ Failed to initialize Sui connection");
                    println!("Error: {}", e);
                    return Err(e.into());
                }
            }

            // Initialize gas coin pool for better transaction performance (like in start command)
            info!("🪙 Initializing gas coin pool...");
            match sui::coin_management::initialize_gas_coin_pool().await {
                Ok(()) => info!("✅ Gas coin pool initialized"),
                Err(e) => {
                    warn!(
                        "⚠️ Failed to initialize gas coin pool: {}. Continuing anyway.",
                        e
                    );
                    println!("⚠️ Warning: Failed to initialize gas coin pool");
                    println!("This may cause transaction failures. Error: {}", e);
                }
            }

            // Create interface
            let mut interface = sui::interface::SilvanaSuiInterface::new();

            // Execute the requested transaction
            match tx_type {
                TransactionType::TerminateJob { instance, job, gas } => {
                    println!(
                        "Terminating job {} in instance {} (gas: {} SUI)",
                        job, instance, gas
                    );

                    match interface.terminate_job(&instance, job, gas).await {
                        Ok(tx_digest) => {
                            println!("✅ Transaction executed successfully");
                            println!("Transaction digest: {}", tx_digest);
                            println!("Job {} has been terminated", job);
                        }
                        Err((error_msg, tx_digest)) => {
                            println!("❌ Transaction execution failed");
                            if let Some(digest) = tx_digest {
                                println!("Transaction digest: {}", digest);
                                println!(
                                    "Note: Transaction was submitted but failed during execution"
                                );
                            } else {
                                println!("Transaction was not submitted to the blockchain");
                            }
                            println!("Error: {}", error_msg);
                            return Err(anyhow::anyhow!("Transaction failed").into());
                        }
                    }
                }

                TransactionType::RestartFailedJob { instance, job, gas } => {
                    println!(
                        "Restarting failed job {} in instance {} (gas: {} SUI)",
                        job, instance, gas
                    );

                    match interface.restart_failed_job(&instance, job, gas).await {
                        Ok(tx_digest) => {
                            println!("✅ Transaction executed successfully");
                            println!("Transaction digest: {}", tx_digest);
                            println!("Failed job {} has been restarted", job);
                        }
                        Err((error_msg, tx_digest)) => {
                            println!("❌ Transaction execution failed");
                            if let Some(digest) = tx_digest {
                                println!("Transaction digest: {}", digest);
                                println!(
                                    "Note: Transaction was submitted but failed during execution"
                                );
                            } else {
                                println!("Transaction was not submitted to the blockchain");
                            }
                            println!("Error: {}", error_msg);
                            return Err(anyhow::anyhow!("Transaction failed").into());
                        }
                    }
                }

                TransactionType::RestartFailedJobs { instance, gas } => {
                    println!(
                        "Restarting all failed jobs in instance {} (gas: {} SUI)",
                        instance, gas
                    );

                    match interface.restart_failed_jobs(&instance, gas).await {
                        Ok(tx_digest) => {
                            println!("✅ Transaction executed successfully");
                            println!("Transaction digest: {}", tx_digest);
                            println!("All failed jobs have been restarted");
                        }
                        Err((error_msg, tx_digest)) => {
                            println!("❌ Transaction execution failed");
                            if let Some(digest) = tx_digest {
                                println!("Transaction digest: {}", digest);
                                println!(
                                    "Note: Transaction was submitted but failed during execution"
                                );
                            } else {
                                println!("Transaction was not submitted to the blockchain");
                            }
                            println!("Error: {}", error_msg);
                            return Err(anyhow::anyhow!("Transaction failed").into());
                        }
                    }
                }
            }

            Ok(())
        }

        Commands::Balance { rpc_url } => {
            // Initialize minimal logging
            tracing_subscriber::registry()
                .with(tracing_subscriber::EnvFilter::new("info"))
                .with(tracing_subscriber::fmt::layer())
                .init();

            // Resolve and initialize Sui connection
            let rpc_url = sui::resolve_rpc_url(rpc_url, chain_override.clone())?;
            sui::SharedSuiState::initialize(&rpc_url).await?;

            // Simply show the balance
            sui::print_balance_info().await?;

            Ok(())
        }
        
        Commands::Split { rpc_url } => {
            // Initialize minimal logging
            tracing_subscriber::registry()
                .with(tracing_subscriber::EnvFilter::new("info"))
                .with(tracing_subscriber::fmt::layer())
                .init();

            // Resolve and initialize Sui connection
            let rpc_url = sui::resolve_rpc_url(rpc_url, chain_override.clone())?;
            sui::SharedSuiState::initialize(&rpc_url).await?;

            println!("Checking gas coin pool and splitting if needed...");

            match sui::coin_management::ensure_gas_coin_pool().await {
                Ok(()) => {
                    println!("✅ Gas coin pool check complete");

                    // Show updated balance info
                    println!("\nUpdated balance:");
                    sui::print_balance_info().await?;
                }
                Err(e) => {
                    error!("Failed to manage gas coin pool: {}", e);
                    return Err(
                        anyhow::anyhow!("Failed to manage gas coin pool: {}", e).into()
                    );
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
            let rpc_url = sui::resolve_rpc_url(rpc_url, chain_override.clone())?;
            sui::SharedSuiState::initialize(&rpc_url).await?;

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
        
        Commands::Faucet { address, amount } => {
            // Initialize minimal logging
            tracing_subscriber::registry()
                .with(tracing_subscriber::EnvFilter::new("info"))
                .with(tracing_subscriber::fmt::layer())
                .init();
            
            // Get the chain from environment
            let chain = std::env::var("SUI_CHAIN").unwrap_or_else(|_| "devnet".to_string()).to_lowercase();
            
            // Check if mainnet (no faucet available)
            if chain == "mainnet" {
                eprintln!("❌ Error: Faucet is not available for mainnet");
                eprintln!("   Please acquire SUI tokens through an exchange or other means");
                return Err(anyhow!("Faucet not available for mainnet").into());
            }
            
            // Validate amount
            if amount > 10.0 {
                eprintln!("❌ Error: Amount exceeds maximum of 10 SUI");
                eprintln!("   Maximum faucet amount is 10 SUI per request");
                return Err(anyhow!("Amount exceeds maximum of 10 SUI").into());
            }
            
            if amount <= 0.0 {
                eprintln!("❌ Error: Amount must be greater than 0");
                return Err(anyhow!("Invalid amount").into());
            }
            
            // Get the address to fund
            let target_address = address.unwrap_or_else(|| {
                std::env::var("SUI_ADDRESS").unwrap_or_else(|_| {
                    eprintln!("❌ Error: No address provided and SUI_ADDRESS not set");
                    std::process::exit(1);
                })
            });
            
            // Validate address format
            if !target_address.starts_with("0x") || target_address.len() != 66 {
                eprintln!("❌ Error: Invalid SUI address format: {}", target_address);
                eprintln!("   Address should start with '0x' and be 66 characters long");
                return Err(anyhow!("Invalid address format").into());
            }
            
            println!("💧 Requesting {} SUI from {} faucet...", amount, chain);
            println!("📍 Target address: {}", target_address);
            
            // Get RPC URL based on chain using the resolver
            let rpc_url = sui::resolve_rpc_url(None, Some(chain.clone()))?;
            
            // Initialize Sui connection to check balance
            sui::SharedSuiState::initialize(&rpc_url).await?;
            
            // Check balance before faucet
            println!("\n📊 Balance before faucet:");
            let balance_before = sui::get_balance_in_sui(&target_address).await?;
            println!("   {:.4} SUI", balance_before);
            
            // Call the faucet
            println!("\n🚰 Calling faucet...");
            let faucet_result = call_faucet(&chain, &target_address, amount).await;
            
            match faucet_result {
                Ok(tx_digest) => {
                    println!("✅ Faucet successful!");
                    println!("   Transaction: {}", tx_digest);
                    
                    // Wait for transaction to be processed
                    println!("\n⏳ Waiting 5 seconds for transaction to be processed...");
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    
                    // Check balance after faucet
                    println!("\n📊 Balance after faucet:");
                    let balance_after = sui::get_balance_in_sui(&target_address).await?;
                    println!("   {:.4} SUI", balance_after);
                    
                    let received = balance_after - balance_before;
                    if received > 0.0 {
                        println!("\n💰 Received: {:.4} SUI", received);
                    }
                }
                Err(e) => {
                    eprintln!("❌ Faucet failed: {}", e);
                    return Err(e.into());
                }
            }
            
            Ok(())
        }
    }
}

/// Call the faucet for the specified chain and address
async fn call_faucet(chain: &str, address: &str, amount_sui: f64) -> anyhow::Result<String> {
    use serde_json::json;
    
    // Get faucet URL from environment or use defaults
    let faucet_url = match chain {
        "devnet" => std::env::var("SUI_FAUCET_DEVNET")
            .unwrap_or_else(|_| "https://faucet.devnet.sui.io/v2/gas".to_string()),
        "testnet" => std::env::var("SUI_FAUCET_TESTNET")
            .unwrap_or_else(|_| "https://sui-faucet.staketab.com".to_string()),
        _ => return Err(anyhow!("Invalid chain for faucet: {}", chain)),
    };
    
    // Check if using Silvana faucet (staketab.com)
    let is_silvana_faucet = faucet_url.contains("staketab.com");
    
    // Create HTTP client
    let client = reqwest::Client::new();
    
    // Build request body based on faucet type
    let body = if is_silvana_faucet {
        // Silvana faucet format - amount is in MIST (1 SUI = 1,000,000,000 MIST)
        let amount_mist = (amount_sui * 1_000_000_000.0) as u64;
        json!({
            "address": address,
            "amount": amount_mist
        })
    } else {
        // Official Sui faucet format - also in MIST
        let amount_mist = (amount_sui * 1_000_000_000.0) as u64;
        json!({
            "FixedAmountRequest": {
                "recipient": address,
                "amount": amount_mist
            }
        })
    };
    
    // Determine endpoint
    let endpoint = if is_silvana_faucet {
        format!("{}/fund", faucet_url)
    } else {
        faucet_url.clone()
    };
    
    // Send the request
    let response = client
        .post(&endpoint)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| anyhow!("Failed to send faucet request: {}", e))?;
    
    // Check response status
    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        return Err(anyhow!("Faucet request failed with status {}: {}", status, error_text));
    }
    
    // Parse response
    let result: serde_json::Value = response
        .json()
        .await
        .map_err(|e| anyhow!("Failed to parse faucet response: {}", e))?;
    
    // Handle Silvana faucet response format
    if is_silvana_faucet {
        if let Some(success) = result.get("success") {
            if success.as_bool() == Some(true) {
                if let Some(tx_hash) = result.get("transaction_hash") {
                    if let Some(tx_str) = tx_hash.as_str() {
                        return Ok(tx_str.to_string());
                    }
                }
            }
        }
        if let Some(error) = result.get("error") {
            return Err(anyhow!("Silvana faucet error: {}", error));
        }
        return Err(anyhow!("Unexpected Silvana faucet response format: {}", result));
    }
    
    // Handle official Sui faucet response format
    if let Some(transferred_gas_objects) = result.get("transferredGasObjects") {
        if let Some(first_obj) = transferred_gas_objects.as_array().and_then(|arr| arr.first()) {
            if let Some(transfer_tx) = first_obj.get("transferTxDigest") {
                if let Some(tx_str) = transfer_tx.as_str() {
                    return Ok(tx_str.to_string());
                }
            }
        }
    }
    
    // Try alternate response format
    if let Some(task) = result.get("task") {
        if let Some(tx_digest) = task.as_str() {
            return Ok(tx_digest.to_string());
        }
    }
    
    if let Some(error) = result.get("error") {
        return Err(anyhow!("Faucet error: {}", error));
    }
    
    Err(anyhow!("Unexpected faucet response format: {}", result))
}
