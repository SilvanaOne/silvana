use anyhow::Result;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tracing::{error, info, warn};

/// Setup example project after unpacking template
/// This includes:
/// - Creating Sui keypairs (agent and coordinator)
/// - Creating Mina keypairs (agent/app-admin and user)
/// - Funding all accounts from faucets
/// - Fetching devnet config
/// - Creating registry
/// - Writing configuration files
pub async fn setup_example_project(project_path: &Path, project_name: &str) -> Result<()> {
    info!("🔑 Setting up project credentials...");

    // Generate Sui keys
    info!("📍 Generating Sui keypairs...");
    let sui_user_key = generate_sui_keypair("user")?;
    let sui_coordinator_key = generate_sui_keypair("coordinator")?;

    // Generate Mina keys
    info!("📍 Generating Mina keypairs...");
    let mina_app_admin_key = generate_mina_keypair("app-admin")?;
    let mina_user_key = generate_mina_keypair("user")?;

    // Fund Sui accounts
    info!("💰 Funding Sui accounts from devnet faucet...");
    if let Err(e) = fund_sui_account(&sui_user_key.address, "user").await {
        warn!("Failed to fund Sui user account: {}", e);
        warn!("⚠️  You can manually fund the account later with:");
        warn!("   silvana faucet sui --address {}", sui_user_key.address);
    }

    // Small delay between faucet requests to avoid rate limiting
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    if let Err(e) = fund_sui_account(&sui_coordinator_key.address, "coordinator").await {
        warn!("Failed to fund Sui coordinator account: {}", e);
        warn!("⚠️  You can manually fund the account later with:");
        warn!(
            "   silvana faucet sui --address {}",
            sui_coordinator_key.address
        );
    }

    // Fund Mina accounts
    info!("💰 Funding Mina accounts from devnet faucet...");
    if let Err(e) = fund_mina_account(&mina_app_admin_key.public_key, "app-admin").await {
        warn!("Failed to fund Mina app-admin account: {}", e);
        warn!("⚠️  You can manually fund the account later with:");
        warn!(
            "   silvana faucet mina --address {}",
            mina_app_admin_key.public_key
        );
    }

    // Small delay between faucet requests
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    if let Err(e) = fund_mina_account(&mina_user_key.public_key, "user").await {
        warn!("Failed to fund Mina user account: {}", e);
        warn!("⚠️  You can manually fund the account later with:");
        warn!(
            "   silvana faucet mina --address {}",
            mina_user_key.public_key
        );
    }

    // Fetch devnet configuration
    info!("📥 Fetching devnet configuration...");
    let config = match crate::config::fetch_config("devnet").await {
        Ok(cfg) => {
            info!("✓ Fetched configuration successfully");
            cfg
        }
        Err(e) => {
            warn!("Failed to fetch devnet config: {}", e);
            warn!("⚠️  Using default values");
            HashMap::new()
        }
    };

    // Get SILVANA_REGISTRY_PACKAGE from config
    let (registry_package, registry_id) = if let Some(package) =
        config.get("SILVANA_REGISTRY_PACKAGE")
    {
        let package = package.clone();
        info!("📝 Creating Silvana registry...");
        match create_registry(&sui_user_key, project_name, Some(package.clone())).await {
            Ok((id, tx_digest)) => {
                info!("   ✓ Registry created successfully");
                info!("   📄 Registry ID: {}", id);
                info!("   📄 Transaction: {}", tx_digest);
                info!(
                    "   🔗 Explorer: https://suiscan.xyz/devnet/tx/{}",
                    tx_digest
                );
                (Some(package), Some(id))
            }
            Err(e) => {
                warn!("Failed to create registry: {}", e);
                warn!("   ⚠️  Registry creation failed. You can create it manually later with:");
                warn!("      silvana registry create <name>");
                warn!("   Then update SILVANA_REGISTRY in your .env file with the registry ID");
                (Some(package), None)
            }
        }
    } else {
        warn!("⚠️  Cannot create registry: SILVANA_REGISTRY_PACKAGE not found in devnet config");
        warn!("   Once the package is available, you can create a registry with:");
        warn!("   silvana registry create registry <name>");
        warn!("   Then update SILVANA_REGISTRY in your .env file with the registry ID");
        (None, None)
    };

    // Create .env files
    create_env_files(
        project_path,
        &sui_user_key,
        &sui_coordinator_key,
        &mina_app_admin_key,
        &mina_user_key,
        &registry_package,
        &registry_id,
    )?;

    // Display summary
    display_setup_summary(
        &sui_user_key,
        &sui_coordinator_key,
        &mina_app_admin_key,
        &mina_user_key,
        &registry_id,
    );

    Ok(())
}

/// Sui keypair structure
#[derive(Debug)]
struct SuiKeypair {
    private_key: String,
    address: String,
}

/// Mina keypair structure  
#[derive(Debug)]
struct MinaKeypair {
    private_key: String,
    public_key: String,
}

/// Generate a Sui Ed25519 keypair
fn generate_sui_keypair(name: &str) -> Result<SuiKeypair> {
    info!("   • Generating Sui {} keypair...", name);

    match sui::keypair::generate_ed25519() {
        Ok(keypair) => {
            info!("     ✓ Sui {} address: {}", name, keypair.address);
            Ok(SuiKeypair {
                private_key: keypair.sui_private_key,
                address: keypair.address.to_string(),
            })
        }
        Err(e) => {
            error!("Failed to generate Sui {} keypair: {}", name, e);
            Err(anyhow::anyhow!(
                "Failed to generate Sui {} keypair: {}",
                name,
                e
            ))
        }
    }
}

/// Generate a Mina keypair
fn generate_mina_keypair(name: &str) -> Result<MinaKeypair> {
    info!("   • Generating Mina {} keypair...", name);

    match mina::generate_mina_keypair() {
        Ok(keypair) => {
            info!(
                "     ✓ Mina {} address: {}...{}",
                name,
                &keypair.public_key[..8],
                &keypair.public_key[keypair.public_key.len() - 4..]
            );
            Ok(MinaKeypair {
                private_key: keypair.private_key,
                public_key: keypair.public_key,
            })
        }
        Err(e) => {
            error!("Failed to generate Mina {} keypair: {}", name, e);
            Err(anyhow::anyhow!(
                "Failed to generate Mina {} keypair: {}",
                name,
                e
            ))
        }
    }
}

/// Fund a Sui account from devnet faucet
async fn fund_sui_account(address: &str, account_type: &str) -> Result<()> {
    info!("Funding Sui {} account: {}", account_type, address);

    // Request 1 SUI from faucet (reasonable amount for testing)
    match sui::request_tokens_from_faucet("devnet", address, Some(1.0)).await {
        Ok(tx_digest) => {
            if tx_digest != "unknown" {
                info!("✓ Faucet request sent");
                info!("📄 Transaction: {}", tx_digest);
                info!("🔗 Explorer: https://suiscan.xyz/devnet/tx/{}", tx_digest);
            } else {
                info!("✓ Faucet request sent (transaction pending)");
                info!(
                    "🔗 Explorer: https://suiscan.xyz/devnet/account/{}",
                    address
                );
            }
            Ok(())
        }
        Err(e) => Err(anyhow::anyhow!("Faucet request failed: {}", e)),
    }
}

/// Fund a Mina account from devnet faucet
async fn fund_mina_account(address: &str, account_type: &str) -> Result<()> {
    info!(
        "Funding Mina {} account: {}...{}",
        account_type,
        &address[..8],
        &address[address.len() - 4..]
    );

    match mina::request_mina_from_faucet(address, "mina:devnet").await {
        Ok(response) => {
            if let Some(status) = &response.status {
                if status == "rate-limit" {
                    return Err(anyhow::anyhow!("Rate limited by faucet"));
                }
            }

            if let Some(error) = &response.error {
                return Err(anyhow::anyhow!("Faucet error: {}", error));
            }

            info!("✓ Faucet request sent successfully");
            if let Some(message) = &response.message {
                info!("📄 Response: {}", message);
            }
            info!(
                "🔗 Explorer: https://minascan.io/devnet/account/{}",
                address
            );
            Ok(())
        }
        Err(e) => Err(anyhow::anyhow!("Faucet request failed: {}", e)),
    }
}

/// Create registry using user's Sui key
async fn create_registry(
    sui_user: &SuiKeypair,
    project_name: &str,
    package_id: Option<String>,
) -> Result<(String, String)> {
    // Set the user's private key for the transaction BEFORE any initialization
    unsafe {
        std::env::set_var("SUI_SECRET_KEY", &sui_user.private_key);
        std::env::set_var("SUI_ADDRESS", &sui_user.address);
        std::env::set_var("SUI_CHAIN", "devnet");
    }

    // Initialize Sui connection only if not already initialized
    if !sui::SharedSuiState::is_initialized() {
        let rpc_url = sui::resolve_rpc_url(None, Some("devnet".to_string()))?;
        sui::SharedSuiState::initialize(&rpc_url).await?;
    }

    // Create interface
    let mut interface = sui::SilvanaSuiInterface::new();

    // Create the registry
    match interface
        .create_silvana_registry(project_name.to_string(), package_id)
        .await
    {
        Ok(result) => Ok((result.registry_id, result.tx_digest)),
        Err(e) => Err(anyhow::anyhow!("Failed to create registry: {}", e)),
    }
}

/// Create .env files for agent and silvana directories
fn create_env_files(
    project_path: &Path,
    sui_user: &SuiKeypair,
    sui_coordinator: &SuiKeypair,
    mina_app_admin: &MinaKeypair,
    mina_user: &MinaKeypair,
    registry_package: &Option<String>,
    registry_id: &Option<String>,
) -> Result<()> {
    let registry_package = registry_package.as_ref().map(|s| s.as_str()).unwrap_or("");
    let registry_id = registry_id.as_ref().map(|s| s.as_str()).unwrap_or("");
    // Read the env.example template if it exists
    let agent_env_example_path = project_path.join("agent").join("env.example");
    let env_template = if agent_env_example_path.exists() {
        fs::read_to_string(&agent_env_example_path)?
    } else {
        // Default template if env.example doesn't exist
        String::from(
            "#\nSUI_CHAIN=devnet\nSUI_ADDRESS=\nSUI_SECRET_KEY=\nSILVANA_REGISTRY_PACKAGE=\nSILVANA_REGISTRY=\nAPP_PACKAGE_ID=\n\n# Mina\nMINA_PRIVATE_KEY=\nMINA_PUBLIC_KEY=\nMINA_APP_ADMIN=\nMINA_APP_ADMIN_PRIVATE_KEY=\n\n# Docker\nDOCKER_USERNAME=\nDOCKER_PASSWORD=\nIMAGE_NAME=add\nDOCKER_ACCESS_TOKEN=",
        )
    };

    // Create agent/.env from template
    let agent_env_path = project_path.join("agent").join(".env");
    let agent_env = env_template
        .replace("SUI_ADDRESS=", &format!("SUI_ADDRESS={}", sui_user.address))
        .replace(
            "SUI_SECRET_KEY=",
            &format!("SUI_SECRET_KEY={}", sui_user.private_key),
        )
        .replace(
            "SILVANA_REGISTRY_PACKAGE=",
            &format!("SILVANA_REGISTRY_PACKAGE={}", &registry_package),
        )
        .replace(
            "SILVANA_REGISTRY=",
            &format!("SILVANA_REGISTRY={}", &registry_id),
        )
        .replace(
            "MINA_PRIVATE_KEY=",
            &format!("MINA_PRIVATE_KEY={}", mina_user.private_key),
        )
        .replace(
            "MINA_PUBLIC_KEY=",
            &format!("MINA_PUBLIC_KEY={}", mina_user.public_key),
        )
        .replace(
            "MINA_APP_ADMIN=",
            &format!("MINA_APP_ADMIN={}", mina_app_admin.public_key),
        )
        .replace(
            "MINA_APP_ADMIN_PRIVATE_KEY=",
            &format!("MINA_APP_ADMIN_PRIVATE_KEY={}", mina_app_admin.private_key),
        );

    fs::write(agent_env_path, agent_env)?;

    // Create silvana subfolder if it doesn't exist
    let silvana_dir = project_path.join("silvana");
    if !silvana_dir.exists() {
        fs::create_dir(&silvana_dir)?;
    }

    // Create silvana/.env with coordinator key
    let silvana_env_path = silvana_dir.join(".env");
    let silvana_env = format!(
        "# Silvana Coordinator Configuration\n\
        \n\
        # Sui Configuration\n\
        SUI_SECRET_KEY={}\n\
        SUI_ADDRESS={}\n\
        SUI_CHAIN=devnet\n\
        \n\
        # Registry Configuration\n\
        SILVANA_REGISTRY_PACKAGE={}\n\
        SILVANA_REGISTRY={}\n",
        sui_coordinator.private_key, sui_coordinator.address, &registry_package, &registry_id
    );
    fs::write(silvana_env_path, silvana_env)?;

    Ok(())
}

/// Display setup summary
fn display_setup_summary(
    sui_user: &SuiKeypair,
    sui_coordinator: &SuiKeypair,
    mina_app_admin: &MinaKeypair,
    mina_user: &MinaKeypair,
    registry_id: &Option<String>,
) {
    info!("✅ Project setup complete!");
    info!("📋 Generated Credentials:");
    info!("   ├── Sui:");
    info!("   │   ├── User: {}", sui_user.address);
    info!("   │   └── Coordinator: {}", sui_coordinator.address);
    info!("   ├── Mina:");
    info!("   │   ├── App Admin: {}", mina_app_admin.public_key);
    info!("   │   └── User: {}", mina_user.public_key);
    if let Some(id) = registry_id {
        if id.len() >= 8 {
            info!(
                "   └── Registry: {}...{}",
                &id[..8],
                &id[id.len().saturating_sub(4)..]
            );
        } else {
            info!("   └── Registry: {}", id);
        }
    } else {
        info!("   └── Registry: (not created)");
    }
    info!("⚙️  Environment files created:");
    info!("   • agent/.env (from env.example template)");
    info!("   • silvana/.env (coordinator configuration)");
}
