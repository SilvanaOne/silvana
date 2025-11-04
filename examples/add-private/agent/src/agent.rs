//! Agent command - Run as agent to process jobs from coordinator

use anyhow::{Context, Result};
use log::{error, info, warn};
use serde::Deserialize;
use silvana_sdk::{agent, complete_job, fail_job, get_job};
use std::time::Duration;
use tokio::time::sleep;

/// Agent configuration
#[derive(Debug, Deserialize)]
struct AgentConfig {
    /// Coordinator gRPC URL
    #[serde(default = "default_coordinator_url")]
    coordinator_url: String,

    /// Session ID for this agent instance (optional - usually provided by coordinator via env)
    #[serde(default)]
    session_id: Option<String>,

    /// Developer identifier
    #[serde(default)]
    developer: Option<String>,

    /// Agent identifier
    #[serde(default)]
    agent: Option<String>,

    /// Agent method name
    #[serde(default)]
    agent_method: Option<String>,

    /// Maximum runtime in seconds (0 = run forever)
    #[serde(default)]
    max_runtime_secs: u64,
}

fn default_coordinator_url() -> String {
    "http://host.docker.internal:50051".to_string()
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

    /// Override configuration from environment variables (Docker coordinator pattern)
    /// Environment variables take precedence over config file values
    fn override_from_env(&mut self) {
        if let Ok(url) = std::env::var("COORDINATOR_URL") {
            self.coordinator_url = url;
        }

        if let Ok(session_id) = std::env::var("SESSION_ID") {
            self.session_id = Some(session_id);
        }

        if let Ok(developer) = std::env::var("DEVELOPER") {
            self.developer = Some(developer);
        }

        if let Ok(agent) = std::env::var("AGENT") {
            self.agent = Some(agent);
        }

        if let Ok(agent_method) = std::env::var("AGENT_METHOD") {
            self.agent_method = Some(agent_method);
        }

        if let Ok(max_runtime) = std::env::var("MAX_RUNTIME_SECS") {
            if let Ok(val) = max_runtime.parse() {
                self.max_runtime_secs = val;
            }
        }
    }

    /// Validate that required fields are present
    fn validate(&self) -> Result<()> {
        if self.session_id.is_none() {
            anyhow::bail!("SESSION_ID is required (from config file or environment variable)");
        }
        if self.developer.is_none() {
            anyhow::bail!("DEVELOPER is required (from config file or environment variable)");
        }
        if self.agent.is_none() {
            anyhow::bail!("AGENT is required (from config file or environment variable)");
        }
        if self.agent_method.is_none() {
            anyhow::bail!("AGENT_METHOD is required (from config file or environment variable)");
        }
        Ok(())
    }
}

/// Process a single job
async fn process_job(job_id: &str, app_instance_method: &str) -> Result<()> {
    info!(
        "Processing job {}: method={}",
        job_id, app_instance_method
    );

    // Send log to coordinator
    agent::info(&format!(
        "Starting job processing: method={}",
        app_instance_method
    ))
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
            fail_job(&format!("Unknown job method: {}", method))
                .await
                .context("Failed to fail job")?;
            return Ok(());
        }
    }

    // Mark job as completed
    info!("  → Completing job {}", job_id);
    complete_job().await.context("Failed to complete job")?;

    // Send completion log to coordinator
    agent::info(&format!("Job {} completed", job_id))
        .await
        .ok();

    info!("✓ Job {} completed successfully", job_id);
    Ok(())
}

/// Main agent loop
async fn run_agent(config: AgentConfig) -> Result<()> {
    // Extract required fields (validated before calling this function)
    let session_id = config
        .session_id
        .as_ref()
        .context("SESSION_ID is required")?;
    let developer = config.developer.as_ref().context("DEVELOPER is required")?;
    let agent_name = config.agent.as_ref().context("AGENT is required")?;
    let agent_method = config
        .agent_method
        .as_ref()
        .context("AGENT_METHOD is required")?;

    info!("Starting Add Private Agent");
    info!("  Coordinator: {}", config.coordinator_url);
    info!("  Session ID: {}", session_id);
    info!("  Developer: {}", developer);
    info!("  Agent: {}", agent_name);
    info!("  Agent Method: {}", agent_method);

    // Set environment variables for SDK
    std::env::set_var("SESSION_ID", session_id);
    std::env::set_var("DEVELOPER", developer);
    std::env::set_var("AGENT", agent_name);
    std::env::set_var("AGENT_METHOD", agent_method);
    std::env::set_var("COORDINATOR_URL", &config.coordinator_url);

    info!("✓ SDK environment configured");

    // Send agent started message
    agent::info("Agent started and ready for jobs").await.ok();

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
        match get_job().await {
            Ok(Some(job)) => {
                job_count += 1;
                info!("\nJob #{} (ID: {})", job_count, job.job_id);
                info!("  App: {} / {}", job.app, job.app_instance);
                info!("  Method: {}", job.app_instance_method);
                info!("  Agent: {} / {}", job.agent, job.agent_method);
                info!("  Sequence: {}", job.job_sequence);

                // Process the job
                if let Err(e) = process_job(&job.job_id, &job.app_instance_method).await {
                    error!("✗ Error processing job {}: {}", job.job_id, e);

                    // Try to mark job as failed
                    if let Err(fail_err) = fail_job(&format!("Agent error: {}", e)).await {
                        error!("Failed to mark job as failed: {}", fail_err);
                    }

                    // Send error log to coordinator
                    agent::error(&format!("Job failed: {}", e)).await.ok();
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
                agent::error(&format!("Error fetching jobs: {}", e))
                    .await
                    .ok();
                // Exit on error
                break;
            }
        }
    }

    signal_handle.close();

    // Send agent stopped message
    agent::info(&format!("Agent stopped. Processed {} job(s)", job_count))
        .await
        .ok();

    info!("\nAgent stopped. Processed {} job(s) total.", job_count);

    Ok(())
}

pub async fn run() -> Result<()> {
    // Load .env file if exists
    let _ = dotenvy::dotenv();

    // Load base configuration from file, or use defaults if file not found
    let mut config = if let Ok(config) = AgentConfig::from_file("config.toml") {
        info!("Loaded base configuration from config.toml");
        config
    } else {
        info!("Config file not found, using defaults");
        AgentConfig {
            coordinator_url: default_coordinator_url(),
            session_id: None,
            developer: None,
            agent: None,
            agent_method: None,
            max_runtime_secs: 0,
        }
    };

    // Environment variables override config file values (Docker coordinator pattern)
    config.override_from_env();

    // Validate that required fields are present
    config.validate()?;

    // Run the agent
    if let Err(e) = run_agent(config).await {
        error!("Agent error: {}", e);
        std::process::exit(1);
    }

    Ok(())
}
