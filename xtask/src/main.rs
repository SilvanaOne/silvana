use clap::{Parser, Subcommand};
use duct::cmd;
use secrets_client::SecretsClient;

/// Get the default RPC server endpoint from environment or fallback
fn get_default_endpoint() -> String {
    // Load .env file if it exists
    dotenvy::dotenv().ok();
    
    // Get SILVANA_RPC_SERVER from environment, fallback to default if not set
    std::env::var("SILVANA_RPC_SERVER")
        .unwrap_or_else(|_| "https://rpc.silvana.dev".to_string())
}

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Build Docker images for every binary
    DockerBuild,
    /// Wipe & re-apply TiDB migrations on the local dev container
    DbReset,
    /// Store a secret via gRPC
    StoreSecret {
        /// RPC endpoint (uses SILVANA_RPC_SERVER env var if not specified)
        #[arg(long, default_value_t = get_default_endpoint())]
        endpoint: String,
        /// Developer identifier
        #[arg(long)]
        developer: String,
        /// Agent identifier  
        #[arg(long)]
        agent: String,
        /// Secret name/key
        #[arg(long)]
        name: String,
        /// Secret value to store
        #[arg(long)]
        secret: String,
        /// Optional app identifier
        #[arg(long)]
        app: Option<String>,
        /// Optional app instance identifier
        #[arg(long)]
        app_instance: Option<String>,
    },
    /// Retrieve a secret via gRPC
    RetrieveSecret {
        /// RPC endpoint (uses SILVANA_RPC_SERVER env var if not specified)
        #[arg(long, default_value_t = get_default_endpoint())]
        endpoint: String,
        /// Developer identifier
        #[arg(long)]
        developer: String,
        /// Agent identifier  
        #[arg(long)]
        agent: String,
        /// Secret name/key
        #[arg(long)]
        name: String,
        /// Optional app identifier
        #[arg(long)]
        app: Option<String>,
        /// Optional app instance identifier
        #[arg(long)]
        app_instance: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::DockerBuild => {
            println!("Building Docker images...");
            //cmd!("docker", "build", "-f", "docker/rpc.Dockerfile", ".").run()?;
            //cmd!("docker", "build", "-f", "docker/coordinator.Dockerfile", ".").run()?;
        }
        Cmd::DbReset => {
            println!("Resetting database...");
            cmd!(
                "docker",
                "exec",
                "tidb",
                "sh",
                "-c",
                "source /migrations/reset.sql"
            )
            .run()?;
        }
        Cmd::StoreSecret {
            endpoint,
            developer,
            agent,
            name,
            secret,
            app,
            app_instance,
        } => {
            store_secret(
                &endpoint,
                &developer,
                &agent,
                &name,
                &secret,
                app.as_deref(),
                app_instance.as_deref(),
            )
            .await?;
        }
        Cmd::RetrieveSecret {
            endpoint,
            developer,
            agent,
            name,
            app,
            app_instance,
        } => {
            retrieve_secret(
                &endpoint,
                &developer,
                &agent,
                &name,
                app.as_deref(),
                app_instance.as_deref(),
            )
            .await?;
        }
    }
    Ok(())
}

async fn store_secret(
    endpoint: &str,
    developer: &str,
    agent: &str,
    name: &str,
    secret: &str,
    app: Option<&str>,
    app_instance: Option<&str>,
) -> anyhow::Result<()> {
    // Validation
    if developer.trim().is_empty() {
        anyhow::bail!("Developer cannot be empty");
    }
    if agent.trim().is_empty() {
        anyhow::bail!("Agent cannot be empty");
    }
    if name.trim().is_empty() {
        anyhow::bail!("Name cannot be empty");
    }
    if secret.trim().is_empty() {
        anyhow::bail!("Secret value cannot be empty");
    }

    println!("🔗 Connecting to RPC endpoint: {}", endpoint);
    
    let mut client = match SecretsClient::new(endpoint).await {
        Ok(client) => client,
        Err(e) => {
            anyhow::bail!("Failed to connect to RPC endpoint {}: {}", endpoint, e);
        }
    };
    
    println!("📦 Storing secret...");
    println!("  Developer: {}", developer);
    println!("  Agent: {}", agent);
    println!("  Name: {}", name);
    if let Some(app) = app {
        println!("  App: {}", app);
    }
    if let Some(app_instance) = app_instance {
        println!("  App Instance: {}", app_instance);
    }
    println!("  Secret length: {} characters", secret.len());
    
    // TODO: For now using empty signature - will be replaced with actual signature validation
    let placeholder_signature = vec![];
    
    match client
        .store_secret(
            developer,
            agent,
            app,
            app_instance,
            Some(name),
            secret,
            &placeholder_signature,
        )
        .await
    {
        Ok(()) => {
            println!("✅ Secret stored successfully!");
        }
        Err(e) => {
            anyhow::bail!("Failed to store secret: {}", e);
        }
    }
    
    Ok(())
}

async fn retrieve_secret(
    endpoint: &str,
    developer: &str,
    agent: &str,
    name: &str,
    app: Option<&str>,
    app_instance: Option<&str>,
) -> anyhow::Result<()> {
    // Validation
    if developer.trim().is_empty() {
        anyhow::bail!("Developer cannot be empty");
    }
    if agent.trim().is_empty() {
        anyhow::bail!("Agent cannot be empty");
    }
    if name.trim().is_empty() {
        anyhow::bail!("Name cannot be empty");
    }

    println!("🔗 Connecting to RPC endpoint: {}", endpoint);
    
    let mut client = match SecretsClient::new(endpoint).await {
        Ok(client) => client,
        Err(e) => {
            anyhow::bail!("Failed to connect to RPC endpoint {}: {}", endpoint, e);
        }
    };
    
    println!("🔍 Retrieving secret...");
    println!("  Developer: {}", developer);
    println!("  Agent: {}", agent);
    println!("  Name: {}", name);
    if let Some(app) = app {
        println!("  App: {}", app);
    }
    if let Some(app_instance) = app_instance {
        println!("  App Instance: {}", app_instance);
    }
    
    // TODO: For now using empty signature - will be replaced with actual signature validation
    let placeholder_signature = vec![];
    
    match client
        .retrieve_secret(
            developer,
            agent,
            app,
            app_instance,
            Some(name),
            &placeholder_signature,
        )
        .await
    {
        Ok(secret_value) => {
            println!("✅ Secret retrieved successfully!");
            println!("📋 Secret value: {}", secret_value);
            println!("📏 Secret length: {} characters", secret_value.len());
        }
        Err(secrets_client::SecretsClientError::SecretNotFound) => {
            println!("❌ Secret not found with the specified parameters");
            println!("💡 Try checking if the secret exists with different scope parameters (app, app-instance)");
            std::process::exit(1);
        }
        Err(e) => {
            anyhow::bail!("Failed to retrieve secret: {}", e);
        }
    }
    
    Ok(())
}
