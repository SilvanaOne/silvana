use crate::error::Result;
use crate::example;
use anyhow::anyhow;
use tracing::{error, info, warn};
use tracing_subscriber::prelude::*;

pub async fn handle_new_command(name: String, force: bool) -> Result<()> {
    // Initialize minimal logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("info"))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Validate project name
    if name.trim().is_empty() {
        error!("Project name cannot be empty");
        return Err(anyhow!("Project name cannot be empty").into());
    }

    // Check for invalid characters in project name
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        error!("Invalid project name: {}", name);
        return Err(anyhow!("Project name contains invalid characters").into());
    }

    let current_dir = std::env::current_dir()
        .map_err(|e| anyhow!("Failed to get current directory: {}", e))?;
    let project_path = current_dir.join(&name);

    // Check if folder exists
    if project_path.exists() && !force {
        error!(
            "Folder '{}' already exists. Use --force to overwrite.",
            name
        );
        return Err(anyhow!(
            "Folder '{}' already exists. Use --force to overwrite.",
            name
        )
        .into());
    }

    info!("🚀 Creating new Silvana project: {}", name);

    // Create the project directory
    if !project_path.exists() {
        std::fs::create_dir(&project_path)
            .map_err(|e| anyhow!("Failed to create project directory: {}", e))?;
    } else if force {
        warn!("⚠️  Overwriting existing folder: {}", name);
        // Clean the existing directory
        std::fs::remove_dir_all(&project_path)
            .map_err(|e| anyhow!("Failed to remove existing directory: {}", e))?;
        std::fs::create_dir(&project_path)
            .map_err(|e| anyhow!("Failed to recreate project directory: {}", e))?;
    }

    info!("📥 Downloading project template...");

    // Get RPC endpoint from environment or use default
    let rpc_endpoint = std::env::var("SILVANA_RPC_SERVER")
        .unwrap_or_else(|_| "https://rpc.silvana.dev".to_string());

    // Initialize RPC client
    let mut rpc_client =
        rpc_client::SilvanaRpcClient::new(rpc_client::RpcClientConfig::new(&rpc_endpoint))
            .await
            .map_err(|e| anyhow!("Failed to connect to RPC server: {}", e))?;

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
                .map_err(|e| anyhow!("Failed to write temporary archive: {}", e))?;

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
                    return Err(anyhow!("Failed to extract template: {}", e).into());
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
            return Err(anyhow!(
                "Failed to download template: {}",
                response.message
            )
            .into());
        }
        Err(e) => {
            error!("Failed to download template: {}", e);
            // Clean up the created directory
            let _ = std::fs::remove_dir_all(&project_path);
            return Err(anyhow!("Failed to download template: {}", e).into());
        }
    }

    Ok(())
}