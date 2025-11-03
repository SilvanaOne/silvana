#[cfg(test)]
use crate::constants::JOB_PROCESSING_CHECK_DELAY_MS;
use crate::constants::{RETRY_MAX_DELAY_SECS, STUCK_JOB_TIMEOUT_SECS};
use crate::coordination_manager::CoordinationManager;
use crate::error::Result;
use silvana_coordination_trait::Coordination;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Key for agent method lookup: (developer, agent, agent_method)
type AgentMethodKey = (String, String, String);

/// Tracking info for an app_instance
#[derive(Clone, Debug)]
struct AppInstanceInfo {
    /// When this app_instance was last updated (job added or reconciliation)
    updated_at: Instant,
    /// Which coordination layer this app_instance belongs to
    layer_id: String,
}

/// Simplified JobsTracker that tracks app_instances with pending jobs
/// and maintains an index for fast lookups by (developer, agent, agent_method)
/// The actual job details are fetched on-demand from the blockchain using indexes
#[derive(Clone)]
pub struct JobsTracker {
    /// Map of app_instance IDs to their tracking info
    app_instances_with_jobs: Arc<RwLock<HashMap<String, AppInstanceInfo>>>,

    /// Index from (developer, agent, agent_method) to set of app_instances
    /// This allows fast lookup when gRPC queries for specific agent methods
    agent_method_index: Arc<RwLock<HashMap<AgentMethodKey, HashSet<String>>>>,

    /// Track last reconciliation time
    last_reconciliation: Arc<RwLock<Instant>>,

    /// Coordination manager for multi-layer support (optional for backward compatibility)
    coordination_manager: Option<Arc<CoordinationManager>>,
}

impl JobsTracker {
    pub fn new() -> Self {
        Self {
            app_instances_with_jobs: Arc::new(RwLock::new(HashMap::new())),
            agent_method_index: Arc::new(RwLock::new(HashMap::new())),
            last_reconciliation: Arc::new(RwLock::new(Instant::now())),
            coordination_manager: None,
        }
    }

    /// Create a new JobsTracker with CoordinationManager for multi-layer support
    pub fn new_with_manager(manager: Arc<CoordinationManager>) -> Self {
        Self {
            app_instances_with_jobs: Arc::new(RwLock::new(HashMap::new())),
            agent_method_index: Arc::new(RwLock::new(HashMap::new())),
            last_reconciliation: Arc::new(RwLock::new(Instant::now())),
            coordination_manager: Some(manager),
        }
    }

    /// Add an app_instance that has pending jobs for a specific agent method
    /// Called when we see a JobCreatedEvent
    pub async fn add_job(
        &self,
        app_instance_id: String,
        developer: String,
        agent: String,
        agent_method: String,
        layer_id: String,
    ) {
        info!(
            "🔵 add_job called with: app_instance='{}' (len={}), dev='{}', agent='{}', method='{}', layer='{}'",
            app_instance_id,
            app_instance_id.len(),
            developer,
            agent,
            agent_method,
            layer_id
        );

        // Add to app_instances map with current timestamp and layer_id
        {
            let mut instances = self.app_instances_with_jobs.write().await;
            let is_new = !instances.contains_key(&app_instance_id);
            instances.insert(
                app_instance_id.clone(),
                AppInstanceInfo {
                    updated_at: Instant::now(),
                    layer_id: layer_id.clone(),
                },
            );
            if is_new {
                info!(
                    "✅ Added NEW app_instance {} to tracking on layer {}",
                    app_instance_id, layer_id
                );
            } else {
                debug!(
                    "Updated timestamp for app_instance {} on layer {}",
                    app_instance_id, layer_id
                );
            }
        }

        // Update agent_method_index
        {
            let mut agent_method_index = self.agent_method_index.write().await;
            let key = (developer, agent, agent_method);
            agent_method_index
                .entry(key.clone())
                .or_insert_with(HashSet::new)
                .insert(app_instance_id.clone());
            debug!(
                "Added app_instance {} to index for {:?}",
                app_instance_id, key
            );
        }
    }

    /// Remove an app_instance (e.g., when it has no more pending jobs)
    pub async fn remove_app_instance(&self, app_instance_id: &str) {
        // Remove from main map
        {
            let mut instances = self.app_instances_with_jobs.write().await;
            if instances.remove(app_instance_id).is_some() {
                debug!("Removed app_instance {} from tracking", app_instance_id);
            }
        }

        // Remove from all agent_method_index entries
        {
            let mut agent_method_index = self.agent_method_index.write().await;
            let mut empty_keys = Vec::new();

            for (key, app_instances) in agent_method_index.iter_mut() {
                app_instances.remove(app_instance_id);
                if app_instances.is_empty() {
                    empty_keys.push(key.clone());
                }
            }

            // Remove empty entries
            for key in empty_keys {
                agent_method_index.remove(&key);
                debug!("Removed empty index entry for {:?}", key);
            }
        }
    }

    /// Get all app instances with jobs (for metrics)
    pub async fn get_app_instances_with_jobs(&self) -> Vec<String> {
        let app_instances = self.app_instances_with_jobs.read().await;
        app_instances.keys().cloned().collect()
    }

    /// Get app_instances that have pending jobs for a specific (developer, agent, agent_method)
    /// This is used by gRPC to quickly find relevant app_instances
    #[allow(dead_code)]
    pub async fn get_app_instances_for_agent_method(
        &self,
        developer: &str,
        agent: &str,
        agent_method: &str,
    ) -> Vec<String> {
        let agent_method_index = self.agent_method_index.read().await;
        let key = (
            developer.to_string(),
            agent.to_string(),
            agent_method.to_string(),
        );

        agent_method_index
            .get(&key)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Get all app_instances that potentially have pending jobs
    pub async fn get_all_app_instances(&self) -> Vec<String> {
        let instances = self.app_instances_with_jobs.read().await;
        let result: Vec<String> = instances.keys().cloned().collect();
        info!(
            "🟢 get_all_app_instances returning {} instances: {:?}",
            result.len(),
            result
        );
        result
    }

    /// Get app_instances for a specific coordination layer
    pub async fn get_app_instances_for_layer(&self, layer_id: &str) -> Vec<String> {
        let instances = self.app_instances_with_jobs.read().await;
        let result: Vec<String> = instances
            .iter()
            .filter(|(_, info)| info.layer_id == layer_id)
            .map(|(id, _)| id.clone())
            .collect();

        result
    }

    /// Get the count of tracked app_instances
    pub async fn app_instances_count(&self) -> usize {
        let instances = self.app_instances_with_jobs.read().await;
        instances.len()
    }

    /// Get the layer_id for a specific app_instance
    pub async fn get_layer_id_for_app_instance(&self, app_instance: &str) -> Option<String> {
        let instances = self.app_instances_with_jobs.read().await;
        instances.get(app_instance).map(|info| info.layer_id.clone())
    }

    /// Get all app_instances with their layer_ids
    pub async fn get_all_app_instances_with_layer(&self) -> Vec<(String, String)> {
        let instances = self.app_instances_with_jobs.read().await;
        instances
            .iter()
            .map(|(app_id, info)| (app_id.clone(), info.layer_id.clone()))
            .collect()
    }

    /// Reconcile with on-chain state by checking pending_jobs_count for each tracked app_instance
    /// Also checks for stuck running jobs and fails them if they've been running too long
    /// Only removes app_instances that haven't been updated during the reconciliation
    /// Returns true if there are still pending jobs after reconciliation
    pub async fn reconcile_with_chain<F>(&self, add_fail_request: F) -> Result<bool>
    where
        F: Fn(
            String,
            u64,
            String,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>,
    {
        // Add random delay to prevent all coordinators from reconciling at the same time
        use rand::Rng;
        let delay_seconds = rand::thread_rng().gen_range(0..=RETRY_MAX_DELAY_SECS);
        info!(
            "⏱️ Waiting {} seconds before starting reconciliation to prevent coordinator synchronization",
            delay_seconds
        );
        tokio::time::sleep(tokio::time::Duration::from_secs(delay_seconds)).await;

        let initial_count = self.app_instances_count().await;
        info!(
            "🔄 Starting reconciliation with on-chain state ({} app_instances tracked)",
            initial_count
        );

        // Get a snapshot of instances to check with their timestamps
        let instances_to_check: Vec<(String, Instant)> = {
            let instances = self.app_instances_with_jobs.read().await;
            instances
                .iter()
                .map(|(id, info)| (id.clone(), info.updated_at))
                .collect()
        };

        // First, check for stuck running jobs
        self.fail_stuck_running_jobs(&instances_to_check, &add_fail_request)
            .await;

        let mut removed_count = 0;
        let mut instances_with_jobs = 0;
        let mut instances_with_errors = 0;
        let mut skipped_updated = 0;

        for (app_instance_id, original_timestamp) in &instances_to_check {
            // Get the coordination layer for this app instance
            let coordination = if let Some(ref manager) = self.coordination_manager {
                // Get layer info from our tracking
                let layer_id = {
                    let instances = self.app_instances_with_jobs.read().await;
                    instances
                        .get(app_instance_id)
                        .map(|info| info.layer_id.clone())
                };

                match layer_id {
                    Some(lid) => match manager.get_layer(&lid) {
                        Some(layer) => layer,
                        None => {
                            warn!(
                                "Layer {} not found for app_instance {}, skipping",
                                lid, app_instance_id
                            );
                            instances_with_errors += 1;
                            continue;
                        }
                    },
                    None => {
                        warn!(
                            "No layer_id found for app_instance {}, skipping",
                            app_instance_id
                        );
                        instances_with_errors += 1;
                        continue;
                    }
                }
            } else {
                // Backward compatibility: fall back to Sui fetch if no manager
                let _app_instance = match sui::fetch::fetch_app_instance(app_instance_id).await {
                    Ok(app_inst) => app_inst,
                    Err(e) => {
                        if e.to_string().contains("not found") || e.to_string().contains("NotFound")
                        {
                            debug!(
                                "App_instance {} not found on chain (likely deleted), removing from tracker",
                                app_instance_id
                            );
                            self.remove_app_instance(app_instance_id).await;
                            removed_count += 1;
                        } else {
                            error!("Failed to fetch app_instance {}: {}", app_instance_id, e);
                            instances_with_errors += 1;
                        }
                        continue;
                    }
                };
                continue; // Skip to next iteration after Sui-specific handling
            };

            // Fetch the app instance to check its state
            let _app_instance = match coordination.fetch_app_instance(app_instance_id).await {
                Ok(app_inst) => app_inst,
                Err(e) => {
                    if e.to_string().contains("not found") || e.to_string().contains("NotFound") {
                        debug!(
                            "App_instance {} not found on chain (likely deleted), removing from tracker",
                            app_instance_id
                        );
                        self.remove_app_instance(app_instance_id).await;
                        removed_count += 1;
                    } else {
                        error!("Failed to fetch app_instance {}: {}", app_instance_id, e);
                        instances_with_errors += 1;
                    }
                    continue;
                }
            };

            // TODO: Re-enable can_remove_app_instance check once we have coordination layer access here
            // For now, skip this check as it requires coordination layer which isn't available in JobsTracker
            // The pending jobs check below will still catch most cases
            // Use the comprehensive can_remove_app_instance check from settlement.rs
            // This checks: settlement complete, no active settlement jobs, no pending sequences, etc.
            // if !crate::settlement::can_remove_app_instance(coordination, &app_instance).await? {
            //     debug!(
            //         "App_instance {} cannot be removed per settlement checks (unsettled blocks or active jobs)",
            //         app_instance_id
            //     );
            //     instances_with_jobs += 1;
            //     continue;
            // }

            // AppInstance passed settlement checks, now check if it has pending or running jobs
            let pending_jobs_count = coordination
                .get_pending_jobs_count(app_instance_id)
                .await
                .unwrap_or(0);
            if pending_jobs_count == 0 {
                // Also check for running jobs before removing
                let total_jobs = coordination
                    .get_total_jobs_count(app_instance_id)
                    .await
                    .unwrap_or(0);
                let failed_jobs = coordination
                    .get_failed_jobs_count(app_instance_id)
                    .await
                    .unwrap_or(0);
                let running_jobs_count =
                    total_jobs.saturating_sub(pending_jobs_count + failed_jobs);
                let has_running_jobs = running_jobs_count > 0;

                if has_running_jobs {
                    debug!(
                        "App_instance {} has {} running jobs, keeping",
                        app_instance_id, running_jobs_count
                    );
                    instances_with_jobs += 1;
                    continue;
                }

                // Check if the timestamp has changed (new events arrived)
                let should_remove = {
                    let instances = self.app_instances_with_jobs.read().await;
                    instances
                        .get(app_instance_id)
                        .map(|info| info.updated_at == *original_timestamp)
                        .unwrap_or(false)
                };

                if should_remove {
                    info!(
                        "App_instance {} passed all removal checks (settlement complete, no jobs), removing from tracking",
                        app_instance_id
                    );
                    self.remove_app_instance(app_instance_id).await;
                    removed_count += 1;
                } else {
                    debug!(
                        "App_instance {} is ready for removal but was updated during reconciliation, keeping",
                        app_instance_id
                    );
                    skipped_updated += 1;
                }
            } else {
                debug!(
                    "App_instance {} has {} pending jobs",
                    app_instance_id, pending_jobs_count
                );
                instances_with_jobs += 1;
            }
        }

        // Update last reconciliation time
        let mut last_rec = self.last_reconciliation.write().await;
        *last_rec = Instant::now();

        let final_count = self.app_instances_count().await;

        debug!(
            "Reconciliation completed: {} checked, {} removed, {} have jobs, {} errors, {} skipped (updated during reconciliation). Now tracking {} app_instances",
            instances_to_check.len(),
            removed_count,
            instances_with_jobs,
            instances_with_errors,
            skipped_updated,
            final_count
        );

        if removed_count > 0 {
            info!("Reconciliation removed {} app_instances", removed_count);
        }

        // Return whether we still have pending jobs
        Ok(final_count > 0)
    }

    /// Check for stuck running jobs and fail them if they've been running too long
    /// Also check for orphaned pending jobs (jobs with Pending status but not in pending_jobs array)
    async fn fail_stuck_running_jobs<F>(
        &self,
        instances_to_check: &[(String, Instant)],
        _add_fail_request: F,
    ) where
        F: Fn(
            String,
            u64,
            String,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>,
    {
        let _max_running_duration = Duration::from_secs(STUCK_JOB_TIMEOUT_SECS);

        if instances_to_check.is_empty() {
            debug!("No app_instances to check for stuck jobs");
            return;
        }

        info!(
            "Checking {} app_instances for stuck running jobs and orphaned pending jobs",
            instances_to_check.len()
        );
        info!(
            "App instances being checked: {:?}",
            instances_to_check
                .iter()
                .map(|(id, _)| id)
                .collect::<Vec<_>>()
        );

        for (app_instance_id, _) in instances_to_check {
            // Get the coordination manager if available
            let manager = match &self.coordination_manager {
                Some(m) => m,
                None => {
                    // No coordination manager - skip stuck job checking for now
                    // This would be the old Sui-only path
                    debug!(
                        "No coordination manager available, skipping stuck job check for {}",
                        app_instance_id
                    );
                    continue;
                }
            };

            // Get layer info from our tracking
            let layer_id = {
                let instances = self.app_instances_with_jobs.read().await;
                match instances.get(app_instance_id) {
                    Some(info) => info.layer_id.clone(),
                    None => {
                        debug!(
                            "No layer_id found for app_instance {}, skipping stuck job check",
                            app_instance_id
                        );
                        continue;
                    }
                }
            };

            // Get the coordination layer
            let coordination = match manager.get_layer(&layer_id) {
                Some(layer) => layer,
                None => {
                    debug!(
                        "Layer {} not found for app_instance {}, skipping stuck job check",
                        layer_id, app_instance_id
                    );
                    continue;
                }
            };

            // Use coordination trait to check for stuck jobs
            // For now, we only check if jobs exist - the full stuck job detection
            // requires fetching job details which the coordination trait doesn't fully support yet
            // This is a simplified version that at least validates the app instance exists
            match coordination.fetch_app_instance(app_instance_id).await {
                Ok(_) => {
                    debug!(
                        "App instance {} exists on coordination layer, skipping detailed stuck job check for now",
                        app_instance_id
                    );
                    // TODO: Implement full stuck job checking using coordination trait once it supports:
                    // 1. Fetching all jobs
                    // 2. Getting settlement job IDs
                    // 3. Checking job status and timing
                }
                Err(e) => {
                    if !e.to_string().contains("not found") && !e.to_string().contains("NotFound") {
                        debug!(
                            "Failed to fetch app_instance {} for stuck job check: {}",
                            app_instance_id, e
                        );
                    }
                }
            }
        }
    }

    /// Backward-compatible Sui-specific stuck job checking (deprecated, kept for reference)
    #[allow(dead_code)]
    async fn fail_stuck_running_jobs_sui<F>(
        &self,
        instances_to_check: &[(String, Instant)],
        add_fail_request: F,
    ) where
        F: Fn(
            String,
            u64,
            String,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>,
    {
        let max_running_duration = Duration::from_secs(STUCK_JOB_TIMEOUT_SECS);
        for (app_instance_id, _) in instances_to_check {
            // Backward compatibility: use Sui-specific code path
            let app_instance = match sui::fetch::fetch_app_instance(app_instance_id).await {
                Ok(app_inst) => app_inst,
                Err(e) => {
                    if !e.to_string().contains("not found") && !e.to_string().contains("NotFound") {
                        debug!("Failed to fetch app_instance {}: {}", app_instance_id, e);
                    }
                    continue;
                }
            };

            // Continue with Sui-specific stuck job checking...
            match sui::fetch::fetch_all_jobs_from_app_instance(&app_instance).await {
                Ok(jobs) => {
                    let job_numbers: Vec<u64> = jobs.iter().map(|j| j.job_sequence).collect();
                    info!(
                        "Found {} total jobs for app_instance {}: {:?}",
                        jobs.len(),
                        app_instance_id,
                        job_numbers
                    );

                    // Get current time
                    let current_time_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64;

                    // Check if this app_instance has a settlement job for any chain
                    let settlement_job_ids =
                        sui::fetch::app_instance::get_settlement_job_ids_for_instance(
                            &app_instance,
                        )
                        .await
                        .unwrap_or_default();

                    // Print all settlement job IDs with their chain names
                    if !settlement_job_ids.is_empty() {
                        info!(
                            "App instance {} has {} settlement job(s):",
                            app_instance_id,
                            settlement_job_ids.len()
                        );
                        for (chain, job_id) in &settlement_job_ids {
                            info!("  - Chain '{}': settlement job ID {}", chain, job_id);
                        }
                    } else {
                        debug!("App instance {} has no settlement jobs", app_instance_id);
                    }

                    // Get the first settlement job ID for backward compatibility
                    let settlement_job_id = settlement_job_ids.values().next().copied();

                    // Get the list of pending_jobs from the Jobs object
                    let pending_jobs_list = if let Some(ref jobs_obj) = app_instance.jobs {
                        jobs_obj.pending_jobs.clone()
                    } else {
                        vec![]
                    };

                    // Check each job for stuck running state or orphaned pending state
                    for job in jobs {
                        debug!(
                            "Checking job {} with status {:?}, updated_at: {}, attempts: {}",
                            job.job_sequence, job.status, job.updated_at, job.attempts
                        );

                        if matches!(job.status, sui::fetch::JobStatus::Running) {
                            // Check if job has been running for longer than the timeout
                            let running_duration_ms =
                                current_time_ms.saturating_sub(job.updated_at);
                            let running_duration = Duration::from_millis(running_duration_ms);

                            let hours = running_duration.as_secs() / 3600;
                            let minutes = (running_duration.as_secs() % 3600) / 60;
                            let seconds = running_duration.as_secs() % 60;

                            info!(
                                "Job {} is Running: updated_at={}, current_time={}, duration={}h {}m {}s ({}ms), max_duration={}s",
                                job.job_sequence,
                                job.updated_at,
                                current_time_ms,
                                hours,
                                minutes,
                                seconds,
                                running_duration_ms,
                                max_running_duration.as_secs()
                            );

                            if running_duration > max_running_duration {
                                // Check if this is the settlement job
                                let is_settlement =
                                    settlement_job_id.map_or(false, |id| id == job.job_sequence);

                                if is_settlement {
                                    warn!(
                                        "SETTLEMENT job {} in app_instance {} has been running for {:.1} minutes, failing it",
                                        job.job_sequence,
                                        app_instance_id,
                                        running_duration.as_secs_f64() / 60.0
                                    );
                                } else {
                                    warn!(
                                        "Job {} in app_instance {} has been running for {:.1} minutes, failing it",
                                        job.job_sequence,
                                        app_instance_id,
                                        running_duration.as_secs_f64() / 60.0
                                    );
                                }

                                // Add random delay to avoid race conditions with other coordinators
                                use crate::constants::JOB_START_JITTER_MAX_MS;
                                use rand::Rng;
                                let delay_ms =
                                    rand::thread_rng().gen_range(0..JOB_START_JITTER_MAX_MS);
                                debug!(
                                    "Adding random delay of {}ms before failing stuck job {} to avoid race conditions",
                                    delay_ms, job.job_sequence
                                );
                                tokio::time::sleep(Duration::from_millis(delay_ms)).await;

                                // Re-fetch the job to check if it's still stuck
                                let job_still_stuck = if let Some(ref jobs_obj) = app_instance.jobs
                                {
                                    match sui::fetch::fetch_job_by_id(
                                        &jobs_obj.jobs_table_id,
                                        job.job_sequence,
                                    )
                                    .await
                                    {
                                        Ok(Some(fresh_job)) => {
                                            if matches!(
                                                fresh_job.status,
                                                sui::fetch::JobStatus::Running
                                            ) {
                                                let fresh_running_duration_ms = current_time_ms
                                                    .saturating_sub(fresh_job.updated_at);
                                                let fresh_running_duration = Duration::from_millis(
                                                    fresh_running_duration_ms,
                                                );
                                                fresh_running_duration > max_running_duration
                                            } else {
                                                debug!(
                                                    "Job {} is no longer in Running status after delay - already handled by another coordinator",
                                                    job.job_sequence
                                                );
                                                false
                                            }
                                        }
                                        Ok(None) => {
                                            debug!(
                                                "Job {} no longer exists after delay - already handled",
                                                job.job_sequence
                                            );
                                            false
                                        }
                                        Err(e) => {
                                            debug!(
                                                "Failed to re-fetch job {} after delay: {} - skipping",
                                                job.job_sequence, e
                                            );
                                            false
                                        }
                                    }
                                } else {
                                    debug!("No jobs object found for app_instance - skipping");
                                    false
                                };

                                if !job_still_stuck {
                                    continue;
                                }

                                // Fail the job
                                // Note: Settlement jobs are periodic, so they'll go back to Pending with a 1-minute delay after max attempts
                                // Regular jobs will either retry (if attempts < max) or be deleted
                                let error_msg = format!(
                                    "Job timed out after running for {} minutes",
                                    running_duration.as_secs() / 60
                                );

                                // Add fail job request to multicall batch
                                add_fail_request(
                                    app_instance_id.clone(),
                                    job.job_sequence,
                                    error_msg.clone(),
                                )
                                .await;

                                if is_settlement {
                                    info!(
                                        "Added fail request for stuck SETTLEMENT job {} in app_instance {} to multicall batch",
                                        job.job_sequence, app_instance_id
                                    );
                                } else {
                                    info!(
                                        "Added fail request for stuck job {} in app_instance {} to multicall batch",
                                        job.job_sequence, app_instance_id
                                    );
                                }
                            }
                        }
                        // Check for orphaned pending jobs (Pending status but not in pending_jobs array)
                        else if matches!(job.status, sui::fetch::JobStatus::Pending) {
                            let is_in_pending_jobs = pending_jobs_list.contains(&job.job_sequence);

                            if !is_in_pending_jobs {
                                // Fetch fresh job AND app_instance in parallel before logging error, as state might have changed
                                let job_sequence = job.job_sequence;

                                // Fetch both in parallel for efficiency
                                let (fresh_job_result, fresh_app_result) = tokio::join!(
                                    sui::fetch::fetch_job_by_id(app_instance_id, job_sequence),
                                    sui::fetch::fetch_app_instance(app_instance_id)
                                );

                                // Process job fetch result
                                let fresh_job = match fresh_job_result {
                                    Ok(Some(fresh)) => fresh,
                                    Ok(None) => {
                                        debug!(
                                            "Job {} no longer exists in app_instance {}",
                                            job_sequence, app_instance_id
                                        );
                                        continue;
                                    }
                                    Err(e) => {
                                        debug!(
                                            "Failed to fetch fresh status for job {}: {}",
                                            job_sequence, e
                                        );
                                        job.clone() // Use the original job if we can't fetch fresh
                                    }
                                };

                                // Process app_instance fetch result
                                let fresh_pending_jobs = match fresh_app_result {
                                    Ok(fresh_app) => {
                                        if let Some(ref jobs_obj) = fresh_app.jobs {
                                            jobs_obj.pending_jobs.clone()
                                        } else {
                                            vec![]
                                        }
                                    }
                                    Err(e) => {
                                        debug!(
                                            "Failed to fetch fresh app_instance for job {}: {}",
                                            job_sequence, e
                                        );
                                        pending_jobs_list.clone() // Use the original pending_jobs if we can't fetch fresh
                                    }
                                };

                                // Only log error if the fresh job is still Pending and still not in the fresh pending_jobs array
                                if matches!(fresh_job.status, sui::fetch::JobStatus::Pending)
                                    && !fresh_pending_jobs.contains(&fresh_job.job_sequence)
                                {
                                    // Check if the job was very recently updated (within last 5 seconds), which might mean it's being processed
                                    let job_age_ms =
                                        current_time_ms.saturating_sub(fresh_job.updated_at);
                                    if job_age_ms < 5000 {
                                        debug!(
                                            "Job {} appears orphaned but was updated {}ms ago, might be transitioning states",
                                            fresh_job.job_sequence, job_age_ms
                                        );
                                        continue;
                                    }

                                    // This job is truly orphaned - it has Pending status but is not in the pending_jobs array
                                    let is_settlement = settlement_job_id
                                        .map_or(false, |id| id == job.job_sequence);

                                    if is_settlement {
                                        error!(
                                            "ORPHANED SETTLEMENT job {} in app_instance {}: status is Pending with {} attempts, but NOT in pending_jobs array! This job cannot be picked up by coordinators. Details: created_at={}, updated_at={}, next_scheduled_at={:?}, original_pending_jobs={:?}, fresh_pending_jobs={:?}",
                                            fresh_job.job_sequence,
                                            app_instance_id,
                                            fresh_job.attempts,
                                            fresh_job.created_at,
                                            fresh_job.updated_at,
                                            fresh_job.next_scheduled_at,
                                            pending_jobs_list,
                                            fresh_pending_jobs
                                        );
                                    } else {
                                        error!(
                                            "ORPHANED job {} in app_instance {}: status is Pending with {} attempts, but NOT in pending_jobs array! This job cannot be picked up by coordinators. Details: created_at={}, updated_at={}, next_scheduled_at={:?}, original_pending_jobs={:?}, fresh_pending_jobs={:?}",
                                            fresh_job.job_sequence,
                                            app_instance_id,
                                            fresh_job.attempts,
                                            fresh_job.created_at,
                                            fresh_job.updated_at,
                                            fresh_job.next_scheduled_at,
                                            pending_jobs_list,
                                            fresh_pending_jobs
                                        );
                                    }

                                    // TODO: We could fix this by calling a new Move function that adds the job back to pending_jobs
                                    // For now, just log it so operators know to manually intervene
                                    // Options to fix:
                                    // 1. Create a new Move function `fix_orphaned_job` that adds it back to pending_jobs
                                    // 2. Delete the job and recreate it
                                    // 3. Manually call start_job then fail_job to reset it
                                } else {
                                    debug!(
                                        "Job {} in app_instance {} is no longer Pending (now {:?}), skipping orphan check",
                                        fresh_job.job_sequence, app_instance_id, fresh_job.status
                                    );
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    // Only log if it's not a "not found" error (app_instance might have been deleted)
                    if !e.to_string().contains("not found") && !e.to_string().contains("NotFound") {
                        debug!(
                            "Failed to fetch jobs for app_instance {}: {}",
                            app_instance_id, e
                        );
                    }
                }
            }
        }
    }

    /// Get statistics about the tracker
    pub async fn get_stats(&self) -> TrackerStats {
        let instances = self.app_instances_with_jobs.read().await;
        let agent_method_index = self.agent_method_index.read().await;

        TrackerStats {
            app_instances_count: instances.len(),
            agent_methods_count: agent_method_index.len(),
        }
    }
}

impl Default for JobsTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about the tracker state
#[derive(Debug, Clone)]
pub struct TrackerStats {
    pub app_instances_count: usize,
    pub agent_methods_count: usize,
}

/// Helper function to fetch pending_jobs_count from embedded Jobs in AppInstance
async fn fetch_pending_jobs_count_from_app_instance(app_instance_id: &str) -> Result<u64> {
    // Use the fetch_app_instance function from sui crate
    let app_instance = sui::fetch::fetch_app_instance(app_instance_id)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch AppInstance {}: {}", app_instance_id, e))?;

    // Get pending_jobs_count from the Jobs struct if it exists
    if let Some(jobs) = app_instance.jobs {
        Ok(jobs.pending_jobs_count)
    } else {
        Ok(0) // No jobs struct means no pending jobs
    }
}

/// Helper function to check if there are any running jobs for an app instance
async fn check_for_running_jobs(app_instance_id: &str) -> Result<bool> {
    // Use the fetch_app_instance function from sui crate
    let app_instance = sui::fetch::fetch_app_instance(app_instance_id)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch AppInstance {}: {}", app_instance_id, e))?;

    // Fetch all jobs and check for Running status
    match sui::fetch::fetch_all_jobs_from_app_instance(&app_instance).await {
        Ok(jobs) => {
            let running_count = jobs
                .iter()
                .filter(|job| matches!(job.status, sui::fetch::JobStatus::Running))
                .count();

            if running_count > 0 {
                debug!(
                    "App instance {} has {} running job(s)",
                    app_instance_id, running_count
                );
                Ok(true)
            } else {
                Ok(false)
            }
        }
        Err(e) => {
            // If we can't fetch jobs, assume there might be running jobs to be safe
            debug!("Failed to fetch jobs for {}: {}", app_instance_id, e);
            Ok(true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_add_and_remove_with_index() {
        let tracker = JobsTracker::new();

        // Add a job
        tracker
            .add_job(
                "app_instance1".to_string(),
                "dev1".to_string(),
                "agent1".to_string(),
                "method1".to_string(),
            )
            .await;

        // Check it was added
        assert_eq!(tracker.app_instances_count().await, 1);
        assert_eq!(tracker.app_instances_count().await, 1);

        // Check the index
        let instances = tracker
            .get_app_instances_for_agent_method("dev1", "agent1", "method1")
            .await;
        assert_eq!(instances.len(), 1);
        assert!(instances.contains(&"app_instance1".to_string()));

        // Remove the app_instance
        tracker.remove_app_instance("app_instance1").await;

        // Check it was removed from both main map and index
        assert_eq!(tracker.app_instances_count().await, 0);
        let instances = tracker
            .get_app_instances_for_agent_method("dev1", "agent1", "method1")
            .await;
        assert_eq!(instances.len(), 0);
    }

    #[tokio::test]
    async fn test_multiple_agent_methods_same_instance() {
        let tracker = JobsTracker::new();

        // Add same app_instance for different agent methods
        tracker
            .add_job(
                "app_instance1".to_string(),
                "dev1".to_string(),
                "agent1".to_string(),
                "method1".to_string(),
            )
            .await;

        tracker
            .add_job(
                "app_instance1".to_string(),
                "dev1".to_string(),
                "agent1".to_string(),
                "method2".to_string(),
            )
            .await;

        // Check app_instance is tracked only once
        assert_eq!(tracker.app_instances_count().await, 1);

        // But appears in both indexes
        let instances1 = tracker
            .get_app_instances_for_agent_method("dev1", "agent1", "method1")
            .await;
        assert_eq!(instances1.len(), 1);

        let instances2 = tracker
            .get_app_instances_for_agent_method("dev1", "agent1", "method2")
            .await;
        assert_eq!(instances2.len(), 1);

        // Remove the app_instance
        tracker.remove_app_instance("app_instance1").await;

        // Check it's removed from all indexes
        let instances1 = tracker
            .get_app_instances_for_agent_method("dev1", "agent1", "method1")
            .await;
        assert_eq!(instances1.len(), 0);

        let instances2 = tracker
            .get_app_instances_for_agent_method("dev1", "agent1", "method2")
            .await;
        assert_eq!(instances2.len(), 0);
    }

    #[tokio::test]
    async fn test_agent_method_index() {
        let tracker = JobsTracker::new();

        // Add jobs for same agent method but different app instances
        tracker
            .add_job(
                "app_instance1".to_string(),
                "dev1".to_string(),
                "agent1".to_string(),
                "method1".to_string(),
            )
            .await;

        tracker
            .add_job(
                "app_instance2".to_string(),
                "dev1".to_string(),
                "agent1".to_string(),
                "method1".to_string(),
            )
            .await;

        // Check the index returns both app_instances
        let instances = tracker
            .get_app_instances_for_agent_method("dev1", "agent1", "method1")
            .await;
        assert_eq!(instances.len(), 2);
        assert!(instances.contains(&"app_instance1".to_string()));
        assert!(instances.contains(&"app_instance2".to_string()));

        // Check total count
        assert_eq!(tracker.app_instances_count().await, 2);
    }

    #[tokio::test]
    async fn test_timestamp_updates() {
        let tracker = JobsTracker::new();

        // Add a job
        tracker
            .add_job(
                "app_instance1".to_string(),
                "dev1".to_string(),
                "agent1".to_string(),
                "method1".to_string(),
            )
            .await;

        // Get the initial timestamp
        let initial_timestamp = {
            let instances = tracker.app_instances_with_jobs.read().await;
            instances.get("app_instance1").unwrap().updated_at
        };

        // Wait a bit
        tokio::time::sleep(tokio::time::Duration::from_millis(
            JOB_PROCESSING_CHECK_DELAY_MS,
        ))
        .await;

        // Add another job to the same instance
        tracker
            .add_job(
                "app_instance1".to_string(),
                "dev1".to_string(),
                "agent1".to_string(),
                "method2".to_string(),
            )
            .await;

        // Check timestamp was updated
        let updated_timestamp = {
            let instances = tracker.app_instances_with_jobs.read().await;
            instances.get("app_instance1").unwrap().updated_at
        };

        assert!(updated_timestamp > initial_timestamp);
    }
}
