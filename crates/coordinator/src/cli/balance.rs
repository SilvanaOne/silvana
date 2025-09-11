use crate::cli::BalanceCommands;
use crate::error::{CoordinatorError, Result};
use anyhow::anyhow;
use tracing::info;
use tracing_subscriber::prelude::*;

pub async fn handle_balance_command(
    subcommand: BalanceCommands,
    chain_override: Option<String>,
) -> Result<()> {
    // Initialize minimal logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("info"))
        .with(tracing_subscriber::fmt::layer())
        .init();

    match subcommand {
        BalanceCommands::Sui { rpc_url, address } => {
            // Resolve and initialize Sui connection
            let rpc_url = sui::resolve_rpc_url(rpc_url, chain_override)
                .map_err(CoordinatorError::Other)?;
            sui::SharedSuiState::initialize(&rpc_url)
                .await
                .map_err(CoordinatorError::Other)?;

            // Show the balance, passing the optional address
            sui::print_balance_info(address.as_deref())
                .await
                .map_err(CoordinatorError::Other)?;
        }

        BalanceCommands::Mina { address, network } => {
            // Validate address format
            if !mina::validate_mina_address(&address) {
                eprintln!("❌ Error: Invalid Mina address format");
                eprintln!(
                    "   Mina addresses should start with 'B62' and be at least 55 characters"
                );
                return Err(anyhow!("Invalid Mina address format").into());
            }

            println!("📊 Checking balance for {} on {}", address, network);
            println!();

            // Check if account exists first
            match mina::account_exists(&address, &network).await {
                Ok(exists) => {
                    if !exists {
                        println!("⚠️  Account not found on {}", network);
                        println!(
                            "   The account may not have been created yet or may not exist on this network"
                        );

                        // Show explorer link if available
                        if let Some(network_config) = mina::MinaNetwork::get_network(&network) {
                            if let Some(explorer_url) = network_config.explorer_account_url {
                                println!("   Check explorer: {}{}", explorer_url, address);
                            }
                        }
                        return Ok(());
                    }
                }
                Err(e) => {
                    eprintln!("⚠️  Warning: Could not verify account existence: {}", e);
                }
            }

            // Fetch balance
            match mina::get_balance_in_mina(&address, &network).await {
                Ok(balance) => {
                    println!("💰 Balance: {} MINA", balance);

                    // Also fetch account info for more details
                    match mina::get_account_info(&address, &network).await {
                        Ok(info) => {
                            println!("   Nonce: {}", info.nonce);
                            if let Some(symbol) = info.token_symbol {
                                println!("   Token Symbol: {}", symbol);
                            }
                        }
                        Err(_) => {
                            // Ignore error, we already have the balance
                        }
                    }

                    // Show explorer link if available
                    if let Some(network_config) = mina::MinaNetwork::get_network(&network) {
                        if let Some(explorer_url) = network_config.explorer_account_url {
                            println!();
                            println!("🔍 View on explorer:");
                            println!("   {}{}", explorer_url, address);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("❌ Failed to fetch balance: {}", e);
                    return Err(e.into());
                }
            }
        }

        BalanceCommands::Ethereum { address, network } => {
            println!("📊 Checking balance for {} on {}", address, network);
            println!();

            // Get network configuration to display native currency
            let network_config = ethereum::EthereumNetwork::get_network(&network);
            let symbol = network_config
                .map(|n| n.native_currency.symbol.clone())
                .unwrap_or_else(|| "ETH".to_string());

            // Fetch balance
            match ethereum::get_balance(&address, &network).await {
                Ok(balance) => {
                    println!("💰 Balance: {} {}", balance, symbol);

                    // Also fetch account info for more details
                    match ethereum::get_account_info(&address, &network).await {
                        Ok(info) => {
                            println!("   Nonce: {}", info.nonce);
                            println!("   Network: {}", info.network);
                        }
                        Err(_) => {
                            // Ignore error, we already have the balance
                        }
                    }

                    // Show explorer link if available
                    if let Some(network_config) = network_config {
                        if let Some(explorer_url) = &network_config.explorer {
                            println!();
                            println!("🔍 View on explorer:");
                            println!("   {}/address/{}", explorer_url, address);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("❌ Failed to fetch balance: {}", e);
                    eprintln!();
                    eprintln!("   Available networks:");
                    for net in ethereum::EthereumNetwork::list_networks() {
                        eprintln!("   • {}", net);
                    }
                    return Err(e.into());
                }
            }
        }
    }

    Ok(())
}