use crate::cli::TransactionType;
use crate::error::{CoordinatorError, Result};
use anyhow::anyhow;
use tracing::{error, info, warn};
use tracing_subscriber::prelude::*;

pub async fn handle_transaction_command(
    rpc_url: Option<String>,
    private_key: Option<String>,
    tx_type: TransactionType,
    chain_override: Option<String>,
) -> Result<()> {
    // Initialize minimal logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("info"))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Resolve RPC URL and initialize Sui connection with optional private key
    let rpc_url = sui::resolve_rpc_url(rpc_url, chain_override.clone())
        .map_err(CoordinatorError::Other)?;
    match sui::SharedSuiState::initialize_with_optional_key(
        &rpc_url,
        private_key.as_deref(),
    )
    .await
    {
        Ok(()) => {}
        Err(e) => {
            error!("Failed to initialize Sui connection: {}", e);
            println!("❌ Failed to initialize Sui connection");
            println!("Error: {}", e);
            return Err(e.into());
        }
    }

    // Initialize gas coin pool for better transaction performance (like in start command)
    info!("🪙 Initializing gas coin pool...");
    match sui::coin_management::initialize_gas_coin_pool().await {
        Ok(()) => info!("✅ Gas coin pool initialized"),
        Err(e) => {
            warn!(
                "⚠️ Failed to initialize gas coin pool: {}. Continuing anyway.",
                e
            );
            println!("⚠️ Warning: Failed to initialize gas coin pool");
            println!("This may cause transaction failures. Error: {}", e);
        }
    }

    // Create interface
    let mut interface = sui::interface::SilvanaSuiInterface::new();

    // Execute the requested transaction
    match tx_type {
        TransactionType::TerminateJob { instance, job, gas } => {
            println!(
                "Terminating job {} in instance {} (gas: {} SUI)",
                job, instance, gas
            );

            match interface.terminate_job(&instance, job, gas).await {
                Ok(tx_digest) => {
                    println!("✅ Transaction executed successfully");
                    println!("Transaction digest: {}", tx_digest);
                    println!("Job {} has been terminated", job);
                }
                Err((error_msg, tx_digest)) => {
                    println!("❌ Transaction execution failed");
                    if let Some(digest) = tx_digest {
                        println!("Transaction digest: {}", digest);
                        println!(
                            "Note: Transaction was submitted but failed during execution"
                        );
                    } else {
                        println!("Transaction was not submitted to the blockchain");
                    }
                    println!("Error: {}", error_msg);
                    return Err(anyhow!("Transaction failed").into());
                }
            }
        }

        TransactionType::RestartFailedJob { instance, job, gas } => {
            println!(
                "Restarting failed job {} in instance {} (gas: {} SUI)",
                job, instance, gas
            );

            match interface.restart_failed_job(&instance, job, gas).await {
                Ok(tx_digest) => {
                    println!("✅ Transaction executed successfully");
                    println!("Transaction digest: {}", tx_digest);
                    println!("Failed job {} has been restarted", job);
                }
                Err((error_msg, tx_digest)) => {
                    println!("❌ Transaction execution failed");
                    if let Some(digest) = tx_digest {
                        println!("Transaction digest: {}", digest);
                        println!(
                            "Note: Transaction was submitted but failed during execution"
                        );
                    } else {
                        println!("Transaction was not submitted to the blockchain");
                    }
                    println!("Error: {}", error_msg);
                    return Err(anyhow!("Transaction failed").into());
                }
            }
        }

        TransactionType::RestartFailedJobs { instance, gas } => {
            println!(
                "Restarting all failed jobs in instance {} (gas: {} SUI)",
                instance, gas
            );

            match interface.restart_failed_jobs(&instance, gas).await {
                Ok(tx_digest) => {
                    println!("✅ Transaction executed successfully");
                    println!("Transaction digest: {}", tx_digest);
                    println!("All failed jobs have been restarted");
                }
                Err((error_msg, tx_digest)) => {
                    println!("❌ Transaction execution failed");
                    if let Some(digest) = tx_digest {
                        println!("Transaction digest: {}", digest);
                        println!(
                            "Note: Transaction was submitted but failed during execution"
                        );
                    } else {
                        println!("Transaction was not submitted to the blockchain");
                    }
                    println!("Error: {}", error_msg);
                    return Err(anyhow!("Transaction failed").into());
                }
            }
        }

        TransactionType::RejectProof {
            instance,
            block,
            sequences,
            gas: _,
        } => {
            // Parse the sequences string into a vector of u64
            let seq_vec: Vec<u64> = sequences
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| {
                    s.parse::<u64>().map_err(|e| {
                        anyhow!("Invalid sequence number '{}': {}", s, e)
                    })
                })
                .collect::<std::result::Result<Vec<_>, _>>()?;

            if seq_vec.is_empty() {
                println!("❌ No sequences provided");
                println!(
                    "Expected comma-separated numbers, e.g., '1,2,3' or '1430,1431,1432'"
                );
                return Err(anyhow!("No sequences provided").into());
            }

            println!(
                "Rejecting proof for block {} with sequences {:?} in instance {}",
                block, seq_vec, instance
            );

            match interface
                .reject_proof(&instance, block, seq_vec.clone())
                .await
            {
                Ok(tx_digest) => {
                    println!("✅ Transaction executed successfully");
                    println!("Transaction digest: {}", tx_digest);
                    println!(
                        "Proof for sequences {:?} in block {} has been rejected",
                        seq_vec, block
                    );
                }
                Err(e) => {
                    println!("❌ Transaction execution failed");
                    println!("Error: {}", e);
                    return Err(anyhow!("Transaction failed").into());
                }
            }
        }

        TransactionType::Purge {
            instance,
            sequences,
            gas,
        } => {
            if let Some(gas_budget) = gas {
                println!(
                    "Purging up to {} sequences from instance {} (gas: {} SUI)",
                    sequences, instance, gas_budget
                );
            } else {
                println!(
                    "Purging up to {} sequences from instance {} (default gas)",
                    sequences, instance
                );
            }

            match interface.purge(&instance, sequences, gas).await {
                Ok(tx_digest) => {
                    println!("✅ Transaction executed successfully");
                    println!("Transaction digest: {}", tx_digest);
                    println!("Successfully purged up to {} sequences", sequences);
                }
                Err((error_msg, tx_digest)) => {
                    println!("❌ Transaction execution failed");
                    if let Some(digest) = tx_digest {
                        println!("Transaction digest: {}", digest);
                        println!(
                            "Note: Transaction was submitted but failed during execution"
                        );
                    } else {
                        println!("Transaction was not submitted to the blockchain");
                    }
                    println!("Error: {}", error_msg);
                    return Err(anyhow!("Transaction failed").into());
                }
            }
        }
    }

    Ok(())
}