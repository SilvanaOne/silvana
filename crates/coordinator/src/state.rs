use crate::agent::AgentJobDatabase;
use crate::jobs::JobsTracker;
use proto::silvana_events_service_client::SilvanaEventsServiceClient;
use tonic::transport::Channel;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{RwLock, Mutex};
use tokio::time::{Duration, Instant};
use tracing::{debug, error, info};

#[derive(Debug, Clone)]
pub struct CurrentAgent {
    pub session_id: String,
    pub developer: String,
    pub agent: String,
    pub agent_method: String,
    #[allow(dead_code)]
    pub settlement_chain: Option<String>,  // Track which chain this agent is settling for
}

/// Information about a job that has been started on the blockchain
#[derive(Debug, Clone)]
pub struct StartedJob {
    pub app_instance: String,
    pub job_sequence: u64,
    pub memory_requirement: u64,
}

/// Request to create a new job
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CreateJobRequest {
    pub job_sequence: Option<u64>,
    pub developer: String,
    pub agent: String,
    pub agent_method: String,
    pub app: String,
    pub app_instance: String,
    pub app_instance_method: String,
    pub initial_input_hash: Option<String>,
    pub merkle_root: Option<String>,
    pub merkle_paths: Option<Vec<Vec<u8>>>,
    pub periodic_run_time: Option<u64>,
    pub input_data: Vec<u8>,
    pub creation_block: Option<u64>,
    pub state_sequence: Option<u64>,
    pub _timestamp: Instant,
}

/// Request to start a job with memory requirements
#[derive(Debug, Clone)]
pub struct StartJobRequest {
    pub job_sequence: u64,
    pub memory_requirement: u64,
    pub _timestamp: Instant,
}

/// Request to complete a job
#[derive(Debug, Clone)]
pub struct CompleteJobRequest {
    pub job_sequence: u64,
    pub _timestamp: Instant,
}

/// Request to fail a job with error message
#[derive(Debug, Clone)]
pub struct FailJobRequest {
    pub job_sequence: u64,
    pub error: String,
    pub _timestamp: Instant,
}

/// Request to terminate a job
#[derive(Debug, Clone)]
pub struct TerminateJobRequest {
    pub job_sequence: u64,
    pub _timestamp: Instant,
}

/// Request to update state for a sequence
#[derive(Debug, Clone)]
pub struct UpdateStateForSequenceRequest {
    pub sequence: u64,
    pub new_state_data: Option<Vec<u8>>,
    pub new_data_availability_hash: Option<String>,
    pub _timestamp: Instant,
}

/// Request to submit a proof
#[derive(Debug, Clone)]
pub struct SubmitProofRequest {
    pub block_number: u64,
    pub sequences: Vec<u64>,
    pub merged_sequences_1: Option<Vec<u64>>,
    pub merged_sequences_2: Option<Vec<u64>>,
    pub job_id: String,
    pub da_hash: String,
    pub cpu_cores: u8,
    pub prover_architecture: String,
    pub prover_memory: u64,
    pub cpu_time: u64,
    pub _timestamp: Instant,
}

/// Request to create an app job (e.g., settlement job) through multicall
#[derive(Debug, Clone)]
pub struct CreateAppJobRequest {
    pub method_name: String,
    pub job_description: Option<String>,
    pub block_number: Option<u64>,
    pub sequences: Option<Vec<u64>>,
    pub sequences1: Option<Vec<u64>>,
    pub sequences2: Option<Vec<u64>>,
    pub data: Vec<u8>,
    pub interval_ms: Option<u64>,
    pub next_scheduled_at: Option<u64>,
    pub settlement_chain: Option<String>,
    pub _timestamp: Instant,
}

/// Request to create a merge job with proof reservation
#[derive(Debug, Clone)]
pub struct CreateMergeJobRequest {
    pub block_number: u64,
    pub sequences: Vec<u64>,       // Combined sequences
    pub sequences1: Vec<u64>,       // First proof sequences
    pub sequences2: Vec<u64>,       // Second proof sequences
    pub job_description: Option<String>,
    pub _timestamp: Instant,
}

/// Batched job operation requests for multicall
#[derive(Debug, Clone)]
pub struct MulticallRequests {
    pub _app_instance: String,
    pub create_jobs: Vec<CreateJobRequest>,
    pub start_jobs: Vec<StartJobRequest>,
    pub complete_jobs: Vec<CompleteJobRequest>,
    pub fail_jobs: Vec<FailJobRequest>,
    pub terminate_jobs: Vec<TerminateJobRequest>,
    pub update_state_for_sequences: Vec<UpdateStateForSequenceRequest>,
    pub submit_proofs: Vec<SubmitProofRequest>,
    pub create_app_jobs: Vec<CreateAppJobRequest>,
    pub create_merge_jobs: Vec<CreateMergeJobRequest>,
    pub last_execution: Instant,
}

#[derive(Clone)]
pub struct SharedState {
    // Track multiple current agents by session_id
    current_agents: Arc<RwLock<HashMap<String, CurrentAgent>>>,
    jobs_tracker: JobsTracker,
    agent_job_db: AgentJobDatabase, // Memory database for agent job tracking
    has_pending_jobs: Arc<AtomicBool>, // Fast check for pending jobs availability
    rpc_client: Arc<RwLock<Option<SilvanaEventsServiceClient<Channel>>>>, // Silvana RPC service client
    shutdown_flag: Arc<AtomicBool>, // Global shutdown flag for graceful shutdown
    force_shutdown_flag: Arc<AtomicBool>, // Force shutdown flag for immediate termination
    app_instance_filter: Arc<RwLock<Option<String>>>, // Optional filter to only process jobs from a specific app instance
    settle_only: Arc<AtomicBool>, // Flag to run as a dedicated settlement node
    multicall_requests: Arc<Mutex<HashMap<String, MulticallRequests>>>, // Batched job operation requests per app instance
    started_jobs_buffer: Arc<Mutex<VecDeque<StartedJob>>>,  // Buffer of jobs started on blockchain
}

impl SharedState {
    pub fn new() -> Self {
        // Initialize the Silvana RPC client asynchronously using shared client
        let rpc_client = Arc::new(RwLock::new(None));
        let rpc_client_clone = rpc_client.clone();
        tokio::spawn(async move {
            match rpc_client::shared::get_shared_client().await {
                Ok(client) => {
                    let mut lock = rpc_client_clone.write().await;
                    *lock = Some(client);
                    info!("Silvana RPC client initialized successfully using shared connection");
                }
                Err(e) => {
                    error!("Failed to initialize Silvana RPC client: {}", e);
                }
            }
        });

        Self {
            current_agents: Arc::new(RwLock::new(HashMap::new())),
            jobs_tracker: JobsTracker::new(),
            agent_job_db: AgentJobDatabase::new(),
            has_pending_jobs: Arc::new(AtomicBool::new(false)),
            rpc_client,
            shutdown_flag: Arc::new(AtomicBool::new(false)),
            force_shutdown_flag: Arc::new(AtomicBool::new(false)),
            app_instance_filter: Arc::new(RwLock::new(None)),
            settle_only: Arc::new(AtomicBool::new(false)),
            multicall_requests: Arc::new(Mutex::new(HashMap::new())),
            started_jobs_buffer: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    /// Get the coordinator ID
    pub fn get_coordinator_id(&self) -> String {
        sui::SharedSuiState::get_instance()
            .get_coordinator_id()
            .clone()
    }

    /// Get the chain
    pub fn get_chain(&self) -> String {
        sui::SharedSuiState::get_instance().get_chain().clone()
    }

    pub async fn set_current_agent(
        &self,
        session_id: String,
        developer: String,
        agent: String,
        agent_method: String,
    ) {
        let mut current_agents = self.current_agents.write().await;
        let current_agent = CurrentAgent {
            session_id: session_id.clone(),
            developer,
            agent,
            agent_method,
            settlement_chain: None,  // Initially no settlement chain is set
        };
        current_agents.insert(session_id, current_agent.clone());
        debug!(
            "Set current agent for session {}: {}/{}/{}",
            current_agent.session_id,
            current_agent.developer,
            current_agent.agent,
            current_agent.agent_method
        );
    }

    pub async fn clear_current_agent(&self, session_id: &str) {
        let mut current_agents = self.current_agents.write().await;
        if let Some(agent) = current_agents.remove(session_id) {
            debug!(
                "Clearing current agent: {}/{}/{} (session: {})",
                agent.developer, agent.agent, agent.agent_method, session_id
            );
        }
    }

    pub async fn get_current_agent(&self, session_id: &str) -> Option<CurrentAgent> {
        let current_agents = self.current_agents.read().await;
        current_agents.get(session_id).cloned()
    }
    
    /// Update the settlement chain for a current agent
    #[allow(dead_code)]
    pub async fn set_agent_settlement_chain(&self, session_id: &str, chain: String) {
        let mut current_agents = self.current_agents.write().await;
        if let Some(agent) = current_agents.get_mut(session_id) {
            if agent.settlement_chain.is_none() {
                agent.settlement_chain = Some(chain.clone());
                debug!(
                    "Set settlement chain '{}' for agent {}/{}/{} (session: {})",
                    chain, agent.developer, agent.agent, agent.agent_method, session_id
                );
            }
        }
    }

    pub async fn get_current_agent_count(&self) -> usize {
        let current_agents = self.current_agents.read().await;
        current_agents.len()
    }

    // Note: Removed is_agent_running() - we now allow multiple instances of the same agent
    // to run in parallel up to MAX_CONCURRENT_AGENTS limit
    
    /// Get all currently running agents (for shutdown reporting)
    pub async fn get_all_current_agents(&self) -> HashMap<String, CurrentAgent> {
        let current_agents = self.current_agents.read().await;
        current_agents.clone()
    }
    
    /// Get the shutdown flag (for sharing with tasks)
    #[allow(dead_code)]
    pub fn get_shutdown_flag(&self) -> Arc<AtomicBool> {
        self.shutdown_flag.clone()
    }
    
    /// Set the shutdown flag to initiate graceful shutdown
    pub fn set_shutdown(&self) {
        self.shutdown_flag.store(true, Ordering::SeqCst);
    }
    
    /// Check if shutdown has been requested
    pub fn is_shutting_down(&self) -> bool {
        self.shutdown_flag.load(Ordering::SeqCst)
    }
    
    /// Set the force shutdown flag for immediate termination
    pub fn set_force_shutdown(&self) {
        self.force_shutdown_flag.store(true, Ordering::SeqCst);
        // Also set the normal shutdown flag
        self.shutdown_flag.store(true, Ordering::SeqCst);
    }
    
    /// Check if force shutdown has been requested
    pub fn is_force_shutdown(&self) -> bool {
        self.force_shutdown_flag.load(Ordering::SeqCst)
    }
    
    /// Alias for is_force_shutdown
    pub fn is_force_shutting_down(&self) -> bool {
        self.is_force_shutdown()
    }
    
    /// Get the count of current agents
    pub async fn get_current_agents_count(&self) -> usize {
        let current_agents = self.current_agents.read().await;
        current_agents.len()
    }
    
    /// Get agent job database statistics
    pub async fn get_agent_job_stats(&self) -> (usize, usize, usize, usize, usize) {
        self.agent_job_db.get_stats().await
    }

    /// Add a new job to tracking from JobCreatedEvent
    pub async fn add_job(
        &self,
        _job_sequence: u64, // No longer tracking individual job sequences
        developer: String,
        agent: String,
        agent_method: String,
        app_instance: String,
    ) {
        self.jobs_tracker
            .add_job(
                app_instance.clone(),
                developer.clone(),
                agent.clone(),
                agent_method.clone(),
            )
            .await;

        // Set the flag that we have pending jobs
        self.has_pending_jobs.store(true, Ordering::Release);

        debug!(
            "Added app_instance {} for {}/{}/{}",
            app_instance, developer, agent, agent_method
        );
    }

    /// Remove a completed or failed job (no longer tracking individual jobs)
    pub async fn remove_job(&self, _job_sequence: u64) {
        // No longer tracking individual jobs, only app_instances
        // The reconciliation process will handle removing app_instances with no pending jobs
        debug!("Job completion/failure noted (individual jobs not tracked)");
    }

    /// Remove an app_instance when it has no pending jobs
    #[allow(dead_code)]
    pub async fn remove_app_instance(&self, app_instance_id: &str) {
        self.jobs_tracker.remove_app_instance(app_instance_id).await;

        // Check if we still have app_instances with pending jobs
        let count = self.jobs_tracker.app_instances_count().await;
        if count == 0 {
            self.has_pending_jobs.store(false, Ordering::Release);
        }

        info!(
            "Removed app_instance from tracking: {} (remaining: {})",
            app_instance_id, count
        );
    }

    /// Get all app_instances with pending jobs
    pub async fn get_app_instances(&self) -> Vec<String> {
        self.jobs_tracker.get_all_app_instances().await
    }

    /// Get app_instances with pending jobs for the current agent
    #[allow(dead_code)]
    pub async fn get_current_app_instances(&self, session_id: &str) -> Vec<String> {
        if let Some(current_agent) = self.get_current_agent(session_id).await {
            self.jobs_tracker
                .get_app_instances_for_agent_method(
                    &current_agent.developer,
                    &current_agent.agent,
                    &current_agent.agent_method,
                )
                .await
        } else {
            Vec::new()
        }
    }


    /// Get the Silvana RPC client (if initialized)
    pub async fn get_rpc_client(&self) -> Option<SilvanaEventsServiceClient<Channel>> {
        let lock = self.rpc_client.read().await;
        lock.clone()
    }

    /// Get reference to the JobsTracker for direct access
    pub fn get_jobs_tracker(&self) -> &JobsTracker {
        &self.jobs_tracker
    }

    /// Get reference to the AgentJobDatabase for direct access
    pub fn get_agent_job_db(&self) -> &AgentJobDatabase {
        &self.agent_job_db
    }

    /// Fast check if there are pending jobs available
    #[allow(dead_code)]
    pub fn has_pending_jobs_available(&self) -> bool {
        self.has_pending_jobs.load(Ordering::Acquire)
    }

    /// Update the pending jobs flag based on current app_instances count
    pub async fn update_pending_jobs_flag(&self) {
        let count = self.jobs_tracker.app_instances_count().await;
        self.has_pending_jobs.store(count > 0, Ordering::Release);
    }
    
    /// Set the app instance filter
    pub async fn set_app_instance_filter(&self, filter: Option<String>) {
        let mut lock = self.app_instance_filter.write().await;
        if let Some(ref instance) = filter {
            info!("Setting app instance filter to: {}", instance);
        } else {
            info!("Clearing app instance filter");
        }
        *lock = filter;
    }
    
    /// Get the app instance filter
    #[allow(dead_code)]
    pub async fn get_app_instance_filter(&self) -> Option<String> {
        let lock = self.app_instance_filter.read().await;
        lock.clone()
    }
    
    /// Set the settle_only flag
    pub fn set_settle_only(&self, settle_only: bool) {
        if settle_only {
            info!("Running as dedicated settlement node - will only process settlement jobs");
        }
        self.settle_only.store(settle_only, Ordering::Release);
    }
    
    /// Check if running in settle_only mode
    pub fn is_settle_only(&self) -> bool {
        self.settle_only.load(Ordering::Acquire)
    }

    /// Add a create job request to the batch
    #[allow(dead_code)]
    pub async fn add_create_job_request(&self, app_instance: String, request: CreateJobRequest) {
        let mut requests = self.multicall_requests.lock().await;
        let entry = requests.entry(app_instance.clone()).or_insert_with(|| MulticallRequests {
            _app_instance: app_instance.clone(),
            create_jobs: Vec::new(),
            start_jobs: Vec::new(),
            complete_jobs: Vec::new(),
            fail_jobs: Vec::new(),
            terminate_jobs: Vec::new(),
            update_state_for_sequences: Vec::new(),
            submit_proofs: Vec::new(),
            create_app_jobs: Vec::new(),
            create_merge_jobs: Vec::new(),
            last_execution: Instant::now(),
        });
        
        // Check for duplicate job based on key identifying fields and replace if found
        if let Some(existing_index) = entry.create_jobs.iter().position(|job| 
            job.agent == request.agent && 
            job.agent_method == request.agent_method && 
            job.app_instance_method == request.app_instance_method &&
            job.job_sequence == request.job_sequence
        ) {
            debug!("Replacing existing create job request for app_instance {} (agent: {}, method: {})", 
                   app_instance, request.agent, request.agent_method);
            entry.create_jobs[existing_index] = request;
        } else {
            entry.create_jobs.push(request);
        }
        
        debug!("Added create job request for app_instance {}", app_instance);
    }

    /// Add a start job request to the batch
    #[allow(dead_code)]
    pub async fn add_start_job_request(&self, app_instance: String, job_sequence: u64, memory_requirement: u64) {
        let mut requests = self.multicall_requests.lock().await;
        let entry = requests.entry(app_instance.clone()).or_insert_with(|| MulticallRequests {
            _app_instance: app_instance.clone(),
            create_jobs: Vec::new(),
            start_jobs: Vec::new(),
            complete_jobs: Vec::new(),
            fail_jobs: Vec::new(),
            terminate_jobs: Vec::new(),
            update_state_for_sequences: Vec::new(),
            submit_proofs: Vec::new(),
            create_app_jobs: Vec::new(),
            create_merge_jobs: Vec::new(),
            last_execution: Instant::now(),
        });
        
        // Check if job with same sequence already exists and replace it
        if let Some(existing_index) = entry.start_jobs.iter().position(|job| job.job_sequence == job_sequence) {
            debug!("Replacing existing start job request for job {} in app_instance {}", job_sequence, app_instance);
            entry.start_jobs[existing_index] = StartJobRequest {
                job_sequence,
                memory_requirement,
                _timestamp: Instant::now(),
            };
        } else {
            entry.start_jobs.push(StartJobRequest {
                job_sequence,
                memory_requirement,
                _timestamp: Instant::now(),
            });
        }
        
        debug!("Added start job request for job {} in app_instance {} (memory: {:.2} GB)", 
               job_sequence, app_instance, memory_requirement as f64 / (1024.0 * 1024.0 * 1024.0));
    }

    /// Add a complete job request to the batch
    pub async fn add_complete_job_request(&self, app_instance: String, job_sequence: u64) {
        let mut requests = self.multicall_requests.lock().await;
        let entry = requests.entry(app_instance.clone()).or_insert_with(|| MulticallRequests {
            _app_instance: app_instance.clone(),
            create_jobs: Vec::new(),
            start_jobs: Vec::new(),
            complete_jobs: Vec::new(),
            fail_jobs: Vec::new(),
            terminate_jobs: Vec::new(),
            update_state_for_sequences: Vec::new(),
            submit_proofs: Vec::new(),
            create_app_jobs: Vec::new(),
            create_merge_jobs: Vec::new(),
            last_execution: Instant::now(),
        });
        
        entry.complete_jobs.push(CompleteJobRequest {
            job_sequence,
            _timestamp: Instant::now(),
        });
        
        debug!("Added complete job request for job {} in app_instance {}", job_sequence, app_instance);
    }

    /// Add a fail job request to the batch
    pub async fn add_fail_job_request(&self, app_instance: String, job_sequence: u64, error: String) {
        let mut requests = self.multicall_requests.lock().await;
        let entry = requests.entry(app_instance.clone()).or_insert_with(|| MulticallRequests {
            _app_instance: app_instance.clone(),
            create_jobs: Vec::new(),
            start_jobs: Vec::new(),
            complete_jobs: Vec::new(),
            fail_jobs: Vec::new(),
            terminate_jobs: Vec::new(),
            update_state_for_sequences: Vec::new(),
            submit_proofs: Vec::new(),
            create_app_jobs: Vec::new(),
            create_merge_jobs: Vec::new(),
            last_execution: Instant::now(),
        });
        
        // Check if fail job with same sequence already exists and replace it
        if let Some(existing_index) = entry.fail_jobs.iter().position(|job| job.job_sequence == job_sequence) {
            debug!("Replacing existing fail job request for job {} in app_instance {}", job_sequence, app_instance);
            entry.fail_jobs[existing_index] = FailJobRequest {
                job_sequence,
                error,
                _timestamp: Instant::now(),
            };
        } else {
            entry.fail_jobs.push(FailJobRequest {
                job_sequence,
                error,
                _timestamp: Instant::now(),
            });
        }
        
        debug!("Added fail job request for job {} in app_instance {}", job_sequence, app_instance);
    }

    /// Check if a specific job has pending complete or fail requests in multicall queue
    pub async fn has_pending_job_request(&self, app_instance: &str, job_sequence: u64) -> bool {
        let requests = self.multicall_requests.lock().await;
        
        if let Some(app_requests) = requests.get(app_instance) {
            // Check for complete job requests
            let has_complete = app_requests.complete_jobs.iter()
                .any(|req| req.job_sequence == job_sequence);
            
            // Check for fail job requests
            let has_fail = app_requests.fail_jobs.iter()
                .any(|req| req.job_sequence == job_sequence);
            
            has_complete || has_fail
        } else {
            false
        }
    }

    /// Add a terminate job request to the batch
    pub async fn add_terminate_job_request(&self, app_instance: String, job_sequence: u64) {
        let mut requests = self.multicall_requests.lock().await;
        let entry = requests.entry(app_instance.clone()).or_insert_with(|| MulticallRequests {
            _app_instance: app_instance.clone(),
            create_jobs: Vec::new(),
            start_jobs: Vec::new(),
            complete_jobs: Vec::new(),
            fail_jobs: Vec::new(),
            terminate_jobs: Vec::new(),
            update_state_for_sequences: Vec::new(),
            submit_proofs: Vec::new(),
            create_app_jobs: Vec::new(),
            create_merge_jobs: Vec::new(),
            last_execution: Instant::now(),
        });
        
        entry.terminate_jobs.push(TerminateJobRequest {
            job_sequence,
            _timestamp: Instant::now(),
        });
        
        debug!("Added terminate job request for job {} in app_instance {}", job_sequence, app_instance);
    }

    /// Add an update state for sequence request to the batch
    pub async fn add_update_state_for_sequence_request(&self, app_instance: String, request: UpdateStateForSequenceRequest) {
        let mut requests = self.multicall_requests.lock().await;
        let entry = requests.entry(app_instance.clone()).or_insert_with(|| MulticallRequests {
            _app_instance: app_instance.clone(),
            create_jobs: Vec::new(),
            start_jobs: Vec::new(),
            complete_jobs: Vec::new(),
            fail_jobs: Vec::new(),
            terminate_jobs: Vec::new(),
            update_state_for_sequences: Vec::new(),
            submit_proofs: Vec::new(),
            create_app_jobs: Vec::new(),
            create_merge_jobs: Vec::new(),
            last_execution: Instant::now(),
        });
        
        entry.update_state_for_sequences.push(request);
        
        debug!("Added update state for sequence request for app_instance {}", app_instance);
    }
    
    /// Add a submit proof request to the batch
    pub async fn add_submit_proof_request(&self, app_instance: String, request: SubmitProofRequest) {
        let mut requests = self.multicall_requests.lock().await;
        let entry = requests.entry(app_instance.clone()).or_insert_with(|| MulticallRequests {
            _app_instance: app_instance.clone(),
            create_jobs: Vec::new(),
            start_jobs: Vec::new(),
            complete_jobs: Vec::new(),
            fail_jobs: Vec::new(),
            terminate_jobs: Vec::new(),
            update_state_for_sequences: Vec::new(),
            submit_proofs: Vec::new(),
            create_app_jobs: Vec::new(),
            create_merge_jobs: Vec::new(),
            last_execution: Instant::now(),
        });
        
        entry.submit_proofs.push(request);
        
        debug!("Added submit proof request for app_instance {}", app_instance);
    }

    /// Add a create app job request to the batch (for settlement and other app jobs)
    pub async fn add_create_app_job_request(&self, app_instance: String, request: CreateAppJobRequest) {
        let mut requests = self.multicall_requests.lock().await;
        let entry = requests.entry(app_instance.clone()).or_insert_with(|| MulticallRequests {
            _app_instance: app_instance.clone(),
            create_jobs: Vec::new(),
            start_jobs: Vec::new(),
            complete_jobs: Vec::new(),
            fail_jobs: Vec::new(),
            terminate_jobs: Vec::new(),
            update_state_for_sequences: Vec::new(),
            submit_proofs: Vec::new(),
            create_app_jobs: Vec::new(),
            create_merge_jobs: Vec::new(),
            last_execution: Instant::now(),
        });
        
        // For settlement jobs, check for duplicates with same chain and replace them
        if request.method_name == "settle" {
            if let Some(ref chain) = request.settlement_chain {
                if let Some(existing_index) = entry.create_app_jobs.iter().position(|job| 
                    job.method_name == "settle" && job.settlement_chain.as_ref() == Some(chain)
                ) {
                    debug!("Replacing existing settlement job for app_instance {} chain '{}'", app_instance, chain);
                    entry.create_app_jobs[existing_index] = request;
                } else {
                    entry.create_app_jobs.push(request);
                }
            } else {
                debug!("Settlement job without chain for app_instance {} - adding anyway", app_instance);
                entry.create_app_jobs.push(request);
            }
        } else {
            // For non-settlement jobs, just add normally (could add dedup logic later if needed)
            entry.create_app_jobs.push(request);
        }
        
        debug!("Added create app job request for app_instance {}", app_instance);
    }
    
    /// Add a create merge job request to the batch
    pub async fn add_create_merge_job_request(&self, app_instance: String, request: CreateMergeJobRequest) {
        let mut requests = self.multicall_requests.lock().await;
        let entry = requests.entry(app_instance.clone()).or_insert_with(|| MulticallRequests {
            _app_instance: app_instance.clone(),
            create_jobs: Vec::new(),
            start_jobs: Vec::new(),
            complete_jobs: Vec::new(),
            fail_jobs: Vec::new(),
            terminate_jobs: Vec::new(),
            update_state_for_sequences: Vec::new(),
            submit_proofs: Vec::new(),
            create_app_jobs: Vec::new(),
            create_merge_jobs: Vec::new(),
            last_execution: Instant::now(),
        });
        
        // Check for duplicate merge job based on block_number and sequences and replace if found
        if let Some(existing_index) = entry.create_merge_jobs.iter().position(|job| 
            job.block_number == request.block_number && 
            job.sequences == request.sequences
        ) {
            debug!("Replacing existing create merge job request for app_instance {} (block: {}, sequences: {:?})", 
                   app_instance, request.block_number, request.sequences);
            entry.create_merge_jobs[existing_index] = request;
        } else {
            entry.create_merge_jobs.push(request);
        }
        
        debug!("Added create merge job request for app_instance {}", app_instance);
    }

    /// Get and clear all pending multicall requests for an app instance
    pub async fn take_multicall_requests(&self, app_instance: &str) -> Option<MulticallRequests> {
        let mut requests = self.multicall_requests.lock().await;
        requests.remove(app_instance)
    }

    /// Check if any app instance has pending requests ready for execution
    pub async fn has_pending_multicall_requests(&self, interval: Duration) -> Vec<String> {
        let requests = self.multicall_requests.lock().await;
        let now = Instant::now();
        
        requests
            .iter()
            .filter(|(_, req)| {
                // Check if enough time has passed since last execution
                now.duration_since(req.last_execution) >= interval &&
                // Check if there are any pending operations
                (!req.create_jobs.is_empty() ||
                 !req.start_jobs.is_empty() || 
                 !req.complete_jobs.is_empty() || 
                 !req.fail_jobs.is_empty() || 
                 !req.terminate_jobs.is_empty() ||
                 !req.submit_proofs.is_empty() ||
                 !req.create_merge_jobs.is_empty())
            })
            .map(|(app_instance, _)| app_instance.clone())
            .collect()
    }

    /// Get all app instances with pending multicall requests (no interval check)
    #[allow(dead_code)]
    pub async fn get_all_pending_multicall_app_instances(&self) -> Vec<String> {
        let requests = self.multicall_requests.lock().await;
        
        requests
            .iter()
            .filter(|(_, req)| {
                // Check if there are any pending operations
                !req.create_jobs.is_empty() ||
                !req.start_jobs.is_empty() || 
                !req.complete_jobs.is_empty() || 
                !req.fail_jobs.is_empty() || 
                !req.terminate_jobs.is_empty() ||
                !req.update_state_for_sequences.is_empty() ||
                !req.submit_proofs.is_empty() ||
                !req.create_app_jobs.is_empty() ||
                !req.create_merge_jobs.is_empty()
            })
            .map(|(app_instance, _)| app_instance.clone())
            .collect()
    }

    /// Update last execution time for an app instance
    #[allow(dead_code)]
    pub async fn update_multicall_execution_time(&self, app_instance: &str) {
        let mut requests = self.multicall_requests.lock().await;
        if let Some(entry) = requests.get_mut(app_instance) {
            entry.last_execution = Instant::now();
        }
    }

    /// Add started jobs to the buffer
    pub async fn add_started_jobs(&self, jobs: Vec<StartedJob>) {
        let mut buffer = self.started_jobs_buffer.lock().await;
        for job in jobs {
            debug!("Adding started job to buffer: app_instance={}, sequence={}, memory={}",
                  job.app_instance, job.job_sequence, job.memory_requirement);
            buffer.push_back(job);
        }
        debug!("Started jobs buffer now contains {} jobs", buffer.len());
    }
    
    /// Get the next started job from the buffer (thread-safe)
    pub async fn get_next_started_job(&self) -> Option<StartedJob> {
        let mut buffer = self.started_jobs_buffer.lock().await;
        let job = buffer.pop_front();
        if let Some(ref j) = job {
            debug!("Retrieved started job from buffer: app_instance={}, sequence={}, memory={}",
                  j.app_instance, j.job_sequence, j.memory_requirement);
            debug!("Started jobs buffer now contains {} jobs", buffer.len());
        }
        job
    }
    
    /// Get the number of jobs in the buffer
    pub async fn get_started_jobs_count(&self) -> usize {
        self.started_jobs_buffer.lock().await.len()
    }
    
    /// Clear all jobs from the buffer (for emergency use)
    #[allow(dead_code)]
    pub async fn clear_started_jobs_buffer(&self) {
        let mut buffer = self.started_jobs_buffer.lock().await;
        let count = buffer.len();
        buffer.clear();
        if count > 0 {
            info!("Cleared {} jobs from started jobs buffer", count);
        }
    }
}
