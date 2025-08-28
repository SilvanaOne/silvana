
use anyhow::Result;
use sui::fetch::{AppInstance, ProofCalculation};
use tracing::{debug, info, warn};
use crate::settlement;
use crate::merge::analyze_and_create_merge_jobs_with_blockchain_data;

// Helper function to analyze proof completion and determine next action
pub async fn analyze_proof_completion(
  app_instance: &AppInstance,
  client: &mut sui_rpc::Client,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
  let analysis_start = std::time::Instant::now();
  debug!("🔍 Starting proof completion analysis for app: {}", app_instance.silvana_app_name);
  
  let last_proved_block_number = app_instance.last_proved_block_number;
  let last_settled_block_number = app_instance.last_settled_block_number;
  let current_block_number = app_instance.block_number;
  let previous_block_last_sequence = app_instance.previous_block_last_sequence;
  let current_sequence = app_instance.sequence;
  let app_instance_id = &app_instance.id;
  
  debug!("📊 AppInstance status: last_proved_block={}, last_settled_block={}, current_block={}, prev_block_last_seq={}, current_seq={}", 
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
      debug!("🔍 Checking for settlement opportunities from block {} to {}", 
          start_block, last_proved_block_number);
      
      for block_number in start_block..=last_proved_block_number {
          if let Ok(has_opportunity) = settlement::check_settlement_opportunity(
              app_instance,
              block_number,
              client
          ).await {
              if has_opportunity {
                  found_settlement_opportunity = true;
                  debug!("💰 Found settlement opportunity for block {}", block_number);
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
          debug!("📝 Creating periodic settle job for app instance {} (will settle blocks 1 to {})", 
              app_instance.silvana_app_name, last_proved_block_number);
          if let Err(e) = settlement::create_periodic_settle_job(app_instance, client).await {
              warn!("Failed to create settle job: {}", e);
          } else {
              debug!("✅ Successfully created periodic settle job");
          }
      } else {
          debug!("📋 Settlement job already exists with ID {}, will handle blocks 1 to {}", 
              existing_settle_job_id.unwrap(), last_proved_block_number);
      }
  } else {
      // No settlement opportunities - terminate existing settlement job if it exists
      if let Some(job_id) = existing_settle_job_id {
          debug!("🚫 No valid blocks to settle (only block 0 or no new proved blocks), terminating settlement job {}", job_id);
          let mut sui_interface = sui::interface::SilvanaSuiInterface::new(client.clone());
          if let Err(e) = sui_interface.terminate_app_job(&app_instance.id, job_id).await {
              warn!("Failed to terminate settlement job {}: {}", job_id, e);
          } else {
              debug!("✅ Successfully terminated settlement job {}", job_id);
          }
      }
  }
  
  // Step 2: Process blocks in order from last_proved_block_number + 1 to current_block_number for merge opportunities
  let blocks_to_analyze = (current_block_number - last_proved_block_number).saturating_sub(0);
  debug!("🔄 Processing {} blocks from {} to {} for merge opportunities", 
      blocks_to_analyze, last_proved_block_number + 1, current_block_number);
  
  let mut analyzed_blocks = 0;
  for block_number in (last_proved_block_number + 1)..=current_block_number {
      analyzed_blocks += 1;
      let block_start = std::time::Instant::now();
      debug!("📦 Analyzing block {} for merge opportunities", block_number);
      
      // Fetch ProofCalculation for this block
      let proof_calc_info = match sui::fetch::fetch_proof_calculation(
          client,
          app_instance_id,
          block_number
      ).await {
          Ok(Some(proof_calc)) => {
              debug!("📊 Found ProofCalculation for block {}", block_number);
              proof_calc
          }
          Ok(None) => {
              debug!("⏭️ No ProofCalculation found for block {}, skipping", block_number);
              continue;
          }
          Err(e) => {
              warn!("Failed to fetch ProofCalculation for block {}: {}, skipping", block_number, e);
              continue;
          }
      };
      
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
          debug!("✅ Successfully analyzed block {} for merge opportunities in {:.2}s", 
              block_number, block_duration.as_secs_f64());
      }
  }
  
  let analysis_duration = analysis_start.elapsed();
  // Single summary message for the entire analysis
  if found_settlement_opportunity || analyzed_blocks > 0 {
      let settlement_status = if found_settlement_opportunity {
          if existing_settle_job_id.is_some() {
              format!("settle_job_exists(id={})", existing_settle_job_id.unwrap())
          } else {
              "settle_job_created".to_string()
          }
      } else {
          "no_settlement".to_string()
      };
      
      info!(
          "✅ Analysis: app={}, blocks_analyzed={}, proved={}, settled={}, {}, time={:.2}s",
          app_instance.silvana_app_name,
          analyzed_blocks,
          last_proved_block_number,
          last_settled_block_number,
          settlement_status,
          analysis_duration.as_secs_f64()
      );
  } else {
      debug!("Analysis complete: no blocks to process, time={:.2}s", analysis_duration.as_secs_f64());
  }
  Ok(())
}