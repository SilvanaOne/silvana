use crate::error::{CoordinatorError, Result};
use anyhow::anyhow;
use tracing::error;
use tracing_subscriber::prelude::*;

pub async fn handle_proofs_command(
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

    // Fetch and display the proof calculation
    match sui::fetch::fetch_proof_calculation(&app_instance, block).await {
        Ok(Some(proof_calc)) => {
            println!("ProofCalculation {{");
            println!("    id: \"{}\",", proof_calc.id);
            println!("    block_number: {},", proof_calc.block_number);
            println!("    start_sequence: {},", proof_calc.start_sequence);

            if let Some(end) = proof_calc.end_sequence {
                println!("    end_sequence: Some({}),", end);
            } else {
                println!("    end_sequence: None,");
            }

            println!("    proofs: [");

            // Sort proofs by first sequence number
            let mut sorted_proofs = proof_calc.proofs.clone();
            sorted_proofs.sort_by_key(|p| p.sequences.first().cloned().unwrap_or(0));

            for proof in &sorted_proofs {
                println!("        Proof {{");

                // Format status
                let status_str = match proof.status {
                    sui::fetch::ProofStatus::Used => "Used",
                    sui::fetch::ProofStatus::Calculated => "Calculated",
                    sui::fetch::ProofStatus::Reserved => "Reserved",
                    sui::fetch::ProofStatus::Started => "Started",
                    sui::fetch::ProofStatus::Rejected => "Rejected",
                };
                println!("            status: {},", status_str);

                // Format DA hash
                if let Some(ref hash) = proof.da_hash {
                    println!("            da_hash: Some(\"{}\"),", hash);
                } else {
                    println!("            da_hash: None,");
                }

                // Format sequence1 and sequence2 for merge proofs
                if let Some(ref seq1) = proof.sequence1 {
                    println!("            sequence1: Some({:?}),", seq1);
                } else {
                    println!("            sequence1: None,");
                }

                if let Some(ref seq2) = proof.sequence2 {
                    println!("            sequence2: Some({:?}),", seq2);
                } else {
                    println!("            sequence2: None,");
                }

                println!("            rejected_count: {},", proof.rejected_count);
                println!("            timestamp: {},", proof.timestamp);
                println!("            prover: \"{}\",", proof.prover);

                if let Some(ref user) = proof.user {
                    println!("            user: Some(\"{}\"),", user);
                } else {
                    println!("            user: None,");
                }

                println!("            job_id: \"{}\",", proof.job_id);
                println!("            sequences: {:?},", proof.sequences);
                println!("        }},");
            }

            println!("    ],");

            if let Some(ref block_proof) = proof_calc.block_proof {
                println!("    block_proof: Some(\"{}\"),", block_proof);
            } else {
                println!("    block_proof: None,");
            }

            println!("    is_finished: {},", proof_calc.is_finished);
            println!("}}");
        }
        Ok(None) => {
            println!("No proof calculation found for block {}", block);
        }
        Err(e) => {
            error!(
                "Failed to fetch proof calculation for block {}: {}",
                block, e
            );
            return Err(anyhow!("Failed to fetch proof calculation: {}", e).into());
        }
    }

    Ok(())
}