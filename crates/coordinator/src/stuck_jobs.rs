// This module is kept for reference but is no longer used.
// Stuck job monitoring is handled by the reconciliation task in jobs.rs
// to avoid duplicate checks and reduce load on Sui nodes.
#![allow(dead_code)]

use crate::state::SharedState;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use sui::fetch::{fetch_all_jobs_from_app_instance, fetch_app_instance, JobStatus};
use sui::interface::SilvanaSuiInterface;
use tracing::{debug, error, info, warn};

/// Monitor for stuck jobs that runs periodically
/// NOTE: This is no longer used - reconciliation handles stuck jobs
pub struct StuckJobMonitor {
    state: SharedState,
    check_interval_secs: u64,
    timeout_duration_secs: u64,
}

impl StuckJobMonitor {
    /// Create a new stuck job monitor
    pub fn new(state: SharedState) -> Self {
        Self {
            state,
            check_interval_secs: 120,  // Check every 2 minutes
            timeout_duration_secs: 600, // Timeout jobs after 10 minutes
        }
    }

    /// Run the stuck job monitor
    pub async fn run(&self) {
        info!("🔍 Starting stuck job monitor (checks every {} seconds)", self.check_interval_secs);
        
        let mut check_interval = tokio::time::interval(Duration::from_secs(self.check_interval_secs));
        check_interval.tick().await; // Skip the first immediate tick
        
        loop {
            // Check for shutdown
            if self.state.is_shutting_down() {
                info!("Stuck job monitor shutting down...");
                break;
            }
            
            check_interval.tick().await;
            
            if self.state.is_shutting_down() {
                break;
            }
            
            // Check for stuck jobs
            self.check_for_stuck_jobs().await;
        }
    }

    /// Check all app instances for stuck jobs
    async fn check_for_stuck_jobs(&self) {
        let all_app_instances = self.state.get_app_instances().await;
        
        if !all_app_instances.is_empty() {
            debug!("Checking {} app_instances for stuck running jobs", all_app_instances.len());
            let mut stuck_count = 0;
            
            for app_instance_id in &all_app_instances {
                if self.state.is_shutting_down() {
                    break;
                }
                
                match self.check_app_instance_for_stuck_jobs(app_instance_id, &mut stuck_count).await {
                    Ok(_) => {}
                    Err(e) => {
                        error!("Error checking app_instance {} for stuck jobs: {}", app_instance_id, e);
                    }
                }
            }
            
            if stuck_count > 0 {
                warn!("Found and failed {} stuck jobs", stuck_count);
            } else {
                debug!("No stuck jobs found");
            }
        }
    }

    /// Check a single app instance for stuck jobs
    async fn check_app_instance_for_stuck_jobs(
        &self,
        app_instance_id: &str,
        stuck_count: &mut usize,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let app_inst = fetch_app_instance(app_instance_id).await?;
        let jobs = fetch_all_jobs_from_app_instance(&app_inst).await?;
        
        let current_time_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_millis() as u64;
        
        for job in jobs {
            if matches!(job.status, JobStatus::Running) {
                let running_duration_ms = current_time_ms.saturating_sub(job.updated_at);
                let running_duration = Duration::from_millis(running_duration_ms);
                
                // Check if job has been running for more than the timeout duration
                if running_duration > Duration::from_secs(self.timeout_duration_secs) {
                    *stuck_count += 1;
                    
                    // Check if this is a settlement job
                    let is_settlement = if let Ok(Some(settle_id)) = 
                        sui::fetch::app_instance::get_settlement_job_id_for_instance(&app_inst).await 
                    {
                        settle_id == job.job_sequence
                    } else {
                        false
                    };
                    
                    if is_settlement {
                        warn!(
                            "Found stuck SETTLEMENT job {} in app_instance {} running for {:.1} minutes",
                            job.job_sequence,
                            app_instance_id,
                            running_duration.as_secs() as f64 / 60.0
                        );
                    } else {
                        warn!(
                            "Found stuck job {} in app_instance {} running for {:.1} minutes",
                            job.job_sequence,
                            app_instance_id,
                            running_duration.as_secs() as f64 / 60.0
                        );
                    }
                    
                    // Try to fail the stuck job
                    self.fail_stuck_job(app_instance_id, job.job_sequence, running_duration).await;
                }
            }
        }
        
        Ok(())
    }

    /// Fail a stuck job
    async fn fail_stuck_job(&self, app_instance_id: &str, job_sequence: u64, running_duration: Duration) {
        let mut sui_interface = SilvanaSuiInterface::new();
        let error_msg = format!(
            "Job timed out after running for {} minutes",
            running_duration.as_secs() / 60
        );
        
        match sui_interface.fail_job(app_instance_id, job_sequence, &error_msg).await {
            Some(tx_hash) => {
                info!(
                    "Successfully failed stuck job {} in app_instance {}, tx: {}",
                    job_sequence, app_instance_id, tx_hash
                );
            }
            None => {
                error!(
                    "Failed to mark stuck job {} as failed in app_instance {}",
                    job_sequence, app_instance_id
                );
            }
        }
    }
}