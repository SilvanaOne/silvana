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
mod session_id;
mod settlement;
mod state;
mod stuck_jobs;

use clap::Parser;
use dotenvy::dotenv;
use tracing::error;
use tracing_subscriber::prelude::*;
use chrono::{DateTime, Utc};

use crate::cli::{Cli, Commands};
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
                    println!("{:#?}", app_instance);
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
                Ok(Some(block)) => {
                    println!("{:#?}", block);
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
        
        Commands::Job { rpc_url, instance, job } => {
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
                    println!("Job {} not found", job);
                }
                Err(e) => {
                    error!("Failed to fetch job {}: {}", job, e);
                    return Err(anyhow::anyhow!("Failed to fetch job: {}", e).into());
                }
            }
            
            Ok(())
        }
        
        Commands::Jobs { rpc_url, instance } => {
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
            
            // Fetch all jobs from the app instance
            match sui::fetch::fetch_all_jobs_from_app_instance(&app_instance).await {
                Ok(all_jobs) => {
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
                    error!("Failed to fetch jobs: {}", e);
                    return Err(anyhow::anyhow!("Failed to fetch jobs: {}", e).into());
                }
            }
            
            Ok(())
        }
    }
}