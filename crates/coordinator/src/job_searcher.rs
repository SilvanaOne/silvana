use crate::constants::{JOB_BUFFER_MEMORY_COEFFICIENT, JOB_SELECTION_POOL_SIZE};
use crate::coordination_manager::CoordinationManager;
use crate::coordination_layer::CoordinationLayer;
use crate::error::{CoordinatorError, Result};
use crate::hardware::{get_available_memory_gb, get_total_memory_gb};
use crate::job_lock::get_job_lock_manager;
use crate::jobs_cache::JobsCache;
use crate::metrics::CoordinatorMetrics;
use crate::proof::analyze_proof_completion;
use crate::settlement::{can_remove_app_instance, fetch_all_pending_jobs};
use crate::state::SharedState;
use futures::future;
use silvana_coordination_trait::{Coordination, Job};
use std::collections::BTreeMap;
use std::sync::Arc;
use sui::fetch_agent_method; // Keep this - registry is always on Sui
use tokio::time::{Duration, sleep};
use tracing::{debug, error, info, warn};

/// Job searcher that monitors for pending jobs and adds them to the multicall queue
pub struct JobSearcher {
    /// Coordination layer ID (e.g., "sui-mainnet", "private-1")
    layer_id: Option<String>,
    /// Coordination layer for this searcher
    layer: Option<Arc<CoordinationLayer>>,
    /// Coordination manager reference
    manager: Option<Arc<CoordinationManager>>,
    /// Shared state
    state: SharedState,
    /// Cache for failed jobs to avoid retrying
    jobs_cache: JobsCache,
    /// Metrics reporter
    metrics: Option<Arc<CoordinatorMetrics>>,
}

impl JobSearcher {
    /// Create a new job searcher (backward compatibility - no layer support)
    pub fn new(state: SharedState) -> Result<Self> {
        Ok(Self {
            layer_id: None,
            layer: None,
            manager: None,
            state,
            jobs_cache: JobsCache::new(),
            metrics: None,
        })
    }

    /// Create a new job searcher for a specific coordination layer
    pub fn new_with_layer(
        layer_id: String,
        manager: Arc<CoordinationManager>,
        state: SharedState,
    ) -> Result<Self> {
        let layer = manager.get_layer(&layer_id)
            .ok_or_else(|| CoordinatorError::Other(anyhow::anyhow!("Layer {} not found", layer_id)))?;

        Ok(Self {
            layer_id: Some(layer_id),
            layer: Some(layer),
            manager: Some(manager),
            state,
            jobs_cache: JobsCache::new(),
            metrics: None,
        })
    }

    /// Set the metrics reporter
    pub fn set_metrics(&mut self, metrics: Arc<CoordinatorMetrics>) {
        self.metrics = Some(metrics);
    }

    /// Main loop for the job searcher
    pub async fn run(&mut self) -> Result<()> {
        let layer_info = if let Some(ref layer_id) = self.layer_id {
            format!(" for layer {}", layer_id)
        } else {
            String::new()
        };
        info!("🔍 Job searcher started{}", layer_info);

        loop {
            // Check for shutdown request
            if self.state.is_shutting_down() {
                info!("🛑 Job searcher{} received shutdown signal", layer_info);
                return Ok(());
            }

            debug!("Starting job searcher{} cycle", layer_info);

            // Get app instances for this layer (or all if no layer)
            let app_instances = if let Some(ref layer_id) = self.layer_id {
                self.state.get_app_instances_for_layer(layer_id).await
            } else {
                self.state.get_app_instances().await
            };

            // Analyze proof completion for all app instances
            for app_instance_id in app_instances {
                // TODO: Update analyze_proof_completion to work with trait types, for now use Sui directly
                match sui::fetch::fetch_app_instance(&app_instance_id).await {
                    Ok(app_instance) => {
                        if let Err(analysis_err) =
                            analyze_proof_completion(&app_instance, &self.state.clone()).await
                        {
                            warn!(
                                "Failed to analyze failed proof for merge opportunities: {}",
                                analysis_err
                            );
                        } else {
                            debug!(
                                "✅ Background merge analysis completed for app instance {}",
                                app_instance_id
                            );
                        }
                    }
                    Err(e) => {
                        error!(
                            "Failed to fetch AppInstance {} for merge analysis: {}",
                            app_instance_id, e
                        );
                    }
                }
            }

            // Periodically clean up expired entries from the failed jobs cache
            self.jobs_cache.cleanup_expired().await;

            // Check for pending jobs and clean up app_instances without jobs
            // Get app instances for this layer (or all if no layer)
            let app_instances = if let Some(ref layer_id) = self.layer_id {
                self.state.get_app_instances_for_layer(layer_id).await
            } else {
                self.state.get_app_instances().await
            };
            let jobs = self.check_and_clean_pending_jobs(app_instances).await?;

            if !jobs.is_empty() {
                // Calculate available memory for new jobs
                let hardware_total_gb = get_total_memory_gb();
                let hardware_available_gb = get_available_memory_gb();

                // Get memory used by running containers
                let running_agents = self.state.get_all_current_agents().await;
                let mut running_memory_gb = 0.0f64;
                for (_session_id, agent_info) in &running_agents {
                    if let Ok(method) = fetch_agent_method(
                        &agent_info.developer,
                        &agent_info.agent,
                        &agent_info.agent_method,
                    )
                    .await
                    {
                        running_memory_gb += method.min_memory_gb as f64;
                    }
                }

                // Get memory reserved by jobs in buffer
                let buffer_count = self.state.get_started_jobs_count().await;
                // Estimate 3GB per buffered job (conservative)
                let buffer_memory_gb = (buffer_count as f64) * 3.0;

                // Calculate memory available for new jobs with coefficient multiplier
                let memory_for_new_jobs_gb = (hardware_available_gb as f64 - buffer_memory_gb)
                    * JOB_BUFFER_MEMORY_COEFFICIENT;

                info!(
                    "Memory status: Hardware total={:.2} GB, available={:.2} GB, running={:.2} GB, buffer={:.2} GB ({}), available for new={:.2} GB (x{:.1} coefficient)",
                    hardware_total_gb as f64,
                    hardware_available_gb as f64,
                    running_memory_gb,
                    buffer_memory_gb,
                    buffer_count,
                    memory_for_new_jobs_gb,
                    JOB_BUFFER_MEMORY_COEFFICIENT
                );

                // Group jobs by app_instance for multicall batching
                let mut jobs_by_app_instance: std::collections::HashMap<String, Vec<Job>> =
                    std::collections::HashMap::new();
                for job in jobs {
                    jobs_by_app_instance
                        .entry(job.app_instance.clone())
                        .or_insert_with(Vec::new)
                        .push(job);
                }

                debug!(
                    "Grouping jobs for {} app_instances",
                    jobs_by_app_instance.len()
                );

                // Process each app_instance's jobs and add to multicall queue
                let mut total_jobs_added = 0;
                let mut all_settlement_jobs = Vec::new();
                let mut all_merge_jobs = Vec::new();
                let mut all_other_jobs = Vec::new();

                for (app_instance, app_jobs) in jobs_by_app_instance {
                    debug!(
                        "Processing {} pending jobs for app_instance {}",
                        app_jobs.len(),
                        app_instance
                    );

                    // Track memory for this batch
                    let mut batch_memory_gb = 0.0f64;
                    let mut jobs_to_add = Vec::new();

                    // Collect unique agent methods to fetch
                    let mut unique_methods = std::collections::HashSet::new();
                    for job in &app_jobs {
                        unique_methods.insert((
                            job.developer.clone(),
                            job.agent.clone(),
                            job.agent_method.clone(),
                        ));
                    }

                    // Fetch all unique agent methods in parallel
                    let method_futures: Vec<_> = unique_methods
                        .iter()
                        .map(|(dev, agent, method)| {
                            let dev = dev.clone();
                            let agent = agent.clone();
                            let method = method.clone();
                            async move {
                                let result = fetch_agent_method(&dev, &agent, &method).await;
                                ((dev, agent, method), result)
                            }
                        })
                        .collect();

                    let method_results = future::join_all(method_futures).await;

                    // Build a cache of agent methods
                    let mut method_cache = std::collections::HashMap::new();
                    for ((dev, agent, method), result) in method_results {
                        if let Ok(agent_method) = result {
                            method_cache.insert((dev, agent, method), agent_method);
                        }
                    }

                    debug!(
                        "Fetched {} unique agent methods for {} jobs",
                        method_cache.len(),
                        app_jobs.len()
                    );

                    // Filter jobs based on available memory using cached agent methods
                    for job in app_jobs {
                        let cache_key = (
                            job.developer.clone(),
                            job.agent.clone(),
                            job.agent_method.clone(),
                        );

                        if let Some(agent_method) = method_cache.get(&cache_key) {
                            let job_memory_gb = agent_method.min_memory_gb as f64;

                            // Check if adding this job would exceed available memory
                            if batch_memory_gb + job_memory_gb > memory_for_new_jobs_gb {
                                continue;
                            }

                            // Job fits in memory budget
                            batch_memory_gb += job_memory_gb;
                            let memory_requirement =
                                (agent_method.min_memory_gb as u64) * 1024 * 1024 * 1024;
                            jobs_to_add.push((job, memory_requirement, agent_method.min_memory_gb));
                        } else {
                            error!(
                                "Failed to fetch agent method for job {} ({}/{}/{})",
                                job.job_sequence, job.developer, job.agent, job.agent_method
                            );
                            // Add to failed cache to avoid retrying immediately
                            self.jobs_cache
                                .add_failed_job(job.app_instance.clone(), job.job_sequence)
                                .await;
                        }
                    }

                    // Add the jobs that fit in memory to multicall queue
                    if !jobs_to_add.is_empty() {
                        for (job, memory_requirement, memory_gb) in jobs_to_add {
                            // Track job types globally for summary
                            match job.app_instance_method.as_str() {
                                "settle" => all_settlement_jobs.push(job.job_sequence),
                                "merge" => all_merge_jobs.push(job.job_sequence),
                                _ => all_other_jobs.push(job.job_sequence),
                            }

                            self.state
                                .add_start_job_request(
                                    job.app_instance.clone(),
                                    job.job_sequence,
                                    memory_requirement,
                                    job.app_instance_method.clone(),
                                    job.block_number,
                                    job.sequences.clone(),
                                    self.layer_id.clone().expect("JobSearcher must have layer_id"),
                                )
                                .await;

                            debug!(
                                "Added job {} ({}) to multicall queue for {} (memory: {} GB)",
                                job.job_sequence,
                                job.app_instance_method,
                                job.app_instance,
                                memory_gb
                            );
                            total_jobs_added += 1;
                        }
                    }
                }

                if total_jobs_added > 0 {
                    // Build detailed summary of job types
                    let mut job_summary = Vec::new();
                    if !all_settlement_jobs.is_empty() {
                        job_summary.push(format!(
                            "{} settlement ({})",
                            all_settlement_jobs.len(),
                            all_settlement_jobs
                                .iter()
                                .map(|s| s.to_string())
                                .collect::<Vec<_>>()
                                .join(",")
                        ));
                    }
                    if !all_merge_jobs.is_empty() {
                        job_summary.push(format!(
                            "{} merge ({})",
                            all_merge_jobs.len(),
                            all_merge_jobs
                                .iter()
                                .map(|s| s.to_string())
                                .collect::<Vec<_>>()
                                .join(",")
                        ));
                    }
                    if !all_other_jobs.is_empty() {
                        job_summary.push(format!(
                            "{} other ({})",
                            all_other_jobs.len(),
                            all_other_jobs
                                .iter()
                                .map(|s| s.to_string())
                                .collect::<Vec<_>>()
                                .join(",")
                        ));
                    }

                    info!(
                        "Added {} new jobs to multicall queue: {}",
                        total_jobs_added,
                        job_summary.join(", ")
                    );
                } else {
                    debug!("No new jobs to add to multicall queue");
                }
            } else {
                debug!("No new jobs found to collect");
            }

            // Sleep before next cycle
            // Settlement nodes check more frequently (5 seconds) vs regular nodes (15 seconds)
            let sleep_duration = if self.state.is_settle_only() {
                Duration::from_secs(5)
            } else {
                Duration::from_secs(15)
            };
            sleep(sleep_duration).await;
        }
    }

    /// Check for pending jobs and clean up app_instances without jobs
    /// This combines job searching with cleanup that reconciliation would do
    /// Collects all viable jobs for batching instead of selecting one randomly
    async fn check_and_clean_pending_jobs(&self, app_instances: Vec<String>) -> Result<Vec<Job>> {
        if app_instances.is_empty() {
            return Ok(Vec::new());
        }

        debug!(
            "Checking {} app_instances for pending jobs",
            app_instances.len()
        );

        // Check which app_instances can be removed (completely caught up with no work)
        let mut instances_to_remove = Vec::new();
        let manager = self.manager.as_ref().expect("JobSearcher requires coordination manager");
        for app_instance_id in &app_instances {
            // Fetch the full AppInstance object to check removal conditions
            // TODO: Update can_remove_app_instance to work with trait types, for now use Sui directly
            match sui::fetch::fetch_app_instance(app_instance_id).await {
                Ok(app_instance) => {
                    if can_remove_app_instance(manager, &app_instance).await.unwrap_or(false) {
                        info!(
                            "App instance {} is fully caught up and can be removed",
                            app_instance_id
                        );
                        instances_to_remove.push(app_instance_id.clone());
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to fetch app_instance {} for removal check: {}",
                        app_instance_id, e
                    );
                }
            }
        }

        // Remove fully caught up instances
        for instance_id in &instances_to_remove {
            self.state.remove_app_instance(instance_id).await;
            debug!("Removed fully caught up app_instance: {}", instance_id);
        }

        // Now fetch actual pending jobs from remaining app_instances (layer-filtered)
        let remaining_instances = if let Some(ref layer_id) = self.layer_id {
            self.state.get_app_instances_for_layer(layer_id).await
        } else {
            self.state.get_app_instances().await
        };
        if remaining_instances.is_empty() {
            debug!("No app_instances with pending jobs remaining after cleanup");
            return Ok(Vec::new());
        }

        debug!(
            "Fetching pending jobs from {} remaining app_instances",
            remaining_instances.len()
        );

        match fetch_all_pending_jobs(manager, &remaining_instances, false, self.state.is_settle_only()).await
        {
            Ok(pending_jobs) => {
                if pending_jobs.is_empty() {
                    // No pending jobs found
                    debug!("No pending jobs found after detailed fetch");
                    return Ok(Vec::new());
                }

                let total_jobs = pending_jobs.len();

                // Filter out jobs that are locked or in failed cache
                let lock_manager = get_job_lock_manager();
                let mut viable_jobs = Vec::new();
                let mut locked_count = 0usize;
                let mut failed_cached_count = 0usize;
                for job in pending_jobs {
                    // Skip if job is currently locked (being processed)
                    if lock_manager.is_locked(&job.app_instance, job.job_sequence) {
                        warn!(
                            "🔒 Skipping job {} from app_instance {} (currently locked)",
                            job.job_sequence, job.app_instance
                        );
                        locked_count += 1;
                        continue;
                    }

                    // Skip if job recently failed
                    if self
                        .jobs_cache
                        .is_recently_failed(&job.app_instance, job.job_sequence)
                        .await
                    {
                        warn!(
                            "Skipping job {} from app_instance {} (recently failed)",
                            job.job_sequence, job.app_instance
                        );
                        failed_cached_count += 1;
                        continue;
                    }

                    viable_jobs.push(job);
                }

                if viable_jobs.is_empty() {
                    debug!(
                        "No viable jobs after filtering (total={}, locked={}, failed_cached={})",
                        total_jobs, locked_count, failed_cached_count
                    );
                    return Ok(Vec::new());
                }

                // Check if we're in settle_only mode
                if self.state.is_settle_only() {
                    // In settle_only mode, only process settlement jobs
                    let settlement_jobs: Vec<Job> = viable_jobs
                        .iter()
                        .filter(|job| job.app_instance_method == "settle")
                        .cloned()
                        .collect();

                    if !settlement_jobs.is_empty() {
                        debug!(
                            "Found {} settlement jobs (settle_only mode)",
                            settlement_jobs.len()
                        );
                        return Ok(settlement_jobs);
                    } else {
                        debug!("No settlement jobs available (settle_only mode)");
                        return Ok(Vec::new());
                    }
                }

                // Normal mode: Collect all viable jobs (up to pool size)
                // Priority order:
                // 1. Settlement jobs (regardless of block)
                // 2. Jobs from lowest block (sorted by sequence array size, smallest first)
                // 3. Jobs from next block (sorted by sequence array size), etc.

                // Separate settlement jobs and group others by block
                let mut settlement_jobs: Vec<Job> = Vec::new();
                let mut jobs_by_block: BTreeMap<u64, Vec<Job>> = BTreeMap::new();

                for job in viable_jobs {
                    match job.app_instance_method.as_str() {
                        "settle" => settlement_jobs.push(job),
                        _ => {
                            // Group by block number (use u64::MAX for None to process last)
                            let block = job.block_number.unwrap_or(u64::MAX);
                            jobs_by_block.entry(block).or_insert(Vec::new()).push(job);
                        }
                    }
                }

                // Sort settlement jobs by sequence
                settlement_jobs.sort_by_key(|j| j.job_sequence);

                debug!("Collected {} settlement jobs", settlement_jobs.len());
                debug!("Collected jobs for {} blocks", jobs_by_block.len());

                // Build the job pool respecting pool size limit
                let mut job_pool = Vec::new();

                // Add all settlement jobs first (highest priority)
                job_pool.extend(settlement_jobs.clone());

                // Process blocks in order (BTreeMap iterates in key order)
                for (block, mut block_jobs) in jobs_by_block {
                    // Sort jobs by sequence array size (smallest first), then by job sequence
                    block_jobs.sort_by(|a, b| {
                        let a_size = a.sequences.as_ref().map(|s| s.len()).unwrap_or(0);
                        let b_size = b.sequences.as_ref().map(|s| s.len()).unwrap_or(0);
                        a_size
                            .cmp(&b_size)
                            .then_with(|| a.job_sequence.cmp(&b.job_sequence))
                    });

                    // Count job types for logging
                    let merge_count = block_jobs
                        .iter()
                        .filter(|j| j.app_instance_method == "merge")
                        .count();
                    let other_count = block_jobs.len() - merge_count;

                    debug!(
                        "Block {}: {} total jobs ({} merge, {} other), sorted by sequence size",
                        if block == u64::MAX {
                            "None".to_string()
                        } else {
                            block.to_string()
                        },
                        block_jobs.len(),
                        merge_count,
                        other_count
                    );

                    // Add jobs respecting pool size limit
                    let remaining = JOB_SELECTION_POOL_SIZE.saturating_sub(job_pool.len());
                    if remaining == 0 {
                        break;
                    }

                    let jobs_to_add = block_jobs.len().min(remaining);
                    job_pool.extend(block_jobs.into_iter().take(jobs_to_add));

                    // Break if we've filled the pool
                    if job_pool.len() >= JOB_SELECTION_POOL_SIZE {
                        break;
                    }
                }

                // Count jobs by type for logging
                let mut merge_jobs_count = 0;
                let mut other_jobs_count = 0;
                for job in &job_pool {
                    if job.app_instance_method != "settle" {
                        if job.app_instance_method == "merge" {
                            merge_jobs_count += 1;
                        } else {
                            other_jobs_count += 1;
                        }
                    }
                }

                info!(
                    "Collected {} jobs for batching: {} settlement, {} merge, {} other",
                    job_pool.len(),
                    settlement_jobs.len(),
                    merge_jobs_count,
                    other_jobs_count
                );

                // Report metrics for job pool
                if let Some(ref metrics) = self.metrics {
                    if !job_pool.is_empty() {
                        metrics.set_job_selection_metrics(
                            job_pool.len(),
                            merge_jobs_count,
                            other_jobs_count,
                            settlement_jobs.len(),
                            locked_count,
                            failed_cached_count,
                            job_pool[0].job_sequence, // Use first job for metric
                            job_pool[0].app_instance.clone(),
                        );
                    }
                }

                Ok(job_pool)
            }
            Err(e) => {
                error!("Failed to fetch pending jobs: {}", e);
                Err(CoordinatorError::Other(e))
            }
        }
    }
}
