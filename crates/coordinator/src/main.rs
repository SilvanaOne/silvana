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
use chrono::{DateTime, Utc};

use crate::cli::{Cli, Commands, TransactionType, BalanceCommands};
use crate::error::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file from current directory
    dotenv().ok();

    let cli = Cli::parse();
    
    match cli.command {
        Commands::Start { 
            rpc_url, 
            package_id, 
            use_tee, 
            container_timeout, 
            log_level,
            grpc_socket_path 
        } => {
            // Initialize logging for the start command
            tracing_subscriber::registry()
                .with(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| log_level.into()),
                )
                .with(tracing_subscriber::fmt::layer())
                .init();

            let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

            // Start the coordinator
            coordinator::start_coordinator(
                rpc_url,
                package_id,
                use_tee,
                container_timeout,
                grpc_socket_path,
            ).await
        }
        
        Commands::Instance { rpc_url, instance } => {
            // Initialize minimal logging
            tracing_subscriber::registry()
                .with(tracing_subscriber::EnvFilter::new("warn"))
                .with(tracing_subscriber::fmt::layer())
                .init();
            
            // Initialize Sui connection
            sui::SharedSuiState::initialize(&rpc_url).await?;
            
            // Fetch and display the app instance
            match sui::fetch::fetch_app_instance(&instance).await {
                Ok(app_instance) => {
                    // Convert timestamps to ISO format
                    let previous_block_timestamp_iso = DateTime::<Utc>::from_timestamp_millis(app_instance.previous_block_timestamp as i64)
                        .map(|dt| dt.to_rfc3339())
                        .unwrap_or_else(|| app_instance.previous_block_timestamp.to_string());
                    
                    let created_at_iso = DateTime::<Utc>::from_timestamp_millis(app_instance.created_at as i64)
                        .map(|dt| dt.to_rfc3339())
                        .unwrap_or_else(|| app_instance.created_at.to_string());
                    
                    // Print formatted app instance
                    println!("AppInstance {{");
                    println!("    id: \"{}\",", app_instance.id);
                    println!("    silvana_app_name: \"{}\",", app_instance.silvana_app_name);
                    
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
                    println!("    methods: {},", serde_json::to_string(&app_instance.methods).unwrap_or_else(|_| "{}".to_string()));
                    
                    // Print state (as JSON)
                    println!("    state: {},", serde_json::to_string(&app_instance.state).unwrap_or_else(|_| "{}".to_string()));
                    
                    println!("    blocks_table_id: \"{}\",", app_instance.blocks_table_id);
                    println!("    proof_calculations_table_id: \"{}\",", app_instance.proof_calculations_table_id);
                    
                    // Print sequence_state_manager (as JSON)
                    println!("    sequence_state_manager: {},", serde_json::to_string(&app_instance.sequence_state_manager).unwrap_or_else(|_| "{}".to_string()));
                    
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
                        if let Some(ref settlement_job) = jobs.settlement_job {
                            println!("        settlement_job: Some({}),", settlement_job);
                        } else {
                            println!("        settlement_job: None,");
                        }
                        println!("    }}),");
                    } else {
                        println!("    jobs: None,");
                    }
                    
                    println!("    sequence: {},", app_instance.sequence);
                    println!("    admin: \"{}\",", app_instance.admin);
                    println!("    block_number: {},", app_instance.block_number);
                    println!("    previous_block_timestamp: \"{}\",", previous_block_timestamp_iso);
                    println!("    previous_block_last_sequence: {},", app_instance.previous_block_last_sequence);
                    println!("    previous_block_actions_state: \"{}\",", app_instance.previous_block_actions_state);
                    println!("    last_proved_block_number: {},", app_instance.last_proved_block_number);
                    println!("    last_settled_block_number: {},", app_instance.last_settled_block_number);
                    
                    if let Some(ref chain) = app_instance.settlement_chain {
                        println!("    settlement_chain: Some(\"{}\"),", chain);
                    } else {
                        println!("    settlement_chain: None,");
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
        
        Commands::Block { rpc_url, instance, block } => {
            // Initialize minimal logging
            tracing_subscriber::registry()
                .with(tracing_subscriber::EnvFilter::new("warn"))
                .with(tracing_subscriber::fmt::layer())
                .init();
            
            // Initialize Sui connection
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
                    let start_actions_commitment_hex = hex::encode(&block_info.start_actions_commitment);
                    let end_actions_commitment_hex = hex::encode(&block_info.end_actions_commitment);
                    
                    // Convert timestamps to ISO format
                    let created_iso = DateTime::<Utc>::from_timestamp_millis(block_info.created_at as i64)
                        .map(|dt| dt.to_rfc3339())
                        .unwrap_or_else(|| block_info.created_at.to_string());
                    
                    let state_calculated_iso = block_info.state_calculated_at
                        .and_then(|ts| DateTime::<Utc>::from_timestamp_millis(ts as i64))
                        .map(|dt| dt.to_rfc3339());
                    
                    let proved_at_iso = block_info.proved_at
                        .and_then(|ts| DateTime::<Utc>::from_timestamp_millis(ts as i64))
                        .map(|dt| dt.to_rfc3339());
                    
                    let sent_to_settlement_at_iso = block_info.sent_to_settlement_at
                        .and_then(|ts| DateTime::<Utc>::from_timestamp_millis(ts as i64))
                        .map(|dt| dt.to_rfc3339());
                    
                    let settled_at_iso = block_info.settled_at
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
                    println!("    time_since_last_block: {},", block_info.time_since_last_block);
                    println!("    number_of_transactions: {},", block_info.number_of_transactions);
                    println!("    start_actions_commitment: \"{}\",", start_actions_commitment_hex);
                    println!("    end_actions_commitment: \"{}\",", end_actions_commitment_hex);
                    
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
                    
                    if let Some(ref tx_hash) = block_info.settlement_tx_hash {
                        println!("    settlement_tx_hash: Some(\"{}\"),", tx_hash);
                    } else {
                        println!("    settlement_tx_hash: None,");
                    }
                    
                    println!("    settlement_tx_included_in_block: {},", block_info.settlement_tx_included_in_block);
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
                    
                    if let Some(ref iso) = sent_to_settlement_at_iso {
                        println!("    sent_to_settlement_at: Some(\"{}\"),", iso);
                    } else {
                        println!("    sent_to_settlement_at: None,");
                    }
                    
                    if let Some(ref iso) = settled_at_iso {
                        println!("    settled_at: Some(\"{}\"),", iso);
                    } else {
                        println!("    settled_at: None,");
                    }
                    
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
        
        Commands::Proofs { rpc_url, instance, block } => {
            // Initialize minimal logging
            tracing_subscriber::registry()
                .with(tracing_subscriber::EnvFilter::new("warn"))
                .with(tracing_subscriber::fmt::layer())
                .init();
            
            // Initialize Sui connection
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
                    error!("Failed to fetch proof calculation for block {}: {}", block, e);
                    return Err(anyhow::anyhow!("Failed to fetch proof calculation: {}", e).into());
                }
            }
            
            Ok(())
        }
        
        Commands::Job { rpc_url, instance, job, failed } => {
            // Initialize minimal logging
            tracing_subscriber::registry()
                .with(tracing_subscriber::EnvFilter::new("warn"))
                .with(tracing_subscriber::fmt::layer())
                .init();
            
            // Initialize Sui connection
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
        
        Commands::Jobs { rpc_url, instance, failed } => {
            // Initialize minimal logging
            tracing_subscriber::registry()
                .with(tracing_subscriber::EnvFilter::new("warn"))
                .with(tracing_subscriber::fmt::layer())
                .init();
            
            // Initialize Sui connection
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
                        let created_iso = DateTime::<Utc>::from_timestamp_millis(job.created_at as i64)
                            .map(|dt| dt.to_rfc3339())
                            .unwrap_or_else(|| job.created_at.to_string());
                        let updated_iso = DateTime::<Utc>::from_timestamp_millis(job.updated_at as i64)
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
        
        Commands::Transaction { rpc_url, private_key, tx_type } => {
            // Initialize minimal logging
            tracing_subscriber::registry()
                .with(tracing_subscriber::EnvFilter::new("info"))
                .with(tracing_subscriber::fmt::layer())
                .init();
            
            // Initialize Sui connection with optional private key (will use env var if not provided)
            match sui::SharedSuiState::initialize_with_optional_key(&rpc_url, private_key.as_deref()).await {
                Ok(()) => {},
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
                    warn!("⚠️ Failed to initialize gas coin pool: {}. Continuing anyway.", e);
                    println!("⚠️ Warning: Failed to initialize gas coin pool");
                    println!("This may cause transaction failures. Error: {}", e);
                }
            }
            
            // Create interface
            let mut interface = sui::interface::SilvanaSuiInterface::new();
            
            // Execute the requested transaction
            match tx_type {
                TransactionType::TerminateJob { instance, job, gas } => {
                    println!("Terminating job {} in instance {} (gas: {} SUI)", job, instance, gas);
                    
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
                                println!("Note: Transaction was submitted but failed during execution");
                            } else {
                                println!("Transaction was not submitted to the blockchain");
                            }
                            println!("Error: {}", error_msg);
                            return Err(anyhow::anyhow!("Transaction failed").into());
                        }
                    }
                }
                
                TransactionType::RestartFailedJob { instance, job, gas } => {
                    println!("Restarting failed job {} in instance {} (gas: {} SUI)", job, instance, gas);
                    
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
                                println!("Note: Transaction was submitted but failed during execution");
                            } else {
                                println!("Transaction was not submitted to the blockchain");
                            }
                            println!("Error: {}", error_msg);
                            return Err(anyhow::anyhow!("Transaction failed").into());
                        }
                    }
                }
                
                TransactionType::RestartFailedJobs { instance, gas } => {
                    println!("Restarting all failed jobs in instance {} (gas: {} SUI)", instance, gas);
                    
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
                                println!("Note: Transaction was submitted but failed during execution");
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
        
        Commands::Balance { rpc_url, command } => {
            // Initialize minimal logging
            tracing_subscriber::registry()
                .with(tracing_subscriber::EnvFilter::new("info"))
                .with(tracing_subscriber::fmt::layer())
                .init();
            
            // Initialize Sui connection
            sui::SharedSuiState::initialize(&rpc_url).await?;
            
            match command {
                BalanceCommands::Check => {
                    // Use the encapsulated balance function from sui crate
                    sui::print_balance_info().await?;
                }
                
                BalanceCommands::Faucet { min_balance, network, amount } => {
                    // Parse network
                    let faucet_network = match network.to_lowercase().as_str() {
                        "testnet" => sui::FaucetNetwork::Testnet,
                        "devnet" => sui::FaucetNetwork::Devnet,
                        _ => {
                            error!("Invalid network: {}. Use 'devnet' or 'testnet'", network);
                            return Err(anyhow::anyhow!("Invalid network: {}. Use 'devnet' or 'testnet'", network).into());
                        }
                    };
                    
                    // Set default min_balance based on network
                    let min_bal = min_balance.unwrap_or(
                        if matches!(faucet_network, sui::FaucetNetwork::Testnet) { 10.0 } else { 5.0 }
                    );
                    
                    println!("Checking balance and requesting from {:?} faucet if needed (minimum: {} SUI)...", 
                        faucet_network, min_bal);
                    
                    match sui::faucet::ensure_sufficient_balance_network(min_bal, faucet_network, amount).await {
                        Ok(requested) => {
                            if requested {
                                println!("✅ Balance was below {} SUI, requested tokens from {:?} faucet", 
                                    min_bal, faucet_network);
                                println!("Tokens should arrive within 1 minute");
                            } else {
                                println!("✅ Balance is sufficient (above {} SUI), no faucet request needed", min_bal);
                            }
                        }
                        Err(e) => {
                            error!("Failed to check/request faucet tokens: {}", e);
                            return Err(anyhow::anyhow!("Failed to check/request faucet tokens: {}", e).into());
                        }
                    }
                }
                
                BalanceCommands::Split => {
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
                            return Err(anyhow::anyhow!("Failed to manage gas coin pool: {}", e).into());
                        }
                    }
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
            
            // Initialize Sui connection
            sui::SharedSuiState::initialize(&rpc_url).await?;
            
            let network_name = sui::get_network_name();
            let address = sui::get_current_address();
            
            println!("🌐 Network: {}", network_name);
            println!("👤 Address: {}", address);
            
            // Print detailed network info
            match sui::print_network_info().await {
                Ok(()) => {},
                Err(e) => {
                    error!("Failed to fetch network info: {}", e);
                    return Err(anyhow::anyhow!("Failed to fetch network info: {}", e).into());
                }
            }
            
            Ok(())
        }
    }
}