use clap::{Parser, Subcommand};
use duct::cmd;
use rpc_client::{RpcClientConfig, SilvanaRpcClient};
use secrets_client::SecretsClient;
use std::collections::HashMap;
use std::fs;
use storage;

/// Get the default RPC server endpoint from environment or fallback
fn get_default_endpoint() -> String {
    // Load .env file if it exists
    dotenvy::dotenv().ok();

    // Get SILVANA_RPC_SERVER from environment, fallback to default if not set
    std::env::var("SILVANA_RPC_SERVER")
        .unwrap_or_else(|_| "https://rpc-devnet.silvana.dev".to_string())
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
    /// Pack examples/add folder and upload to S3 as examples/add.tar.zst
    ExampleArchive,
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
    /// Write configuration to RPC server
    WriteConfig {
        /// RPC endpoint (uses SILVANA_RPC_SERVER env var if not specified)
        #[arg(long, default_value_t = get_default_endpoint())]
        endpoint: String,
        /// Chain identifier (testnet or devnet)
        #[arg(long)]
        chain: String,
    },
    /// Read configuration from RPC server
    ReadConfig {
        /// RPC endpoint (uses SILVANA_RPC_SERVER env var if not specified)
        #[arg(long, default_value_t = get_default_endpoint())]
        endpoint: String,
        /// Chain identifier (testnet, devnet, or mainnet)
        #[arg(long)]
        chain: String,
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
        Cmd::ExampleArchive => {
            pack_examples_add().await?;
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
        Cmd::WriteConfig { endpoint, chain } => {
            write_config(&endpoint, &chain).await?;
        }
        Cmd::ReadConfig { endpoint, chain } => {
            read_config(&endpoint, &chain).await?;
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
            println!(
                "💡 Try checking if the secret exists with different scope parameters (app, app-instance)"
            );
            std::process::exit(1);
        }
        Err(e) => {
            anyhow::bail!("Failed to retrieve secret: {}", e);
        }
    }

    Ok(())
}

async fn write_config(endpoint: &str, chain: &str) -> anyhow::Result<()> {
    // Validate chain
    if !["devnet", "testnet", "mainnet"].contains(&chain) {
        anyhow::bail!(
            "Invalid chain: {}. Must be devnet, testnet, or mainnet",
            chain
        );
    }

    // Determine the config file path
    let config_file = match chain {
        "devnet" => "infra/config/.env.devnet",
        "testnet" => "infra/config/.env.testnet",
        "mainnet" => "infra/config/.env.mainnet",
        _ => unreachable!(),
    };

    // Check if the config file exists
    if !std::path::Path::new(config_file).exists() {
        anyhow::bail!("Config file not found: {}", config_file);
    }

    println!("📖 Reading configuration from: {}", config_file);

    // Read the config file
    let content = fs::read_to_string(config_file)?;

    // Parse the .env file into a HashMap
    let mut config = HashMap::new();
    for line in content.lines() {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Parse KEY=VALUE format
        if let Some(pos) = line.find('=') {
            let key = line[..pos].trim().to_string();
            let value = line[pos + 1..].trim().to_string();

            // Remove quotes if present
            let value = if (value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\''))
            {
                value[1..value.len() - 1].to_string()
            } else {
                value
            };

            config.insert(key, value);
        }
    }

    if config.is_empty() {
        anyhow::bail!("No configuration found in {}", config_file);
    }

    println!("📊 Found {} configuration items", config.len());

    // Connect to RPC
    println!("🔗 Connecting to RPC endpoint: {}", endpoint);

    let mut client = SilvanaRpcClient::new(rpc_client::RpcClientConfig::new(endpoint))
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to RPC endpoint {}: {}", endpoint, e))?;

    println!("📤 Writing configuration for chain: {}", chain);

    // Write the config
    match client.write_config(chain, config).await {
        Ok(response) => {
            if response.success {
                println!("✅ Configuration written successfully!");
                println!("  Items written: {}", response.items_written);
                println!("  Message: {}", response.message);
            } else {
                println!("❌ Failed to write configuration");
                println!("  Message: {}", response.message);
                std::process::exit(1);
            }
        }
        Err(e) => {
            anyhow::bail!("Failed to write configuration: {}", e);
        }
    }

    Ok(())
}

async fn read_config(endpoint: &str, chain: &str) -> anyhow::Result<()> {
    // Validate chain
    if !["devnet", "testnet", "mainnet"].contains(&chain) {
        anyhow::bail!(
            "Invalid chain: {}. Must be devnet, testnet, or mainnet",
            chain
        );
    }

    println!("🔗 Connecting to RPC endpoint: {}", endpoint);

    let mut client = SilvanaRpcClient::new(rpc_client::RpcClientConfig::new(endpoint))
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to RPC endpoint {}: {}", endpoint, e))?;

    println!("📥 Reading configuration for chain: {}", chain);

    // Read the config
    match client.get_config(chain).await {
        Ok(response) => {
            if response.success {
                println!("✅ Configuration retrieved successfully!");
                println!("  Message: {}", response.message);

                if response.config.is_empty() {
                    println!("  ⚠️  No configuration found for chain: {}", chain);
                } else {
                    println!("\n📋 Configuration ({} items):", response.config.len());
                    println!("  {}", "-".repeat(60));

                    // Sort keys for consistent output
                    let mut keys: Vec<_> = response.config.keys().collect();
                    keys.sort();

                    for key in keys {
                        if let Some(value) = response.config.get(key) {
                            // Mask sensitive values
                            let display_value = if key.to_lowercase().contains("key")
                                || key.to_lowercase().contains("secret")
                                || key.to_lowercase().contains("password")
                                || key.to_lowercase().contains("token")
                            {
                                if value.len() > 8 {
                                    format!("{}...{}", &value[..4], &value[value.len() - 4..])
                                } else {
                                    "***".to_string()
                                }
                            } else {
                                value.clone()
                            };

                            println!("  {} = {}", key, display_value);
                        }
                    }
                    println!("  {}", "-".repeat(60));
                }
            } else {
                println!("❌ Failed to read configuration");
                println!("  Message: {}", response.message);
                std::process::exit(1);
            }
        }
        Err(e) => {
            anyhow::bail!("Failed to read configuration: {}", e);
        }
    }

    Ok(())
}

async fn pack_examples_add() -> anyhow::Result<()> {
    println!("📦 Packing examples/add folder...");

    // Get the examples/add folder path
    let current_dir = std::env::current_dir()?;
    let examples_add_path = current_dir.join("examples").join("add");

    // Verify source folder exists
    if !examples_add_path.exists() {
        anyhow::bail!("examples/add folder not found at {:?}", examples_add_path);
    }

    println!("Source folder: {:?}", examples_add_path);

    // Create the archive using storage utility
    let archive_data = storage::pack_folder_to_bytes(
        &examples_add_path,
        None, // Use default config with max compression
    )?;

    println!("✅ Archive created: {} bytes", archive_data.len());

    // Calculate SHA256
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(&archive_data);
    let hash = format!("{:x}", hasher.finalize());
    println!("   SHA256: {}", hash);

    // Upload via RPC
    println!("📤 Uploading to RPC server...");

    // Get RPC endpoint from environment or use default
    let rpc_endpoint = std::env::var("SILVANA_RPC_SERVER")
        .unwrap_or_else(|_| "https://rpc-devnet.silvana.dev".to_string());

    // Initialize RPC client
    let mut rpc_client = SilvanaRpcClient::new(RpcClientConfig::new(&rpc_endpoint)).await?;

    // Use fixed S3 key for consistent retrieval
    let s3_key = "examples/add.tar.zst";

    // Upload via RPC
    let response = rpc_client
        .write_binary(
            archive_data,
            s3_key,
            "application/zstd",
            Some(hash.clone()),
            std::collections::HashMap::new(),
        )
        .await?;

    if response.success {
        println!("✅ Archive uploaded successfully!");
        println!("   S3 Key: {}", s3_key);
        println!("   Hash: {}", response.sha256);
        println!("   S3 URL: {}", response.s3_url);
        println!("");
        println!("📥 To download and unpack this archive later:");
        println!("   Use the S3 key: {}", s3_key);
    } else {
        anyhow::bail!("Failed to upload archive: {}", response.message);
    }

    Ok(())
}
