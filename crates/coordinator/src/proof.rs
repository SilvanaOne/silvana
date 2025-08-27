
use anyhow::Result;
use sui::fetch::{AppInstance, ProofCalculation};
use tracing::{debug, info, warn};
use crate::settlement;
use crate::merge::analyze_and_create_merge_jobs_with_blockchain_data;

// Helper function to analyze proof completion and determine next action
// Refactored to take AppInstance struct with all necessary data
pub async fn analyze_proof_completion(
  app_instance: &AppInstance,
  client: &mut sui_rpc::Client,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
  let analysis_start = std::time::Instant::now();
  info!("🔍 Starting proof completion analysis for app: {}", app_instance.silvana_app_name);
  
  let last_proved_block_number = app_instance.last_proved_block_number;
  let last_settled_block_number = app_instance.last_settled_block_number;
  let current_block_number = app_instance.block_number;
  let previous_block_last_sequence = app_instance.previous_block_last_sequence;
  let current_sequence = app_instance.sequence;
  let app_instance_id = &app_instance.id;
  
  info!("📊 AppInstance status: last_proved_block={}, last_settled_block={}, current_block={}, prev_block_last_seq={}, current_seq={}", 
      last_proved_block_number, last_settled_block_number, current_block_number, previous_block_last_sequence, current_sequence);
  
  // Check if we're at the start of a new block with no sequences processed yet
  if last_proved_block_number + 1 == current_block_number && 
     previous_block_last_sequence + 1 == current_sequence {
      let analysis_duration = analysis_start.elapsed();
      info!("✅ We're at the start of block {} with no sequences processed yet (waiting for sequences) - took {:.2}s", 
          current_block_number, analysis_duration.as_secs_f64());
      return Ok(());
  }
  
  // Check for settlement opportunities (skip block 0 as it cannot be settled)
  let mut found_settlement_opportunity = false;
  
  // Only check for settlement if there are proved blocks > 0 that haven't been settled
  // Block 0 is the genesis/initial state and cannot be settled
  if last_proved_block_number > 0 && last_proved_block_number > last_settled_block_number {
      let start_block = std::cmp::max(1, last_settled_block_number + 1);
      info!("🔍 Checking for settlement opportunities from block {} to {}", 
          start_block, last_proved_block_number);
      
      for block_number in start_block..=last_proved_block_number {
          if let Ok(has_opportunity) = settlement::check_settlement_opportunity(
              app_instance,
              block_number,
              client
          ).await {
              if has_opportunity {
                  found_settlement_opportunity = true;
                  info!("💰 Found settlement opportunity for block {}", block_number);
                  break; // Found at least one opportunity
              }
          }
      }
  } else {
      debug!("📊 No valid blocks to settle yet (last_proved={}, last_settled={}). Block 0 cannot be settled.", 
          last_proved_block_number, last_settled_block_number);
  }
  
  // Check for existing settlement job
  let existing_settle_job_id = settlement::get_settlement_job_id(app_instance, client).await
      .unwrap_or(None);
  
  if found_settlement_opportunity {
      // Create settlement job if it doesn't exist
      if existing_settle_job_id.is_none() {
          info!("📝 Creating periodic settle job for app instance {} (will settle blocks 1 to {})", 
              app_instance.silvana_app_name, last_proved_block_number);
          if let Err(e) = settlement::create_periodic_settle_job(app_instance, client).await {
              warn!("Failed to create settle job: {}", e);
          } else {
              info!("✅ Successfully created periodic settle job");
          }
      } else {
          info!("📋 Settlement job already exists with ID {}, will handle blocks 1 to {}", 
              existing_settle_job_id.unwrap(), last_proved_block_number);
      }
  } else {
      // No settlement opportunities - terminate existing settlement job if it exists
      if let Some(job_id) = existing_settle_job_id {
          info!("🚫 No valid blocks to settle (only block 0 or no new proved blocks), terminating settlement job {}", job_id);
          let mut sui_interface = sui::interface::SilvanaSuiInterface::new(client.clone());
          if let Err(e) = sui_interface.terminate_app_job(&app_instance.id, job_id).await {
              warn!("Failed to terminate settlement job {}: {}", job_id, e);
          } else {
              info!("✅ Successfully terminated settlement job {}", job_id);
          }
      }
  }
  
  // Step 2: Process blocks in order from last_proved_block_number + 1 to current_block_number for merge opportunities
  let blocks_to_analyze = (current_block_number - last_proved_block_number).saturating_sub(0);
  info!("🔄 Processing {} blocks from {} to {} for merge opportunities", 
      blocks_to_analyze, last_proved_block_number + 1, current_block_number);
  
  let mut analyzed_blocks = 0;
  for block_number in (last_proved_block_number + 1)..=current_block_number {
      analyzed_blocks += 1;
      let block_start = std::time::Instant::now();
      info!("📦 Analyzing block {} for merge opportunities", block_number);
      
      // Fetch ProofCalculations for this block
      let proof_calculations = match sui::fetch::fetch_proof_calculations(
          client,
          app_instance_id,
          block_number
      ).await {
          Ok(proofs) => {
              if proofs.is_empty() {
                  info!("⏭️ No ProofCalculations found for block {}, skipping", block_number);
                  continue;
              }
              info!("📊 Found {} ProofCalculation(s) for block {}", proofs.len(), block_number);
              proofs
          }
          Err(e) => {
              warn!("Failed to fetch ProofCalculations for block {}: {}, skipping", block_number, e);
              continue;
          }
      };
      
      // Use the first ProofCalculation for this block
      let proof_calc_info = &proof_calculations[0];
      
      // Create a ProofCalculation struct for analysis
      let proof_calc = ProofCalculation {
          id: proof_calc_info.id.clone(),
          block_number,
          start_sequence: proof_calc_info.start_sequence,
          end_sequence: proof_calc_info.end_sequence,
          proofs: vec![], // Will be populated by merge analysis
          block_proof: proof_calc_info.block_proof.clone(),
          is_finished: proof_calc_info.is_finished,
      };
      
      // Analyze this block for merge opportunities
      // Use empty da_hash since we're just looking for merge opportunities
      if let Err(e) = analyze_and_create_merge_jobs_with_blockchain_data(
          &proof_calc,
          app_instance_id,
          client,
          "", // No specific da_hash for general analysis
      ).await {
          let block_duration = block_start.elapsed();
          warn!("Failed to analyze block {} for merges in {:.2}s: {}", 
              block_number, block_duration.as_secs_f64(), e);
          // Continue to next block even if this one fails
      } else {
          let block_duration = block_start.elapsed();
          info!("✅ Successfully analyzed block {} for merge opportunities in {:.2}s", 
              block_number, block_duration.as_secs_f64());
      }
  }
  
  let analysis_duration = analysis_start.elapsed();
  info!("🎉 Completed proof completion and settlement analysis for {} blocks in {:.2}s", 
      analyzed_blocks, analysis_duration.as_secs_f64());
  Ok(())
}