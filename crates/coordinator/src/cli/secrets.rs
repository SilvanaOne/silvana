use crate::cli::SecretsCommands;
use crate::error::Result;
use anyhow::anyhow;
use tracing::{error, info};
use tracing_subscriber::prelude::*;

pub async fn handle_secrets_command(subcommand: SecretsCommands) -> Result<()> {
    // Initialize minimal logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("info"))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Get default endpoint if not specified
    let get_endpoint = || {
        std::env::var("SILVANA_RPC_SERVER")
            .unwrap_or_else(|_| "https://rpc.silvana.dev".to_string())
    };

    match subcommand {
        SecretsCommands::Store {
            endpoint,
            developer,
            agent,
            name,
            secret,
            app,
            app_instance,
        } => {
            let endpoint = endpoint.unwrap_or_else(get_endpoint);

            info!("🔗 Connecting to RPC endpoint: {}", endpoint);

            let mut client = match secrets_client::SecretsClient::new(&endpoint).await {
                Ok(client) => client,
                Err(e) => {
                    error!("Failed to connect to RPC endpoint {}: {}", endpoint, e);
                    return Err(anyhow!("Failed to connect to RPC endpoint").into());
                }
            };

            info!("📦 Storing secret...");
            info!("  Developer: {}", developer);
            info!("  Agent: {}", agent);
            info!("  Name: {}", name);
            if let Some(ref app) = app {
                info!("  App: {}", app);
            }
            if let Some(ref app_instance) = app_instance {
                info!("  App Instance: {}", app_instance);
            }
            info!("  Secret length: {} characters", secret.len());

            // TODO: For now using empty signature - will be replaced with actual signature validation
            let placeholder_signature = vec![];

            match client
                .store_secret(
                    &developer,
                    &agent,
                    app.as_deref(),
                    app_instance.as_deref(),
                    Some(&name),
                    &secret,
                    &placeholder_signature,
                )
                .await
            {
                Ok(()) => {
                    info!("✅ Secret stored successfully!");
                }
                Err(e) => {
                    error!("Failed to store secret: {}", e);
                    return Err(anyhow!("Failed to store secret").into());
                }
            }
        }

        SecretsCommands::Retrieve {
            endpoint,
            developer,
            agent,
            name,
            app,
            app_instance,
        } => {
            let endpoint = endpoint.unwrap_or_else(get_endpoint);

            info!("🔗 Connecting to RPC endpoint: {}", endpoint);

            let mut client = match secrets_client::SecretsClient::new(&endpoint).await {
                Ok(client) => client,
                Err(e) => {
                    error!("Failed to connect to RPC endpoint {}: {}", endpoint, e);
                    return Err(anyhow!("Failed to connect to RPC endpoint").into());
                }
            };

            info!("🔍 Retrieving secret...");
            info!("  Developer: {}", developer);
            info!("  Agent: {}", agent);
            info!("  Name: {}", name);
            if let Some(ref app) = app {
                info!("  App: {}", app);
            }
            if let Some(ref app_instance) = app_instance {
                info!("  App Instance: {}", app_instance);
            }

            // TODO: For now using empty signature - will be replaced with actual signature validation
            let placeholder_signature = vec![];

            match client
                .retrieve_secret(
                    &developer,
                    &agent,
                    app.as_deref(),
                    app_instance.as_deref(),
                    Some(&name),
                    &placeholder_signature,
                )
                .await
            {
                Ok(secret_value) => {
                    info!("✅ Secret retrieved successfully!");
                    info!("📋 Secret value: {}", secret_value);
                    info!("📏 Secret length: {} characters", secret_value.len());
                }
                Err(secrets_client::SecretsClientError::SecretNotFound) => {
                    error!("❌ Secret not found with the specified parameters");
                    info!(
                        "💡 Try checking if the secret exists with different scope parameters (app, app-instance)"
                    );
                    return Err(anyhow!("Secret not found").into());
                }
                Err(e) => {
                    error!("Failed to retrieve secret: {}", e);
                    return Err(anyhow!("Failed to retrieve secret").into());
                }
            }
        }
    }

    Ok(())
}