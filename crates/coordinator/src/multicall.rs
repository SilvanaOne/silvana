use crate::constants::MULTICALL_INTERVAL_SECS;
use crate::error::{CoordinatorError, Result};
use crate::metrics::CoordinatorMetrics;
use crate::state::{SharedState, StartedJob};
use std::sync::Arc;
use std::time::Instant;
use tokio::time::{Duration, sleep};
use tracing::{debug, error, info};

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
            // Check for shutdown request - but don't exit immediately
            // We need to keep processing until docker is done
            if self.state.is_shutting_down() {
                // Check if docker still has work
                let buffer_size = self.state.get_started_jobs_buffer_size().await;
                let current_agents = self.state.get_current_agent_count().await;
                
                if buffer_size == 0 && current_agents == 0 {
                    // Docker is done, now process final multicall operations
                    info!("🛑 Multicall processor: Docker completed, processing final operations...");
                    
                    let pending_operations = self.state.get_total_operations_count().await;
                    if pending_operations > 0 {
                        info!("📤 Processing {} final operations before shutdown...", pending_operations);
                        
                        // Execute all pending batches
                        while self.state.get_total_operations_count().await > 0 {
                            if let Err(e) = self.execute_multicall_batch().await {
                                error!("Failed to execute shutdown multicall batch: {}", e);
                                // Continue trying to process remaining operations
                            }
                        }
                        
                        info!("✅ All pending multicall operations processed");
                    }
                    
                    return Ok(());
                } else {
                    // Docker still has work, continue processing multicalls normally
                    if buffer_size > 0 || current_agents > 0 {
                        debug!(
                            "Multicall continuing during shutdown: {} jobs buffered, {} containers running",
                            buffer_size, current_agents
                        );
                    }
                }
            }

            // Check if we should execute multicall
            let should_execute_by_limit = self.state.should_execute_multicall_by_limit().await;
            let should_execute_by_time = self.state.should_execute_multicall_by_time().await;
            let total_operations = self.state.get_total_operations_count().await;

            if should_execute_by_limit {
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

    /// Execute a single multicall batch
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

        let max_operations = sui::get_max_operations_per_multicall();

        // Build a single batch from multiple app instances
        let mut current_batch_operations = Vec::new();
        let mut current_batch_started_jobs = Vec::new();
        let mut current_operation_count = 0;
        let mut has_operations = false;

        // Try to fill a single batch by taking operations from each app instance
        for app_instance in &app_instances {
            if current_operation_count >= max_operations {
                break; // Batch is full
            }

            // Get partial operations from this app instance
            if let Some(operations) = self
                .take_partial_multicall_operations(
                    &app_instance,
                    max_operations - current_operation_count,
                )
                .await
            {
                let operation_count = operations.total_operations();
                if operation_count > 0 {
                    has_operations = true;
                    current_operation_count += operation_count;

                    // Collect started jobs for buffer management
                    for (i, sequence) in operations.start_job_sequences.iter().enumerate() {
                        let memory_req = operations
                            .start_job_memory_requirements
                            .get(i)
                            .copied()
                            .unwrap_or(0);
                        current_batch_started_jobs.push((
                            app_instance.clone(),
                            *sequence,
                            memory_req,
                        ));
                    }

                    info!(
                        "Added {} operations from app_instance {} to batch (batch total: {})",
                        operation_count, app_instance, current_operation_count
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
            "Executing multicall batch with {} operations from {} app instances",
            current_operation_count,
            current_batch_operations.len()
        );
        
        let batch_start_time = Instant::now();

        let mut sui_interface = sui::SilvanaSuiInterface::new();
        match sui_interface
            .multicall_job_operations(current_batch_operations, None)
            .await
        {
            Ok(result) => {
                let batch_duration = batch_start_time.elapsed();
                info!(
                    "Successfully executed batch multicall with {} operations (tx: {})",
                    current_operation_count, result.tx_digest
                );
                
                // Report successful multicall metrics
                if let Some(ref metrics) = self.metrics {
                    metrics.increment_multicall_batch_executed(
                        current_operation_count,
                        batch_duration.as_millis() as usize
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
                    .filter(|(_, sequence, _)| successful_start_sequences.contains(sequence))
                    .map(|(app_instance, sequence, memory_req)| StartedJob {
                        app_instance,
                        job_sequence: sequence,
                        memory_requirement: memory_req,
                    })
                    .collect();

                // Add successful jobs to the buffer
                if !successful_started_jobs.is_empty() {
                    info!(
                        "Adding {} successfully started jobs to container launch buffer",
                        successful_started_jobs.len()
                    );
                    self.state.add_started_jobs(successful_started_jobs.clone()).await;
                    
                    // Report start jobs metrics
                    if let Some(ref metrics) = self.metrics {
                        metrics.add_multicall_start_jobs_result(
                            successful_started_jobs.len(),
                            failed_start_sequences.len()
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
            }
            Err((error_msg, tx_digest_opt)) => {
                error!(
                    "Batch multicall failed: {} (tx: {:?})",
                    error_msg, tx_digest_opt
                );
                
                // Report failed multicall metrics
                if let Some(ref metrics) = self.metrics {
                    metrics.increment_multicall_batch_failed();
                }
                
                return Err(CoordinatorError::Other(anyhow::anyhow!(
                    "Batch multicall failed: {}",
                    error_msg
                )));
            }
        }

        Ok(())
    }

    /// Take partial operations from an app instance up to the specified limit
    async fn take_partial_multicall_operations(
        &self,
        app_instance: &str,
        max_operations: usize,
    ) -> Option<sui::MulticallOperations> {
        let app_instance = crate::state::normalize_app_instance_id(app_instance);
        let mut requests_lock = self.state.get_multicall_requests_mut().await;

        if let Some(requests) = requests_lock.get_mut(&app_instance) {
            let mut operations_count = 0;
            let mut taken_operations = sui::MulticallOperations::new(app_instance.clone(), 0);

            // Take operations one by one until we hit the limit

            // Start jobs - sort by sequence
            if operations_count < max_operations && !requests.start_jobs.is_empty() {
                requests
                    .start_jobs
                    .sort_by(|a, b| a.job_sequence.cmp(&b.job_sequence));

                // Take start jobs with validation
                let mut validated_start_jobs = Vec::new();
                let mut remaining_start_jobs = Vec::new();

                for start_job in requests.start_jobs.drain(..) {
                    if operations_count >= max_operations {
                        remaining_start_jobs.push(start_job);
                        continue;
                    }

                    // Validate settlement jobs by checking chain
                    let is_valid = true;
                    if let Ok(Some(_chain)) =
                        sui::fetch::app_instance::get_settlement_chain_by_job_sequence(
                            &app_instance,
                            start_job.job_sequence,
                        )
                        .await
                    {
                        info!(
                            " Settlement start_job {} for app_instance {} - validated",
                            start_job.job_sequence, app_instance
                        );
                    }

                    if is_valid {
                        taken_operations
                            .start_job_sequences
                            .push(start_job.job_sequence);
                        taken_operations
                            .start_job_memory_requirements
                            .push(start_job.memory_requirement);
                        operations_count += 1;
                        validated_start_jobs.push(start_job);
                    } else {
                        remaining_start_jobs.push(start_job);
                    }
                }

                // Put remaining jobs back
                requests.start_jobs = remaining_start_jobs;
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
                                " Settlement create_job for app_instance {} - validated",
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
                                "=� Settlement create_job for app_instance {} has no settlement_chain - SKIPPING",
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
                Some(taken_operations)
            } else {
                None
            }
        } else {
            None
        }
    }
}
