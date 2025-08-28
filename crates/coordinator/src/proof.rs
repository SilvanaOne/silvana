
use anyhow::Result;
use sui::fetch::{AppInstance, ProofCalculation, fetch_blocks_range, fetch_proof_calculations_range};
use tracing::{debug, info, warn};
use crate::settlement;
use crate::merge::analyze_and_create_merge_jobs_with_blockchain_data;

// Helper function to analyze proof completion and determine next action
pub async fn analyze_proof_completion(
  app_instance: &AppInstance,
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
  
  // Track statistics
  let mut blocks_checked_for_settlement = 0;
  let mut blocks_fetched = 0;
  let mut proofs_fetched = 0;
  
  // Check for settlement opportunities (skip block 0 as it cannot be settled)
  let mut found_settlement_opportunity = false;
  
  // Only check for settlement if there are proved blocks > 0 that haven't been settled
  // Block 0 is the genesis/initial state and cannot be settled
  if last_proved_block_number > 0 && last_proved_block_number > last_settled_block_number {
      let start_block = std::cmp::max(1, last_settled_block_number + 1);
      debug!("🔍 Checking for settlement opportunities from block {} to {}", 
          start_block, last_proved_block_number);
      
      // Fetch all blocks and proof calculations in parallel
      let fetch_start = std::time::Instant::now();
      
      // Run both fetches concurrently
      let (blocks_result, proofs_result) = tokio::join!(
          fetch_blocks_range(
              app_instance,
              start_block,
              last_proved_block_number
          ),
          fetch_proof_calculations_range(
              app_instance,
              start_block,
              last_proved_block_number
          )
      );
      
      let fetch_duration = fetch_start.elapsed();
      
      // Process blocks result
      let blocks_map = match blocks_result {
          Ok(blocks) => {
              blocks_fetched = blocks.len();
              debug!("Fetched {} blocks", blocks.len());
              blocks
          },
          Err(e) => {
              warn!("Failed to fetch blocks range: {}", e);
              Default::default()
          }
      };
      
      // Process proofs result
      let proofs_map = match proofs_result {
          Ok(proofs) => {
              proofs_fetched = proofs.len();
              debug!("Fetched {} proof calculations", proofs.len());
              proofs
          },
          Err(e) => {
              warn!("Failed to fetch proof calculations range: {}", e);
              Default::default()
          }
      };
      
      debug!("Fetched {} blocks and {} proofs in parallel in {:.2}s", 
          blocks_fetched, proofs_fetched, fetch_duration.as_secs_f64());
      
      // Check each block for settlement opportunities
      for block_number in start_block..=last_proved_block_number {
          blocks_checked_for_settlement += 1;
          if let Some(block_details) = blocks_map.get(&block_number) {
              let proof_calc = proofs_map.get(&block_number);
              
              let has_block_proof = proof_calc
                  .and_then(|pc| pc.block_proof.as_ref())
                  .map(|bp| !bp.is_empty())
                  .unwrap_or(false);
              
              // Check settlement opportunities:
              // 1. Proof is available but no settlement transaction
              let proof_available = block_details.proof_data_availability.is_some() || has_block_proof;
              let no_settlement_tx = block_details.settlement_tx_hash.is_none();
              
              if proof_available && no_settlement_tx {
                  debug!("Settlement opportunity found for block {}: proof available but no settlement tx", block_number);
                  found_settlement_opportunity = true;
                  break;
              }
              
              // 2. Settlement transaction exists but not included in block
              let has_settlement_tx = block_details.settlement_tx_hash.is_some();
              let not_included = !block_details.settlement_tx_included_in_block;
              
              if has_settlement_tx && not_included {
                  debug!("Settlement opportunity found for block {}: settlement tx exists but not included", block_number);
                  found_settlement_opportunity = true;
                  break;
              }
          }
      }
  } else {
      debug!("📊 No valid blocks to settle yet (last_proved={}, last_settled={}). Block 0 cannot be settled.", 
          last_proved_block_number, last_settled_block_number);
  }
  
  // Check for existing settlement job
  let existing_settle_job_id = sui::fetch::app_instance::get_settlement_job_id(app_instance).await
      .unwrap_or(None);
  
  if found_settlement_opportunity {
      // Create settlement job if it doesn't exist
      if existing_settle_job_id.is_none() {
          debug!("📝 Creating periodic settle job for app instance {} (will settle blocks 1 to {})", 
              app_instance.silvana_app_name, last_proved_block_number);
          if let Err(e) = settlement::create_periodic_settle_job(app_instance).await {
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
          let mut sui_interface = sui::interface::SilvanaSuiInterface::new();
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
          app_instance,
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
  
  // Prepare comprehensive stats
  let settlement_status = if found_settlement_opportunity {
      if existing_settle_job_id.is_some() {
          format!("settle_job_exists(id={})", existing_settle_job_id.unwrap())
      } else {
          "settle_job_created".to_string()
      }
  } else if existing_settle_job_id.is_some() {
      "settle_job_terminated".to_string()
  } else {
      "no_settlement_needed".to_string()
  };
  
  // Always log comprehensive stats at info level
  info!(
      "📊 Proof analysis complete | app: {} | current_block: {} | last_proved: {} | last_settled: {} | blocks_checked_settlement: {} | blocks_fetched: {} | proofs_fetched: {} | blocks_analyzed_merge: {} | settlement: {} | duration: {:.3}s",
      app_instance.silvana_app_name,
      current_block_number,
      last_proved_block_number,
      last_settled_block_number,
      blocks_checked_for_settlement,
      blocks_fetched,
      proofs_fetched,
      analyzed_blocks,
      settlement_status,
      analysis_duration.as_secs_f64()
  );
  
  Ok(())
}