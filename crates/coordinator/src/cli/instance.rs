use crate::error::{CoordinatorError, Result};
use anyhow::anyhow;
use chrono::{DateTime, Utc};
use tracing::error;
use tracing_subscriber::prelude::*;

pub async fn handle_instance_command(
    rpc_url: Option<String>,
    instance: String,
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

    // Fetch and display the app instance
    match sui::fetch::fetch_app_instance(&instance).await {
        Ok(app_instance) => {
            // Convert timestamps to ISO format
            let previous_block_timestamp_iso = DateTime::<Utc>::from_timestamp_millis(
                app_instance.previous_block_timestamp as i64,
            )
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_else(|| app_instance.previous_block_timestamp.to_string());

            let created_at_iso =
                DateTime::<Utc>::from_timestamp_millis(app_instance.created_at as i64)
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_else(|| app_instance.created_at.to_string());

            // Print formatted app instance
            println!("AppInstance {{");
            println!("    id: \"{}\",", app_instance.id);
            println!(
                "    silvana_app_name: \"{}\",",
                app_instance.silvana_app_name
            );

            if let Some(ref desc) = app_instance.description {
                println!("    description: Some(\"{}\"),", desc);
            } else {
                println!("    description: None,");
            }

            // Print metadata
            if !app_instance.metadata.is_empty() {
                println!("    metadata: {{");
                for (key, value) in &app_instance.metadata {
                    println!("        \"{}\": \"{}\",", key, value);
                }
                println!("    }},");
            } else {
                println!("    metadata: {{}},");
            }

            // Print kv
            if !app_instance.kv.is_empty() {
                println!("    kv: {{");
                for (key, value) in &app_instance.kv {
                    println!("        \"{}\": \"{}\",", key, value);
                }
                println!("    }},");
            } else {
                println!("    kv: {{}},");
            }

            // Print methods (as JSON)
            println!(
                "    methods: {},",
                serde_json::to_string(&app_instance.methods)
                    .unwrap_or_else(|_| "{}".to_string())
            );

            // Print state (as JSON)
            println!(
                "    state: {},",
                serde_json::to_string(&app_instance.state)
                    .unwrap_or_else(|_| "{}".to_string())
            );

            println!("    blocks_table_id: \"{}\",", app_instance.blocks_table_id);
            println!(
                "    proof_calculations_table_id: \"{}\",",
                app_instance.proof_calculations_table_id
            );

            // Print sequence_state_manager (as JSON)
            println!(
                "    sequence_state_manager: {},",
                serde_json::to_string(&app_instance.sequence_state_manager)
                    .unwrap_or_else(|_| "{}".to_string())
            );

            // Print jobs if present
            if let Some(ref jobs) = app_instance.jobs {
                println!("    jobs: Some(Jobs {{");
                println!("        id: \"{}\",", jobs.id);
                println!("        jobs_table_id: \"{}\",", jobs.jobs_table_id);
                println!(
                    "        total_jobs_count: {}, // Total jobs in ObjectTable (pending + running + failed)",
                    jobs.total_jobs_count
                );
                // Note: failed_jobs_table_id no longer exists - failed jobs are in main jobs table
                println!("        failed_jobs_count: {},", jobs.failed_jobs_count);
                println!("        failed_jobs_index: {:?},", jobs.failed_jobs_index);
                println!("        pending_jobs: {:?},", jobs.pending_jobs);
                println!("        pending_jobs_count: {},", jobs.pending_jobs_count);

                // Calculate and display running jobs count
                let running_jobs_count = jobs
                    .total_jobs_count
                    .saturating_sub(jobs.pending_jobs_count + jobs.failed_jobs_count);
                println!(
                    "        running_jobs_count: {}, // Calculated: total - pending - failed",
                    running_jobs_count
                );

                // Display pending_jobs_indexes in a readable format
                println!("        pending_jobs_indexes: {{");
                for (developer, agents) in &jobs.pending_jobs_indexes {
                    println!("            \"{}\": {{", developer);
                    for (agent, methods) in agents {
                        println!("                \"{}\": {{", agent);
                        for (method, job_ids) in methods {
                            println!("                    \"{}\": {:?},", method, job_ids);
                        }
                        println!("                }},");
                    }
                    println!("            }},");
                }
                println!("        }},");

                println!("        next_job_sequence: {},", jobs.next_job_sequence);
                println!("        max_attempts: {},", jobs.max_attempts);
                // Settlement jobs are now per-chain in settlements
                println!("    }}),");
            } else {
                println!("    jobs: None,");
            }

            println!("    sequence: {},", app_instance.sequence);
            println!("    admin: \"{}\",", app_instance.admin);
            println!("    block_number: {},", app_instance.block_number);
            println!(
                "    previous_block_timestamp: \"{}\",",
                previous_block_timestamp_iso
            );
            println!(
                "    previous_block_last_sequence: {},",
                app_instance.previous_block_last_sequence
            );
            println!(
                "    previous_block_actions_state: \"{}\",",
                app_instance.previous_block_actions_state
            );
            println!(
                "    last_proved_block_number: {},",
                app_instance.last_proved_block_number
            );
            println!(
                "    last_settled_block_number: {},",
                app_instance.last_settled_block_number
            );
            println!(
                "    last_settled_sequence: {},",
                app_instance.last_settled_sequence
            );
            println!(
                "    last_purged_sequence: {},",
                app_instance.last_purged_sequence
            );
            // Print settlement data for every chain
            println!("    settlements: {{");
            for (chain, settlement) in &app_instance.settlements {
                println!("        \"{}\": Settlement {{", chain);
                println!("            chain: \"{}\",", settlement.chain);
                println!(
                    "            last_settled_block_number: {},",
                    settlement.last_settled_block_number
                );
                if let Some(ref addr) = settlement.settlement_address {
                    println!("            settlement_address: Some(\"{}\"),", addr);
                } else {
                    println!("            settlement_address: None,");
                }
                if let Some(job_id) = settlement.settlement_job {
                    println!("            settlement_job: Some({}),", job_id);
                } else {
                    println!("            settlement_job: None,");
                }
                println!(
                    "            block_settlements_table_id: {:?},",
                    settlement.block_settlements_table_id
                );
                println!("        }},");
            }
            println!("    }},");

            // Print settlement chains
            if !app_instance.settlements.is_empty() {
                println!("    settlement_chains: {{");
                for (chain, settlement) in &app_instance.settlements {
                    println!("        \"{}\": {{", chain);
                    println!(
                        "            last_settled_block_number: {},",
                        settlement.last_settled_block_number
                    );
                    if let Some(ref addr) = settlement.settlement_address {
                        println!("            settlement_address: Some(\"{}\"),", addr);
                    } else {
                        println!("            settlement_address: None,");
                    }
                    println!("        }},");
                }
                println!("    }},");
            } else {
                println!("    settlement_chains: None,");
            }

            println!("    created_at: \"{}\",", created_at_iso);
            println!("}}");
        }
        Err(e) => {
            error!("Failed to fetch app instance {}: {}", instance, e);
            return Err(anyhow!("Failed to fetch app instance: {}", e).into());
        }
    }

    Ok(())
}