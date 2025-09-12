use crate::error::{CoordinatorError, Result};
use anyhow::anyhow;
use chrono::{DateTime, Utc};
use tracing::error;
use tracing_subscriber::prelude::*;

pub async fn handle_block_command(
    rpc_url: Option<String>,
    instance: String,
    block: u64,
    chain_override: Option<String>,
) -> Result<()> {
    // Initialize minimal logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("warn"))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Resolve and initialize Sui connection (read-only mode)
    let rpc_url = sui::resolve_rpc_url(rpc_url, chain_override)
        .map_err(CoordinatorError::Other)?;
    sui::SharedSuiState::initialize_read_only(&rpc_url)
        .await
        .map_err(CoordinatorError::Other)?;

    // Fetch the app instance first
    let app_instance = match sui::fetch::fetch_app_instance(&instance).await {
        Ok(instance) => instance,
        Err(e) => {
            error!("Failed to fetch app instance {}: {}", instance, e);
            return Err(anyhow!("Failed to fetch app instance: {}", e).into());
        }
    };

    // Fetch and display the block
    match sui::fetch::fetch_block_info(&app_instance, block).await {
        Ok(Some(block_info)) => {
            // Convert commitments to hex
            let actions_commitment_hex = hex::encode(&block_info.actions_commitment);
            let state_commitment_hex = hex::encode(&block_info.state_commitment);
            let start_actions_commitment_hex =
                hex::encode(&block_info.start_actions_commitment);
            let end_actions_commitment_hex = hex::encode(&block_info.end_actions_commitment);

            // Convert timestamps to ISO format
            let created_iso =
                DateTime::<Utc>::from_timestamp_millis(block_info.created_at as i64)
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_else(|| block_info.created_at.to_string());
            let state_calculated_iso = block_info
                .state_calculated_at
                .and_then(|ts| DateTime::<Utc>::from_timestamp_millis(ts as i64))
                .map(|dt| dt.to_rfc3339());
            let proved_at_iso = block_info
                .proved_at
                .and_then(|ts| DateTime::<Utc>::from_timestamp_millis(ts as i64))
                .map(|dt| dt.to_rfc3339());

            // Print formatted block
            println!("Block {{");
            println!("    name: \"{}\",", block_info.name);
            println!("    block_number: {},", block_info.block_number);
            println!("    start_sequence: {},", block_info.start_sequence);
            println!("    end_sequence: {},", block_info.end_sequence);
            println!("    actions_commitment: \"{}\",", actions_commitment_hex);
            println!("    state_commitment: \"{}\",", state_commitment_hex);
            println!(
                "    time_since_last_block: {},",
                block_info.time_since_last_block
            );
            println!(
                "    number_of_transactions: {},",
                block_info.number_of_transactions
            );
            println!(
                "    start_actions_commitment: \"{}\",",
                start_actions_commitment_hex
            );
            println!(
                "    end_actions_commitment: \"{}\",",
                end_actions_commitment_hex
            );

            if let Some(ref data_avail) = block_info.state_data_availability {
                println!("    state_data_availability: Some(\"{}\"),", data_avail);
            } else {
                println!("    state_data_availability: None,");
            }

            if let Some(ref proof_avail) = block_info.proof_data_availability {
                println!("    proof_data_availability: Some(\"{}\"),", proof_avail);
            } else {
                println!("    proof_data_availability: None,");
            }

            println!("    created_at: \"{}\",", created_iso);

            if let Some(ref iso) = state_calculated_iso {
                println!("    state_calculated_at: Some(\"{}\"),", iso);
            } else {
                println!("    state_calculated_at: None,");
            }

            if let Some(ref iso) = proved_at_iso {
                println!("    proved_at: Some(\"{}\"),", iso);
            } else {
                println!("    proved_at: None,");
            }

            // Print settlement information for all chains
            println!("    block_settlements: {{");
            for (chain, settlement) in &app_instance.settlements {
                // Fetch the BlockSettlement from the ObjectTable
                let block_settlement_opt =
                    match sui::fetch::fetch_block_settlement(settlement, block).await {
                        Ok(bs) => bs,
                        Err(e) => {
                            eprintln!(
                                "Error fetching block settlement for chain {}: {}",
                                chain, e
                            );
                            None
                        }
                    };

                if let Some(block_settlement) = block_settlement_opt {
                    println!("        \"{}\": BlockSettlement {{", chain);
                    println!(
                        "            block_number: {},",
                        block_settlement.block_number
                    );

                    if let Some(ref tx_hash) = block_settlement.settlement_tx_hash {
                        println!("            settlement_tx_hash: Some(\"{}\"),", tx_hash);
                    } else {
                        println!("            settlement_tx_hash: None,");
                    }

                    println!(
                        "            settlement_tx_included_in_block: {},",
                        block_settlement.settlement_tx_included_in_block
                    );

                    if let Some(sent_at) = block_settlement.sent_to_settlement_at {
                        let sent_iso =
                            DateTime::<Utc>::from_timestamp_millis(sent_at as i64)
                                .map(|dt| dt.to_rfc3339())
                                .unwrap_or_else(|| sent_at.to_string());
                        println!(
                            "            sent_to_settlement_at: Some(\"{}\"),",
                            sent_iso
                        );
                    } else {
                        println!("            sent_to_settlement_at: None,");
                    }

                    if let Some(settled_at) = block_settlement.settled_at {
                        let settled_iso =
                            DateTime::<Utc>::from_timestamp_millis(settled_at as i64)
                                .map(|dt| dt.to_rfc3339())
                                .unwrap_or_else(|| settled_at.to_string());
                        println!("            settled_at: Some(\"{}\"),", settled_iso);
                    } else {
                        println!("            settled_at: None,");
                    }

                    println!("        }},");
                } else {
                    println!("        \"{}\": None,", chain);
                }
            }
            println!("    }},");
            println!("}}");
        }
        Ok(None) => {
            println!("Block {} not found", block);
        }
        Err(e) => {
            error!("Failed to fetch block {}: {}", block, e);
            return Err(anyhow!("Failed to fetch block: {}", e).into());
        }
    }

    Ok(())
}