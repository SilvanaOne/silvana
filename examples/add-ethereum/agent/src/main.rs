//! Add Ethereum Agent
//!
//! This agent processes jobs from the coordinator service via gRPC.
//! It demonstrates a simple job processing pattern with three job types:
//! - init: Initialize the application (mocked with 10s delay)
//! - add: Process add operations (mocked with 10s delay)
//! - settle: Settle state to settlement chain (mocked with 10s delay)
//!
//! The agent communicates with the coordinator service using gRPC,
//! similar to the TypeScript agent in examples/add/agent.

use anyhow::{Context, Result};
use coordinator_client::{CoordinatorClient, LogLevel};
use log::{error, info, warn};
use serde::Deserialize;
use std::time::Duration;
use tokio::time::sleep;

/// Agent configuration
#[derive(Debug, Deserialize)]
struct AgentConfig {
    /// Coordinator gRPC URL
    coordinator_url: String,

    /// Session ID for this agent instance
    session_id: String,

    /// Developer identifier
    developer: String,

    /// Agent identifier
    agent: String,

    /// Agent method name
    agent_method: String,

    /// Polling interval in seconds
    #[serde(default = "default_poll_interval")]
    poll_interval_secs: u64,

    /// Maximum runtime in seconds (0 = run forever)
    #[serde(default)]
    max_runtime_secs: u64,
}

fn default_poll_interval() -> u64 {
    5
}

impl AgentConfig {
    /// Load configuration from file
    fn from_file(path: &str) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path))?;

        let config: Self = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path))?;

        Ok(config)
    }

    /// Load from environment variables (for Docker)
    fn from_env() -> Result<Self> {
        Ok(AgentConfig {
            coordinator_url: std::env::var("COORDINATOR_URL")
                .unwrap_or_else(|_| "http://host.docker.internal:50051".to_string()),
            session_id: std::env::var("SESSION_ID")
                .context("SESSION_ID environment variable is required")?,
            developer: std::env::var("DEVELOPER")
                .context("DEVELOPER environment variable is required")?,
            agent: std::env::var("AGENT").context("AGENT environment variable is required")?,
            agent_method: std::env::var("AGENT_METHOD")
                .context("AGENT_METHOD environment variable is required")?,
            poll_interval_secs: std::env::var("POLL_INTERVAL_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(5),
            max_runtime_secs: std::env::var("MAX_RUNTIME_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
        })
    }
}

/// Process a single job
async fn process_job(
    client: &mut CoordinatorClient,
    job_id: &str,
    app_instance_method: &str,
) -> Result<()> {
    info!("Processing job {}: method={}", job_id, app_instance_method);

    // Send log to coordinator
    client
        .send_log(
            LogLevel::Info,
            &format!("Starting job processing: method={}", app_instance_method),
        )
        .await
        .ok();

    match app_instance_method {
        "init" => {
            info!("  → Mocking init job processing...");
            sleep(Duration::from_secs(10)).await;
            info!("  → Init job completed");
        }
        "add" => {
            info!("  → Mocking add job processing...");
            sleep(Duration::from_secs(10)).await;
            info!("  → Add job completed");
        }
        "settle" => {
            info!("  → Mocking settle job processing...");
            sleep(Duration::from_secs(10)).await;
            info!("  → Settle job completed");
        }
        method => {
            warn!("  → Unknown job method: {}, failing job", method);
            client
                .fail_job(&format!("Unknown job method: {}", method))
                .await
                .context("Failed to fail job")?;
            return Ok(());
        }
    }

    // Mark job as completed
    info!("  → Completing job {}", job_id);
    client.complete_job().await.context("Failed to complete job")?;

    // Send completion log to coordinator
    client
        .send_log(LogLevel::Info, &format!("Job {} completed", job_id))
        .await
        .ok();

    info!("✓ Job {} completed successfully", job_id);
    Ok(())
}

/// Main agent loop
async fn run_agent(config: AgentConfig) -> Result<()> {
    info!("Starting Add Ethereum Agent");
    info!("  Coordinator: {}", config.coordinator_url);
    info!("  Session ID: {}", config.session_id);
    info!("  Developer: {}", config.developer);
    info!("  Agent: {}", config.agent);
    info!("  Agent Method: {}", config.agent_method);
    info!("  Poll Interval: {}s", config.poll_interval_secs);

    // Initialize coordinator client
    let mut client = CoordinatorClient::new(
        &config.coordinator_url,
        config.session_id.clone(),
        config.developer.clone(),
        config.agent.clone(),
        config.agent_method.clone(),
    )
    .await
    .context("Failed to initialize coordinator client")?;

    info!("✓ Coordinator client initialized");

    // Send agent started message
    client
        .send_log(LogLevel::Info, "Agent started and ready for jobs")
        .await
        .ok();

    let start_time = std::time::Instant::now();
    let max_runtime = if config.max_runtime_secs > 0 {
        Some(Duration::from_secs(config.max_runtime_secs))
    } else {
        None
    };

    // Setup signal handling for graceful shutdown
    let mut signals = signal_hook_tokio::Signals::new(&[
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGTERM,
    ])?;

    let signal_handle = signals.handle();

    // Spawn signal handler
    let shutdown = tokio::spawn(async move {
        use futures::StreamExt;
        while let Some(signal) = signals.next().await {
            match signal {
                signal_hook::consts::SIGINT | signal_hook::consts::SIGTERM => {
                    info!("Received shutdown signal, stopping agent...");
                    return true;
                }
                _ => {}
            }
        }
        false
    });

    info!("Agent running. Press Ctrl+C to stop.\n");

    let mut job_count = 0;

    loop {
        // Check for shutdown signal
        if shutdown.is_finished() {
            break;
        }

        // Check max runtime
        if let Some(max_runtime) = max_runtime {
            if start_time.elapsed() >= max_runtime {
                info!("Maximum runtime reached, stopping agent");
                break;
            }
        }

        // Fetch pending job
        match client.get_job().await {
            Ok(Some(job)) => {
                job_count += 1;
                info!("\nJob #{} (ID: {})", job_count, job.job_id);
                info!("  App: {} / {}", job.app, job.app_instance);
                info!("  Method: {}", job.app_instance_method);
                info!("  Agent: {} / {}", job.agent, job.agent_method);
                info!("  Sequence: {}", job.job_sequence);

                // Process the job
                if let Err(e) = process_job(&mut client, &job.job_id, &job.app_instance_method).await
                {
                    error!("✗ Error processing job {}: {}", job.job_id, e);

                    // Try to mark job as failed
                    if let Err(fail_err) = client.fail_job(&format!("Agent error: {}", e)).await {
                        error!("Failed to mark job as failed: {}", fail_err);
                    }

                    // Send error log to coordinator
                    client
                        .send_log(LogLevel::Error, &format!("Job failed: {}", e))
                        .await
                        .ok();
                }
            }
            Ok(None) => {
                // No job available - exit (following TypeScript agent pattern)
                info!("No job available - exiting");
                break;
            }
            Err(e) => {
                error!("Error fetching jobs: {}", e);
                // Send error log to coordinator
                client
                    .send_log(LogLevel::Error, &format!("Error fetching jobs: {}", e))
                    .await
                    .ok();
                // Exit on error
                break;
            }
        }
    }

    signal_handle.close();

    // Send agent stopped message
    client
        .send_log(
            LogLevel::Info,
            &format!("Agent stopped. Processed {} job(s)", job_count),
        )
        .await
        .ok();

    info!("\nAgent stopped. Processed {} job(s) total.", job_count);

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logger
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    // Load configuration - try config file first, then environment variables
    let config = if let Ok(config) = AgentConfig::from_file("config.toml") {
        info!("Loaded configuration from config.toml");
        config
    } else {
        info!("Config file not found, loading from environment variables");
        AgentConfig::from_env()?
    };

    // Run the agent
    if let Err(e) = run_agent(config).await {
        error!("Agent error: {}", e);
        std::process::exit(1);
    }

    Ok(())
}
