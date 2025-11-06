use crate::constants::MULTICALL_INTERVAL_SECS;
use crate::error::Result;
use crate::metrics::CoordinatorMetrics;
use crate::state::{SharedState, StartedJob, TerminateJobRequest};
use silvana_coordination_trait::Coordination;
use std::sync::Arc;
use tokio::time::{Duration, Instant, sleep};
use tracing::{debug, error, info, warn};

/// Multicall processor that monitors multicall_requests and executes batched operations
pub struct MulticallProcessor {
    state: SharedState,
    metrics: Option<Arc<CoordinatorMetrics>>,
}

impl MulticallProcessor {
    pub fn new(state: SharedState) -> Self {
        Self {
            state,
            metrics: None,
        }
    }

    /// Set the metrics reporter
    pub fn set_metrics(&mut self, metrics: Arc<CoordinatorMetrics>) {
        self.metrics = Some(metrics);
    }

    /// Main loop for the multicall processor
    pub async fn run(&mut self) -> Result<()> {
        info!("🚀 Multicall processor started");

        loop {
            // Check for shutdown request
            if self.state.is_shutting_down() {
                // During shutdown, continue processing all operations
                let pending_operations = self.state.get_total_operations_count().await;
                let buffer_size = self.state.get_started_jobs_buffer_size().await;
                let current_agents = self.state.get_current_agent_count().await;

                // Only exit when both multicall operations AND docker work are done
                if pending_operations == 0 && buffer_size == 0 && current_agents == 0 {
                    // Everything appears done - but wait 1 second and double-check
                    // in case new operations were just added
                    info!("Multicall operations appear complete, waiting 1 second to verify...");
                    sleep(Duration::from_secs(1)).await;

                    // Final check after delay
                    let final_operations = self.state.get_total_operations_count().await;
                    let final_buffer_size = self.state.get_started_jobs_buffer_size().await;
                    let final_agents = self.state.get_current_agent_count().await;

                    if final_operations > 0 || final_buffer_size > 0 || final_agents > 0 {
                        // Race condition detected - new work appeared
                        debug!(
                            "Race condition detected: {} new operations, {} buffered jobs, {} agents - continuing",
                            final_operations, final_buffer_size, final_agents
                        );
                        continue; // Go back to processing
                    }

                    // Really done now - set flag before exiting
                    info!("🛑 Multicall processor received shutdown signal");
                    info!("✅ All multicall operations processed and docker completed");
                    self.state.set_multicall_completed();
                    return Ok(());
                } else {
                    // Continue processing during shutdown
                    if pending_operations > 0 || buffer_size > 0 || current_agents > 0 {
                        debug!(
                            "Multicall continuing during shutdown: {} operations, {} jobs buffered, {} containers running",
                            pending_operations, buffer_size, current_agents
                        );

                        // Try to process any stuck operations during shutdown
                        if pending_operations > 0 && buffer_size == 0 && current_agents == 0 {
                            // Operations exist but no active work - might be stuck
                            debug!(
                                "Attempting to process {} potentially stuck operations during shutdown",
                                pending_operations
                            );
                            if let Err(e) = self.execute_multicall_batch().await {
                                error!("Failed to execute stuck operations during shutdown: {}", e);
                            }
                            // Update timestamp to prevent repeated attempts
                            self.state.update_last_multicall_timestamp().await;
                        }
                    }
                    // Don't return - continue the loop to process operations
                }
            }

            // Check if we should execute multicall
            let should_execute_by_limit = self.state.should_execute_multicall_by_limit().await;
            let should_execute_by_time = self.state.should_execute_multicall_by_time().await;
            let total_operations = self.state.get_total_operations_count().await;

            // For settlement nodes, check if we have any settle jobs to process immediately
            let should_execute_settle_immediately = if self.state.is_settle_only() {
                self.state.has_settle_jobs_pending().await
            } else {
                false
            };

            if should_execute_settle_immediately {
                debug!("Executing multicall immediately for settlement job (settle mode)");
                if let Err(e) = self.execute_multicall_batch().await {
                    error!("Failed to execute settlement multicall: {}", e);
                }
            } else if should_execute_by_limit {
                info!(
                    "Executing multicall due to operation limit: {} operations >= {} limit",
                    total_operations,
                    sui::get_max_operations_per_multicall()
                );
                let start_time = Instant::now();
                if let Err(e) = self.execute_multicall_batch().await {
                    error!("Failed to execute limit-triggered multicall: {}", e);
                }
                let duration = start_time.elapsed();
                debug!("Multicall execution time: {} ms", duration.as_millis());
            } else if should_execute_by_time && total_operations > 0 {
                info!(
                    "Executing multicall due to time interval: {} seconds passed with {} operations",
                    MULTICALL_INTERVAL_SECS, total_operations
                );

                if let Err(e) = self.execute_multicall_batch().await {
                    error!("Failed to execute time-triggered multicall: {}", e);
                }
                // Always update timestamp after time-triggered execution attempt
                self.state.update_last_multicall_timestamp().await;
            } else if should_execute_by_time {
                // Time passed but no operations - just update timestamp to reset the timer
                debug!("Time interval passed but no operations to execute");
                self.state.update_last_multicall_timestamp().await;
            } else if total_operations > 0 {
                debug!(
                    "{} operations pending, waiting {} more seconds ({}s elapsed since last multicall)",
                    total_operations,
                    MULTICALL_INTERVAL_SECS - self.state.get_seconds_since_last_multicall().await,
                    self.state.get_seconds_since_last_multicall().await
                );
            }

            // Sleep before next check
            sleep(Duration::from_secs(1)).await;
        }
    }

    /// Execute a single multicall batch with progressive retry on failure
    async fn execute_multicall_batch(&self) -> Result<()> {
        // Get app instances with pending requests
        let app_instances = self.state.has_pending_multicall_requests().await;

        if app_instances.is_empty() {
            debug!("No app instances with pending multicall requests");
            return Ok(());
        }

        debug!(
            "Found {} app instances ready for multicall execution",
            app_instances.len()
        );

        // Use Sui's max operations per multicall as the initial batch size
        let initial_max_operations = sui::get_max_operations_per_multicall();
        let mut current_max_operations = initial_max_operations;

        debug!(
            "Starting multicall with initial batch size: {} (sui max)",
            initial_max_operations
        );

        // Progressive retry loop: divide batch size by 2 on each failure
        let mut attempt_count = 0;
        loop {
            attempt_count += 1;

            // Build a single batch from multiple app instances
            let mut current_batch_operations = Vec::new();
            let mut current_batch_started_jobs = Vec::new();
            let mut current_operation_count = 0;
            let mut has_operations = false;

            // Try to fill a single batch by taking operations from each app instance
            for app_instance in &app_instances {
                if current_operation_count >= current_max_operations {
                    break; // Batch is full
                }

                // Get partial operations from this app instance
                if let Some((operations, start_job_requests)) = self
                    .take_partial_multicall_operations(
                        &app_instance,
                        current_max_operations - current_operation_count,
                    )
                    .await
                {
                    let operation_count = operations.total_operations();
                    if operation_count > 0 {
                        has_operations = true;
                        current_operation_count += operation_count;

                        // Collect started jobs for buffer management with full metadata
                        for start_req in start_job_requests {
                            current_batch_started_jobs.push((
                                app_instance.clone(),
                                start_req.job_sequence,
                                start_req.memory_requirement,
                                start_req.job_type,
                                start_req.block_number,
                                start_req.sequences,
                                start_req.layer_id.clone(),
                            ));
                        }

                        info!(
                            "Added {} operations from app_instance {} to batch (batch total: {}, max: {}, attempt: {})",
                            operation_count,
                            app_instance,
                            current_operation_count,
                            current_max_operations,
                            attempt_count
                        );

                        current_batch_operations.push(operations);
                    }
                }
            }

            // If no operations were collected, we're done
            if !has_operations {
                debug!("No operations to process in this batch");
                return Ok(());
            }

            // Execute the batch
            info!(
                "Executing multicall batch with {} operations from {} app instances (attempt: {})",
                current_operation_count,
                current_batch_operations.len(),
                attempt_count
            );

            let batch_start_time = Instant::now();

            let mut sui_interface = sui::SilvanaSuiInterface::new();
            let max_computation_cost = if current_operation_count > 1 {
                Some(crate::constants::MAX_COMPUTATION_COST_MIST)
            } else {
                None
            };
            match sui_interface
                .multicall_job_operations(current_batch_operations.clone(), None, max_computation_cost)
                .await
            {
                Ok(result) => {
                    let batch_duration = batch_start_time.elapsed();
                    info!(
                        "Successfully executed batch multicall with {} operations (tx: {})",
                        current_operation_count, result.tx_digest
                    );

                    // Send CoordinationTxEvent
                    self.state
                        .send_coordination_tx_event(result.tx_digest.clone());

                    // Report successful multicall metrics
                    if let Some(ref metrics) = self.metrics {
                        metrics.increment_multicall_batch_executed(
                            current_operation_count,
                            batch_duration.as_millis() as usize,
                        );
                    }

                    // Add only successfully started jobs to buffer for container launching
                    let successful_start_sequences = result.successful_start_jobs();
                    let failed_start_sequences = result.failed_start_jobs();

                    if !failed_start_sequences.is_empty() {
                        info!("Some start jobs failed: {:?}", failed_start_sequences);
                    }

                    // Filter started jobs to only include successful ones
                    let successful_started_jobs: Vec<StartedJob> = current_batch_started_jobs
                        .into_iter()
                        .filter(|(_, sequence, _, _, _, _, _)| {
                            successful_start_sequences.contains(sequence)
                        })
                        .map(
                            |(
                                app_instance,
                                sequence,
                                memory_req,
                                job_type,
                                block_number,
                                sequences,
                                layer_id,
                            )| StartedJob {
                                app_instance,
                                job_sequence: sequence,
                                memory_requirement: memory_req,
                                job_type,
                                block_number,
                                sequences,
                                layer_id,
                                retry_count: 0,
                                last_retry_at: None,
                            },
                        )
                        .collect();

                    // Add successful jobs to the buffer
                    if !successful_started_jobs.is_empty() {
                        info!(
                            "Adding {} successfully started jobs to container launch buffer",
                            successful_started_jobs.len()
                        );
                        self.state
                            .add_started_jobs(successful_started_jobs.clone())
                            .await;

                        // Report start jobs metrics
                        if let Some(ref metrics) = self.metrics {
                            metrics.add_multicall_start_jobs_result(
                                successful_started_jobs.len(),
                                failed_start_sequences.len(),
                            );
                        }
                    }

                    info!(
                        "Multicall complete: {} jobs started successfully, {} failed",
                        successful_start_sequences.len(),
                        failed_start_sequences.len()
                    );

                    // Update the last multicall timestamp after successful execution
                    self.state.update_last_multicall_timestamp().await;

                    // Success - break out of retry loop
                    break;
                }
                Err((error_msg, tx_digest_opt)) => {
                    // Check if it's a gas-related error
                    let is_gas_issue = error_msg.contains("InsufficientGas")
                        || error_msg.contains("gas")
                        || error_msg.contains("Computation cost")
                        || error_msg.contains("Balance of gas object");

                    // Return operations to queue for retry, preserving start job metadata
                    let mut total_returned = 0;
                    for operations in current_batch_operations {
                        let op_count = operations.total_operations();

                        // Filter metadata for this specific app_instance
                        let app_instance_metadata: Vec<_> = current_batch_started_jobs
                            .iter()
                            .filter(|(app_inst, _, _, _, _, _, _)| app_inst == &operations.app_instance)
                            .map(|(app, seq, mem, jtype, block, seqs, lid)| {
                                (app.clone(), *seq, *mem, jtype.clone(), *block, seqs.clone(), lid.clone())
                            })
                            .collect();

                        self.state.return_operations_to_queue(operations, app_instance_metadata).await;
                        total_returned += op_count;
                    }

                    // Calculate next batch size
                    let next_max_operations = if current_max_operations > 1 {
                        (current_max_operations / 2).max(1)
                    } else {
                        1
                    };

                    // Check if we should retry or give up
                    if current_max_operations == 1 || next_max_operations == current_max_operations
                    {
                        // Already at minimum batch size, no more retries
                        let full_error = format!(
                            "Batch multicall failed: {} (tx: {:?})",
                            error_msg, tx_digest_opt
                        );
                        error!("{}", full_error);
                        error!(
                            "❌ Failed to execute multicall even with batch size of 1 after {} attempts",
                            attempt_count
                        );

                        warn!(
                            "Returned {} operations to queue for later retry after all attempts failed",
                            total_returned
                        );

                        // Send error event to RPC
                        self.state.send_coordinator_message_event(
                            proto::LogLevel::Error,
                            format!(
                                "{} - All retry attempts failed. Returned {} operations to queue",
                                full_error, total_returned
                            ),
                        );

                        // Report failed multicall metrics
                        if let Some(ref metrics) = self.metrics {
                            metrics.increment_multicall_batch_failed();
                        }

                        // Don't update timestamp on failure - allow retry in next cycle
                        info!("Operations returned to queue, will retry in next multicall cycle");
                        break;
                    }

                    // Log the retry attempt
                    if is_gas_issue {
                        warn!(
                            "⚠️ Multicall failed due to gas constraints with batch size {} (attempt {}): {}. Retrying with batch size {}...",
                            current_max_operations, attempt_count, error_msg, next_max_operations
                        );
                    } else {
                        warn!(
                            "⚠️ Multicall failed with batch size {} (attempt {}): {}. Retrying with batch size {}...",
                            current_max_operations, attempt_count, error_msg, next_max_operations
                        );
                    }

                    info!(
                        "Returned {} operations to queue for retry with smaller batch size",
                        total_returned
                    );

                    // Update max operations for next attempt
                    current_max_operations = next_max_operations;

                    // Small delay before retry to avoid hammering the network
                    sleep(Duration::from_millis(500)).await;

                    // Continue loop to retry with smaller batch
                    continue;
                }
            }
        }

        Ok(())
    }

    /// Take partial operations from an app instance up to the specified limit
    /// Returns the operations and the associated StartJobRequest data for metadata preservation
    async fn take_partial_multicall_operations(
        &self,
        app_instance: &str,
        max_operations: usize,
    ) -> Option<(sui::MulticallOperations, Vec<crate::state::StartJobRequest>)> {
        let app_instance = crate::state::normalize_app_instance_id(app_instance);
        let mut requests_lock = self.state.get_multicall_requests_mut().await;

        if let Some(requests) = requests_lock.get_mut(&app_instance) {
            let mut operations_count = 0;
            let mut taken_operations = sui::MulticallOperations::new(app_instance.clone(), 0);
            let mut validated_start_jobs = Vec::new();

            // Take operations one by one until we hit the limit

            // Create merge jobs
            while operations_count < max_operations && !requests.create_merge_jobs.is_empty() {
                let merge_req = requests.create_merge_jobs.remove(0);
                taken_operations.create_merge_jobs.push((
                    merge_req.block_number,
                    merge_req.sequences,
                    merge_req.sequences1,
                    merge_req.sequences2,
                    merge_req.job_description,
                ));
                operations_count += 1;
            }

            // Submit proofs
            while operations_count < max_operations && !requests.submit_proofs.is_empty() {
                let proof_req = requests.submit_proofs.remove(0);
                taken_operations.submit_proofs.push((
                    proof_req.block_number,
                    proof_req.sequences,
                    proof_req.merged_sequences_1,
                    proof_req.merged_sequences_2,
                    proof_req.job_id,
                    proof_req.da_hash,
                    proof_req.cpu_cores,
                    proof_req.prover_architecture,
                    proof_req.prover_memory,
                    proof_req.cpu_time,
                ));
                operations_count += 1;
                info!(
                    "Added submit proof to batch: block {}",
                    proof_req.block_number
                );
            }

            // Update state operations
            while operations_count < max_operations
                && !requests.update_state_for_sequences.is_empty()
            {
                let update_req = requests.update_state_for_sequences.remove(0);
                taken_operations.update_state_for_sequences.push((
                    update_req.sequence,
                    update_req.new_state_data,
                    update_req.new_data_availability_hash,
                ));
                operations_count += 1;
                info!(
                    "Added update state operation to batch: {}",
                    update_req.sequence
                );
            }

            // Complete jobs
            while operations_count < max_operations && !requests.complete_jobs.is_empty() {
                let complete_job = requests.complete_jobs.remove(0);
                taken_operations
                    .complete_job_sequences
                    .push(complete_job.job_sequence);
                operations_count += 1;
                info!("Added complete job to batch: {}", complete_job.job_sequence);
            }

            // Fail jobs
            while operations_count < max_operations && !requests.fail_jobs.is_empty() {
                let fail_job = requests.fail_jobs.remove(0);
                taken_operations
                    .fail_job_sequences
                    .push(fail_job.job_sequence);
                taken_operations.fail_errors.push(fail_job.error);
                operations_count += 1;
                info!("Added fail job to batch: {}", fail_job.job_sequence);
            }

            // Terminate jobs
            while operations_count < max_operations && !requests.terminate_jobs.is_empty() {
                let terminate_job = requests.terminate_jobs.remove(0);
                taken_operations
                    .terminate_job_sequences
                    .push(terminate_job.job_sequence);
                operations_count += 1;
                info!(
                    "Added terminate job to batch: {}",
                    terminate_job.job_sequence
                );
            }

            // Start jobs - sort by sequence
            if operations_count < max_operations && !requests.start_jobs.is_empty() {
                requests
                    .start_jobs
                    .sort_by(|a, b| a.job_sequence.cmp(&b.job_sequence));

                // Take start jobs with validation
                let mut remaining_start_jobs = Vec::new();

                for start_job in requests.start_jobs.drain(..) {
                    if operations_count >= max_operations {
                        remaining_start_jobs.push(start_job);
                        continue;
                    }

                    // Fetch fresh app instance and job from blockchain to validate
                    let mut is_valid = false;
                    let mut is_error = false;

                    // Use coordination manager to route to correct layer
                    let coordination_result = match self.state.get_coordination_manager() {
                        Some(manager) => {
                            match manager.get_layer_for_app(&app_instance).await {
                                Ok((_layer_id, coordination)) => Some(coordination),
                                Err(e) => {
                                    error!("Failed to find coordination layer for app instance {}: {}", app_instance, e);
                                    None
                                }
                            }
                        }
                        None => {
                            error!("No coordination manager available");
                            None
                        }
                    };

                    if let Some(coordination) = coordination_result {
                        match coordination.fetch_app_instance(&app_instance).await {
                            Ok(_fresh_app_instance) => {
                                // Fetch the job from blockchain using trait method
                                match coordination.fetch_job_by_sequence(&app_instance, start_job.job_sequence).await {
                                    Ok(Some(fresh_job)) => {
                                        // Check if job status is Pending
                                        if matches!(fresh_job.status, silvana_coordination_trait::JobStatus::Pending) {
                                            // Check if it's a settlement job
                                            let is_settlement_job =
                                                fresh_job.app_instance_method == "settle";

                                            // Check settlement chain using coordination trait
                                            match coordination.get_settlement_job_sequences(&app_instance).await {
                                                Ok(settlement_ids) => {
                                                    let has_settlement_chain = settlement_ids.values().any(|&id| id == start_job.job_sequence);

                                                    if has_settlement_chain {
                                                        // Has settlement chain - validate if it's a settlement job
                                                        if is_settlement_job {
                                                            is_valid = true;
                                                            info!(
                                                                "Settlement job {} validated (status: Pending)",
                                                                start_job.job_sequence
                                                            );
                                                        } else {
                                                            warn!(
                                                                "Job {} has settlement_chain but is not a settlement job (method: {}) - skipping",
                                                                start_job.job_sequence, fresh_job.app_instance_method
                                                            );
                                                        }
                                                    } else {
                                                        // No settlement chain - validate if it's NOT a settlement job
                                                        if !is_settlement_job {
                                                            is_valid = true;
                                                            info!(
                                                                "Regular job {} validated (status: Pending)",
                                                                start_job.job_sequence
                                                            );
                                                        } else {
                                                            warn!(
                                                                "🧹 Orphaned settlement job {} detected (method: {}) - adding to terminate queue",
                                                                start_job.job_sequence, fresh_job.app_instance_method
                                                            );

                                                            requests.terminate_jobs.push(TerminateJobRequest {
                                                                job_sequence: start_job.job_sequence,
                                                                _timestamp: Instant::now(),
                                                            });
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    warn!(
                                                        "Failed to check settlement chain for job {}: {}",
                                                        start_job.job_sequence, e
                                                    );
                                                    is_error = true;
                                                }
                                            }
                                        } else {
                                            info!(
                                                "Job {} has status {:?}, not Pending - skipping",
                                                start_job.job_sequence, fresh_job.status
                                            );
                                        }
                                    }
                                    Ok(None) => {
                                        info!(
                                            "Job {} not found in blockchain",
                                            start_job.job_sequence
                                        );
                                    }
                                    Err(e) => {
                                        error!(
                                            "Failed to fetch job {} from blockchain: {}",
                                            start_job.job_sequence, e
                                        );
                                        is_error = true;
                                    }
                                }
                            }
                            Err(e) => {
                                error!(
                                    "Failed to fetch app instance {} from blockchain: {}",
                                    app_instance, e
                                );
                            }
                        }
                    } else {
                        // No coordination layer found - mark as error to retry later
                        is_error = true;
                    }

                    if is_valid {
                        taken_operations
                            .start_job_sequences
                            .push(start_job.job_sequence);
                        taken_operations
                            .start_job_memory_requirements
                            .push(start_job.memory_requirement);
                        operations_count += 1;
                        validated_start_jobs.push(start_job.clone());
                    } else {
                        info!("Job {} is not valid - skipping", start_job.job_sequence);
                        if is_error {
                            remaining_start_jobs.push(start_job);
                        }
                    }
                }

                // Put remaining jobs back
                requests.start_jobs = remaining_start_jobs;
            }

            // Create app jobs - validate settlement jobs
            if operations_count < max_operations && !requests.create_app_jobs.is_empty() {
                let mut validated_create_jobs = Vec::new();
                let mut remaining_create_jobs = Vec::new();

                for create_req in requests.create_app_jobs.drain(..) {
                    if operations_count >= max_operations {
                        remaining_create_jobs.push(create_req);
                        continue;
                    }

                    // Check if this is a settlement job
                    if create_req.method_name == "settle" {
                        if create_req.settlement_chain.is_some() {
                            info!(
                                " Settlement create_job for app_instance {} - validated",
                                app_instance
                            );
                            taken_operations.create_jobs.push((
                                create_req.method_name.clone(),
                                create_req.job_description.clone(),
                                create_req.block_number,
                                create_req.sequences.clone(),
                                create_req.sequences1.clone(),
                                create_req.sequences2.clone(),
                                create_req.data.clone(),
                                create_req.interval_ms,
                                create_req.next_scheduled_at,
                                create_req.settlement_chain.clone(),
                            ));
                            operations_count += 1;
                            validated_create_jobs.push(create_req);
                        } else {
                            error!(
                                "⚠️ Settlement create_job for app_instance {} has no settlement_chain - SKIPPING",
                                app_instance
                            );
                        }
                    } else {
                        // Non-settlement job, include it
                        taken_operations.create_jobs.push((
                            create_req.method_name.clone(),
                            create_req.job_description.clone(),
                            create_req.block_number,
                            create_req.sequences.clone(),
                            create_req.sequences1.clone(),
                            create_req.sequences2.clone(),
                            create_req.data.clone(),
                            create_req.interval_ms,
                            create_req.next_scheduled_at,
                            create_req.settlement_chain.clone(),
                        ));
                        operations_count += 1;
                        validated_create_jobs.push(create_req);
                    }
                }

                // Put remaining jobs back
                requests.create_app_jobs = remaining_create_jobs;
            }

            // Create jobs - direct create_jobs field
            while operations_count < max_operations && !requests.create_jobs.is_empty() {
                let create_job = requests.create_jobs.remove(0);
                // Convert CreateJobRequest to tuple format for create_jobs
                taken_operations.create_jobs.push((
                    create_job.app_instance_method.clone(),
                    None, // job_description
                    create_job.creation_block,
                    None, // sequences
                    None, // sequences1
                    None, // sequences2
                    create_job.input_data.clone(),
                    None, // interval_ms
                    None, // next_scheduled_at
                    None, // settlement_chain
                ));
                operations_count += 1;
            }

            // Get available memory
            use crate::constants::JOB_BUFFER_MEMORY_COEFFICIENT;
            use crate::hardware::get_available_memory_gb;
            let raw_available_memory_gb = get_available_memory_gb();
            let available_memory_with_coefficient_gb =
                raw_available_memory_gb as f64 * JOB_BUFFER_MEMORY_COEFFICIENT;
            let available_memory_bytes =
                (available_memory_with_coefficient_gb * 1024.0 * 1024.0 * 1024.0) as u64;
            taken_operations.available_memory = available_memory_bytes;

            // Check if we should remove the app instance from map
            let should_remove = requests.create_jobs.is_empty()
                && requests.start_jobs.is_empty()
                && requests.complete_jobs.is_empty()
                && requests.fail_jobs.is_empty()
                && requests.terminate_jobs.is_empty()
                && requests.update_state_for_sequences.is_empty()
                && requests.submit_proofs.is_empty()
                && requests.create_app_jobs.is_empty()
                && requests.create_merge_jobs.is_empty();

            // Log remaining operations before potentially removing
            if !should_remove {
                debug!(
                    "Remaining operations for {}: {} start, {} complete, {} fail, {} terminate, {} update state, {} submit proofs, {} create app jobs, {} create merge jobs",
                    app_instance,
                    requests.start_jobs.len(),
                    requests.complete_jobs.len(),
                    requests.fail_jobs.len(),
                    requests.terminate_jobs.len(),
                    requests.update_state_for_sequences.len(),
                    requests.submit_proofs.len(),
                    requests.create_app_jobs.len(),
                    requests.create_merge_jobs.len()
                );
            }

            // Remove app instance from map if no operations remain
            if should_remove {
                requests_lock.remove(&app_instance);
            }

            if operations_count > 0 {
                Some((taken_operations, validated_start_jobs))
            } else {
                None
            }
        } else {
            None
        }
    }
}
