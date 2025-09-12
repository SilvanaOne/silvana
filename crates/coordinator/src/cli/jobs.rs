use crate::error::{CoordinatorError, Result};
use anyhow::anyhow;
use chrono::{DateTime, Utc};
use tracing::error;
use tracing_subscriber::prelude::*;

pub async fn handle_job_command(
    rpc_url: Option<String>,
    instance: String,
    job: u64,
    failed: bool,
    chain_override: Option<String>,
) -> Result<()> {
    // Initialize minimal logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("warn"))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Resolve and initialize Sui connection (read-only mode)
    let rpc_url = sui::resolve_rpc_url(rpc_url, chain_override)
        .map_err(CoordinatorError::Other)?;
    sui::SharedSuiState::initialize_read_only(&rpc_url)
        .await
        .map_err(CoordinatorError::Other)?;

    // Fetch the app instance first
    let app_instance = match sui::fetch::fetch_app_instance(&instance).await {
        Ok(instance) => instance,
        Err(e) => {
            error!("Failed to fetch app instance {}: {}", instance, e);
            return Err(anyhow!("Failed to fetch app instance: {}", e).into());
        }
    };

    // Fetch and display the job
    // Note: Failed jobs are now in the main jobs table, not a separate table
    let jobs_table_id = if let Some(ref jobs) = app_instance.jobs {
        &jobs.jobs_table_id
    } else {
        error!("App instance has no jobs object");
        return Err(anyhow!("App instance has no jobs").into());
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
            return Err(anyhow!("Failed to fetch job: {}", e).into());
        }
    }

    Ok(())
}

pub async fn handle_jobs_command(
    rpc_url: Option<String>,
    instance: String,
    failed: bool,
    chain_override: Option<String>,
) -> Result<()> {
    // Initialize minimal logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("warn"))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Resolve and initialize Sui connection (read-only mode)
    let rpc_url = sui::resolve_rpc_url(rpc_url, chain_override)
        .map_err(CoordinatorError::Other)?;
    sui::SharedSuiState::initialize_read_only(&rpc_url)
        .await
        .map_err(CoordinatorError::Other)?;

    // Fetch the app instance first
    let app_instance = match sui::fetch::fetch_app_instance(&instance).await {
        Ok(instance) => instance,
        Err(e) => {
            error!("Failed to fetch app instance {}: {}", instance, e);
            return Err(anyhow!("Failed to fetch app instance: {}", e).into());
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
            return Err(anyhow!("Failed to fetch {}: {}", job_type, e).into());
        }
    }

    Ok(())
}