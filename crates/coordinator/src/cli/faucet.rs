use crate::cli::FaucetCommands;
use crate::constants;
use crate::error::{CoordinatorError, Result};
use anyhow::anyhow;
use tracing::info;
use tracing_subscriber::prelude::*;

pub async fn handle_faucet_command(subcommand: FaucetCommands) -> Result<()> {
    // Initialize minimal logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("info"))
        .with(tracing_subscriber::fmt::layer())
        .init();

    match subcommand {
        FaucetCommands::Sui { address, amount } => {
            // Get the chain from environment
            let chain = std::env::var("SUI_CHAIN")
                .unwrap_or_else(|_| "devnet".to_string())
                .to_lowercase();

            // Check if mainnet (no faucet available)
            if chain == "mainnet" {
                tracing::error!("❌ Faucet is not available for mainnet");
                tracing::error!("   Please acquire SUI tokens through an exchange or other means");
                return Err(anyhow!("Faucet not available for mainnet").into());
            }

            // Validate amount
            if amount > 10.0 {
                tracing::error!("❌ Amount exceeds maximum of 10 SUI");
                tracing::error!("   Maximum faucet amount is 10 SUI per request");
                return Err(anyhow!("Amount exceeds maximum of 10 SUI").into());
            }

            if amount <= 0.0 {
                tracing::error!("❌ Amount must be greater than 0");
                return Err(anyhow!("Invalid amount").into());
            }

            // Get the address to fund
            let target_address = address.unwrap_or_else(|| {
                std::env::var("SUI_ADDRESS").unwrap_or_else(|_| {
                    tracing::error!("❌ No address provided and SUI_ADDRESS not set");
                    std::process::exit(1);
                })
            });

            // Validate address format
            if !target_address.starts_with("0x") || target_address.len() != 66 {
                tracing::error!("❌ Invalid SUI address format: {}", target_address);
                tracing::error!("   Address should start with '0x' and be 66 characters long");
                return Err(anyhow!("Invalid address format").into());
            }

            info!("💧 Requesting {} SUI from {} faucet...", amount, chain);
            info!("📍 Target address: {}", target_address);

            // Get RPC URL based on chain using the resolver
            let rpc_url = sui::resolve_rpc_url(None, Some(chain.clone()))?;

            // Initialize Sui connection to check balance
            sui::SharedSuiState::initialize(&rpc_url)
                .await
                .map_err(CoordinatorError::Other)?;

            // Check balance before faucet
            info!("📊 Balance before faucet:");
            let balance_before = sui::get_balance_in_sui(&target_address)
                .await
                .map_err(CoordinatorError::Other)?;
            info!("   {:.4} SUI", balance_before);

            // Call the faucet
            info!("🚰 Calling faucet...");
            let faucet_result =
                sui::request_tokens_from_faucet(&chain, &target_address, Some(amount)).await;

            match faucet_result {
                Ok(tx_digest) => {
                    info!("✅ Faucet request sent!");

                    // Handle the case where transaction digest is not immediately available
                    if tx_digest != "unknown" {
                        info!("   Transaction: {}", tx_digest);
                        info!(
                            "   🔗 Explorer: https://suiscan.xyz/{}/tx/{}",
                            chain, tx_digest
                        );
                    } else {
                        info!(
                            "   🔗 Explorer: https://suiscan.xyz/{}/account/{}",
                            chain, target_address
                        );
                    }

                    // Wait for transaction to be processed
                    info!(
                        "⏳ Waiting {} seconds for transaction to be processed...",
                        constants::CLI_TRANSACTION_WAIT_SECS
                    );
                    tokio::time::sleep(tokio::time::Duration::from_secs(
                        constants::CLI_TRANSACTION_WAIT_SECS,
                    ))
                    .await;

                    // Check balance after faucet
                    info!("📊 Balance after faucet:");
                    let balance_after = sui::get_balance_in_sui(&target_address)
                        .await
                        .map_err(CoordinatorError::Other)?;
                    info!("   {:.4} SUI", balance_after);

                    let received = balance_after - balance_before;
                    if received > 0.0 {
                        info!("💰 Received: {:.4} SUI", received);
                    }
                }
                Err(e) => {
                    tracing::error!("❌ Faucet failed: {}", e);
                    return Err(e.into());
                }
            }
        }

        FaucetCommands::Mina { address, network } => {
            // Validate the address format
            if !mina::validate_mina_address(&address) {
                tracing::error!("❌ Invalid Mina address format");
                tracing::error!(
                    "   Mina addresses should start with 'B62' and be at least 55 characters"
                );
                return Err(anyhow!("Invalid Mina address format").into());
            }

            // Validate network - the network module will handle validation
            if mina::MinaNetwork::get_network(&network).is_none() {
                tracing::error!("❌ Invalid network '{}'", network);
                tracing::error!(
                    "   Supported networks: mina:devnet, zeko:testnet (or devnet, zeko for short)"
                );
                return Err(anyhow!("Invalid network").into());
            }

            info!("💧 Requesting MINA from {} faucet...", network);
            info!("📍 Target address: {}", address);

            // Call the Mina faucet
            match mina::request_mina_from_faucet(&address, &network).await {
                Ok(response) => {
                    if let Some(status) = &response.status {
                        if status == "rate-limit" {
                            tracing::error!("⚠️  Rate limited by faucet");
                            tracing::error!("   Please wait 30 minutes before trying again");
                            return Err(anyhow!("Rate limited by faucet").into());
                        }
                    }

                    if let Some(error) = &response.error {
                        tracing::error!("❌ Faucet error: {}", error);
                        return Err(anyhow!("Faucet error: {}", error).into());
                    }

                    info!("✅ Faucet request successful!");
                    if let Some(message) = &response.message {
                        info!("   Response: {}", message);
                    }

                    info!(
                        "⏳ Note: It may take a few minutes for the funds to appear in your account"
                    );

                    // Get explorer URL from network config
                    if let Some(network_config) = mina::MinaNetwork::get_network(&network) {
                        if let Some(explorer_url) = network_config.explorer_account_url {
                            info!("   Check your balance at:");
                            info!("   {}{}", explorer_url, address);
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("❌ Faucet request failed: {}", e);
                    return Err(e.into());
                }
            }
        }
    }

    Ok(())
}