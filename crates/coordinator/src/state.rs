use crate::agent::AgentJobDatabase;
use crate::coordination_manager::CoordinationManager;
use crate::jobs::JobsTracker;
use proto::silvana_rpc_service_client::SilvanaRpcServiceClient;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{Mutex, RwLock};
use tokio::time::Instant;
use tonic::transport::Channel;
use tracing::{debug, error, info, warn};

/// Normalize app instance ID to always have 0x prefix
pub fn normalize_app_instance_id(app_instance: &str) -> String {
    if app_instance.starts_with("0x") {
        app_instance.to_string()
    } else {
        format!("0x{}", app_instance)
    }
}

#[derive(Debug, Clone)]
pub struct CurrentAgent {
    pub session_id: String,
    pub developer: String,
    pub agent: String,
    pub agent_method: String,
    pub settlement_chain: Option<String>, // Track which chain this session is settling for
}

/// Information about a job that has been started on the blockchain
#[derive(Debug, Clone)]
pub struct StartedJob {
    pub app_instance: String,
    pub job_sequence: u64,
    pub memory_requirement: u64,
    /// Job type (e.g., "settle", "merge", or other app_instance_method)
    pub job_type: String,
    /// Block number associated with the job
    pub block_number: Option<u64>,
    /// Sequences array (helpful for debugging and prioritization)
    pub sequences: Option<Vec<u64>>,
    /// Coordination layer ID this job belongs to
    pub layer_id: String,
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
    pub job_type: String,
    pub block_number: Option<u64>,
    pub sequences: Option<Vec<u64>>,
    pub layer_id: String,
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
    pub retry_count: u32,
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
    pub sequences: Vec<u64>,  // Combined sequences
    pub sequences1: Vec<u64>, // First proof sequences
    pub sequences2: Vec<u64>, // Second proof sequences
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
}

#[derive(Clone)]
pub struct SharedState {
    // Track multiple current agents by session_id
    current_agents: Arc<RwLock<HashMap<String, CurrentAgent>>>,
    jobs_tracker: JobsTracker,
    agent_job_db: AgentJobDatabase, // Memory database for agent job tracking
    has_pending_jobs: Arc<AtomicBool>, // Fast check for pending jobs availability
    rpc_client: Arc<RwLock<Option<SilvanaRpcServiceClient<Channel>>>>, // Silvana RPC service client
    shutdown_flag: Arc<AtomicBool>, // Global shutdown flag for graceful shutdown
    force_shutdown_flag: Arc<AtomicBool>, // Force shutdown flag for immediate termination
    multicall_completed: Arc<AtomicBool>, // Flag set when multicall processor has exited
    app_instance_filter: Arc<RwLock<Option<String>>>, // Optional filter to only process jobs from a specific app instance
    settle_only: Arc<AtomicBool>,                     // Flag to run as a dedicated settlement node
    multicall_requests: Arc<Mutex<HashMap<String, MulticallRequests>>>, // Batched job operation requests per app instance
    started_jobs_buffer: Arc<Mutex<VecDeque<StartedJob>>>, // Buffer of jobs started on blockchain
    last_multicall_timestamp: Arc<Mutex<Instant>>,         // Timestamp of last multicall execution

    // Optional coordination manager for multi-layer support
    coordination_manager: Option<Arc<CoordinationManager>>,
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
            multicall_completed: Arc::new(AtomicBool::new(false)),
            app_instance_filter: Arc::new(RwLock::new(None)),
            settle_only: Arc::new(AtomicBool::new(false)),
            multicall_requests: Arc::new(Mutex::new(HashMap::new())),
            started_jobs_buffer: Arc::new(Mutex::new(VecDeque::new())),
            last_multicall_timestamp: Arc::new(Mutex::new(Instant::now())),
            coordination_manager: None,
        }
    }

    /// Create new SharedState with CoordinationManager for multi-layer support
    pub fn new_with_manager(manager: Arc<CoordinationManager>) -> Self {
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
            jobs_tracker: JobsTracker::new_with_manager(manager.clone()),
            agent_job_db: AgentJobDatabase::new(),
            has_pending_jobs: Arc::new(AtomicBool::new(false)),
            rpc_client,
            shutdown_flag: Arc::new(AtomicBool::new(false)),
            force_shutdown_flag: Arc::new(AtomicBool::new(false)),
            multicall_completed: Arc::new(AtomicBool::new(false)),
            app_instance_filter: Arc::new(RwLock::new(None)),
            settle_only: Arc::new(AtomicBool::new(false)),
            multicall_requests: Arc::new(Mutex::new(HashMap::new())),
            started_jobs_buffer: Arc::new(Mutex::new(VecDeque::new())),
            last_multicall_timestamp: Arc::new(Mutex::new(Instant::now())),
            coordination_manager: Some(manager),
        }
    }

    /// Get the coordination manager if available
    pub fn get_coordination_manager(&self) -> Option<Arc<CoordinationManager>> {
        self.coordination_manager.clone()
    }

    /// Get the coordinator ID (returns None if not available for read-only operations)
    pub fn get_coordinator_id(&self) -> Option<String> {
        sui::SharedSuiState::get_instance()
            .get_coordinator_id()
            .map(|s| s.clone())
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
            settlement_chain: None, // Initially no settlement chain is set
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

    /// Update only the settlement chain for an existing session
    /// This ensures the session's agent identity cannot be corrupted
    pub async fn set_settlement_chain_for_session(
        &self,
        session_id: &str,
        settlement_chain: Option<String>,
    ) -> Result<(), String> {
        let mut current_agents = self.current_agents.write().await;

        // Get existing agent or return error
        let existing_agent = current_agents
            .get(session_id)
            .ok_or_else(|| format!("Session {} not found", session_id))?;

        // Check if session already has a different settlement chain
        if let Some(existing_chain) = &existing_agent.settlement_chain {
            if let Some(new_chain) = &settlement_chain {
                if existing_chain != new_chain {
                    return Err(format!(
                        "Session {} is already locked to chain '{}', cannot change to '{}'",
                        session_id, existing_chain, new_chain
                    ));
                }
            }
            // If already has a chain and new is None or same, keep existing
            return Ok(());
        }

        // Create updated agent with same identity but new settlement chain
        let updated_agent = CurrentAgent {
            session_id: session_id.to_string(),
            developer: existing_agent.developer.clone(),
            agent: existing_agent.agent.clone(),
            agent_method: existing_agent.agent_method.clone(),
            settlement_chain: settlement_chain.clone(),
        };

        current_agents.insert(session_id.to_string(), updated_agent.clone());

        if let Some(chain) = settlement_chain {
            debug!(
                "Set settlement chain '{}' for session {}: {}/{}/{}",
                chain,
                session_id,
                updated_agent.developer,
                updated_agent.agent,
                updated_agent.agent_method
            );
        }

        Ok(())
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

    /// Clear all current agents (used during force shutdown to clear phantom agents)
    pub async fn clear_all_current_agents(&self) {
        let mut current_agents = self.current_agents.write().await;
        let count = current_agents.len();
        if count > 0 {
            warn!(
                "Force clearing all {} current agents (phantom cleanup)",
                count
            );
            current_agents.clear();
        }
    }

    pub async fn get_current_agent(&self, session_id: &str) -> Option<CurrentAgent> {
        let current_agents = self.current_agents.read().await;
        current_agents.get(session_id).cloned()
    }

    pub async fn get_current_agent_count(&self) -> usize {
        let current_agents = self.current_agents.read().await;
        current_agents.len()
    }

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

    /// Set the multicall completed flag to signal that multicall processor has exited
    pub fn set_multicall_completed(&self) {
        self.multicall_completed.store(true, Ordering::SeqCst);
    }

    /// Check if multicall processor has completed
    pub fn is_multicall_completed(&self) -> bool {
        self.multicall_completed.load(Ordering::SeqCst)
    }

    /// Get the count of current agents
    pub async fn get_current_agents_count(&self) -> usize {
        let current_agents = self.current_agents.read().await;
        current_agents.len()
    }

    /// Get agent job database statistics
    pub async fn get_running_jobs(&self) -> usize {
        self.agent_job_db.get_running_jobs().await
    }

    /// Add a new job to tracking from JobCreatedEvent
    pub async fn add_job(
        &self,
        developer: String,
        agent: String,
        agent_method: String,
        app_instance: String,
        layer_id: String,
    ) {
        self.jobs_tracker
            .add_job(
                app_instance.clone(),
                developer.clone(),
                agent.clone(),
                agent_method.clone(),
                layer_id.clone(),
            )
            .await;

        // Set the flag that we have pending jobs
        self.has_pending_jobs.store(true, Ordering::Release);

        debug!(
            "Added app_instance {} for {}/{}/{} on layer {}",
            app_instance, developer, agent, agent_method, layer_id
        );
    }

    /// Remove an app_instance when it has no pending jobs
    #[allow(dead_code)]
    pub async fn remove_app_instance(&self, app_instance_id: &str) {
        let app_instance_id = normalize_app_instance_id(app_instance_id);
        self.jobs_tracker
            .remove_app_instance(&app_instance_id)
            .await;

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

    /// Get app_instances with pending jobs for a specific coordination layer
    pub async fn get_app_instances_for_layer(&self, layer_id: &str) -> Vec<String> {
        self.jobs_tracker.get_app_instances_for_layer(layer_id).await
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
    pub async fn get_rpc_client(&self) -> Option<SilvanaRpcServiceClient<Channel>> {
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

    /// Check if there are any settle jobs pending in the multicall requests
    pub async fn has_settle_jobs_pending(&self) -> bool {
        let requests = self.multicall_requests.lock().await;
        for (_app_instance, req) in requests.iter() {
            // Check if any create_app_jobs are settle jobs
            for create_job in &req.create_app_jobs {
                if create_job.method_name == "settle" {
                    return true;
                }
            }
            // In settle mode, any start jobs should be settlement-related
            if self.is_settle_only() && !req.start_jobs.is_empty() {
                return true;
            }
        }
        false
    }

    /// Add a create job request to the batch
    #[allow(dead_code)]
    pub async fn add_create_job_request(&self, app_instance: String, request: CreateJobRequest) {
        let app_instance = normalize_app_instance_id(&app_instance);
        let mut requests = self.multicall_requests.lock().await;
        let entry = requests
            .entry(app_instance.clone())
            .or_insert_with(|| MulticallRequests {
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
            });

        // Check for duplicate job based on key identifying fields and replace if found
        if let Some(existing_index) = entry.create_jobs.iter().position(|job| {
            job.agent == request.agent
                && job.agent_method == request.agent_method
                && job.app_instance_method == request.app_instance_method
                && job.job_sequence == request.job_sequence
        }) {
            debug!(
                "Replacing existing create job request for app_instance {} (agent: {}, method: {})",
                app_instance, request.agent, request.agent_method
            );
            entry.create_jobs[existing_index] = request;
        } else {
            entry.create_jobs.push(request);
        }

        debug!("Added create job request for app_instance {}", app_instance);
    }

    /// Add a start job request to the batch
    pub async fn add_start_job_request(
        &self,
        app_instance: String,
        job_sequence: u64,
        memory_requirement: u64,
        job_type: String,
        block_number: Option<u64>,
        sequences: Option<Vec<u64>>,
        layer_id: String,
    ) {
        let app_instance = normalize_app_instance_id(&app_instance);
        let mut requests = self.multicall_requests.lock().await;
        let entry = requests
            .entry(app_instance.clone())
            .or_insert_with(|| MulticallRequests {
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
            });

        // Check if job with same sequence already exists and replace it
        if let Some(existing_index) = entry
            .start_jobs
            .iter()
            .position(|job| job.job_sequence == job_sequence)
        {
            debug!(
                "Replacing existing start job request for job {} in app_instance {}",
                job_sequence, app_instance
            );
            entry.start_jobs[existing_index] = StartJobRequest {
                job_sequence,
                memory_requirement,
                job_type,
                block_number,
                sequences,
                layer_id: layer_id.clone(),
                _timestamp: Instant::now(),
            };
        } else {
            entry.start_jobs.push(StartJobRequest {
                job_sequence,
                memory_requirement,
                job_type,
                block_number,
                sequences,
                layer_id,
                _timestamp: Instant::now(),
            });
        }

        debug!(
            "Added start job request for job {} in app_instance {} (memory: {:.2} GB)",
            job_sequence,
            app_instance,
            memory_requirement as f64 / (1024.0 * 1024.0 * 1024.0)
        );
    }

    /// Add a complete job request to the batch
    pub async fn add_complete_job_request(&self, app_instance: String, job_sequence: u64) {
        let app_instance = normalize_app_instance_id(&app_instance);
        let mut requests = self.multicall_requests.lock().await;
        let entry = requests
            .entry(app_instance.clone())
            .or_insert_with(|| MulticallRequests {
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
            });

        entry.complete_jobs.push(CompleteJobRequest {
            job_sequence,
            _timestamp: Instant::now(),
        });

        info!(
            "Added complete job request for job {} in app_instance {}",
            job_sequence, app_instance
        );
    }

    /// Add a fail job request to the batch
    pub async fn add_fail_job_request(
        &self,
        app_instance: String,
        job_sequence: u64,
        error: String,
    ) {
        let app_instance = normalize_app_instance_id(&app_instance);
        let mut requests = self.multicall_requests.lock().await;
        let entry = requests
            .entry(app_instance.clone())
            .or_insert_with(|| MulticallRequests {
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
            });

        // Check if fail job with same sequence already exists and replace it
        if let Some(existing_index) = entry
            .fail_jobs
            .iter()
            .position(|job| job.job_sequence == job_sequence)
        {
            debug!(
                "Replacing existing fail job request for job {} in app_instance {}",
                job_sequence, app_instance
            );
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

        debug!(
            "Added fail job request for job {} in app_instance {}",
            job_sequence, app_instance
        );
    }

    /// Check if a specific job has pending complete or fail requests in multicall queue
    #[allow(dead_code)]
    pub async fn has_pending_job_request(&self, app_instance: &str, job_sequence: u64) -> bool {
        let app_instance = normalize_app_instance_id(app_instance);
        let requests = self.multicall_requests.lock().await;

        if let Some(app_requests) = requests.get(&app_instance) {
            // Check for complete job requests
            let has_complete = app_requests
                .complete_jobs
                .iter()
                .any(|req| req.job_sequence == job_sequence);

            // Check for fail job requests
            let has_fail = app_requests
                .fail_jobs
                .iter()
                .any(|req| req.job_sequence == job_sequence);

            has_complete || has_fail
        } else {
            false
        }
    }

    /// Add a terminate job request to the batch
    pub async fn add_terminate_job_request(&self, app_instance: String, job_sequence: u64) {
        let app_instance = normalize_app_instance_id(&app_instance);
        let mut requests = self.multicall_requests.lock().await;
        let entry = requests
            .entry(app_instance.clone())
            .or_insert_with(|| MulticallRequests {
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
            });

        entry.terminate_jobs.push(TerminateJobRequest {
            job_sequence,
            _timestamp: Instant::now(),
        });

        debug!(
            "Added terminate job request for job {} in app_instance {}",
            job_sequence, app_instance
        );
    }

    /// Add an update state for sequence request to the batch
    pub async fn add_update_state_for_sequence_request(
        &self,
        app_instance: String,
        request: UpdateStateForSequenceRequest,
    ) {
        let app_instance = normalize_app_instance_id(&app_instance);
        let mut requests = self.multicall_requests.lock().await;
        let entry = requests
            .entry(app_instance.clone())
            .or_insert_with(|| MulticallRequests {
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
            });

        entry.update_state_for_sequences.push(request);

        debug!(
            "Added update state for sequence request for app_instance {}",
            app_instance
        );
    }

    /// Add a submit proof request to the batch
    pub async fn add_submit_proof_request(
        &self,
        app_instance: String,
        request: SubmitProofRequest,
    ) {
        let app_instance = normalize_app_instance_id(&app_instance);
        let mut requests = self.multicall_requests.lock().await;
        let entry = requests
            .entry(app_instance.clone())
            .or_insert_with(|| MulticallRequests {
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
            });

        entry.submit_proofs.push(request);

        debug!(
            "Added submit proof request for app_instance {}",
            app_instance
        );
    }

    /// Add a create app job request to the batch (for settlement and other app jobs)
    pub async fn add_create_app_job_request(
        &self,
        app_instance: String,
        request: CreateAppJobRequest,
    ) {
        let app_instance = normalize_app_instance_id(&app_instance);
        let mut requests = self.multicall_requests.lock().await;
        let entry = requests
            .entry(app_instance.clone())
            .or_insert_with(|| MulticallRequests {
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
            });

        // For settlement jobs, check for duplicates with same chain and replace them
        if request.method_name == "settle" {
            if let Some(ref chain) = request.settlement_chain {
                if let Some(existing_index) = entry.create_app_jobs.iter().position(|job| {
                    job.method_name == "settle" && job.settlement_chain.as_ref() == Some(chain)
                }) {
                    debug!(
                        "Replacing existing settlement job for app_instance {} chain '{}'",
                        app_instance, chain
                    );
                    entry.create_app_jobs[existing_index] = request;
                } else {
                    entry.create_app_jobs.push(request);
                }
            } else {
                debug!(
                    "Settlement job without chain for app_instance {} - adding anyway",
                    app_instance
                );
                entry.create_app_jobs.push(request);
            }
        } else {
            // For non-settlement jobs, just add normally (could add dedup logic later if needed)
            entry.create_app_jobs.push(request);
        }

        debug!(
            "Added create app job request for app_instance {}",
            app_instance
        );
    }

    /// Add a create merge job request to the batch
    pub async fn add_create_merge_job_request(
        &self,
        app_instance: String,
        request: CreateMergeJobRequest,
    ) {
        let app_instance = normalize_app_instance_id(&app_instance);
        let mut requests = self.multicall_requests.lock().await;
        let entry = requests
            .entry(app_instance.clone())
            .or_insert_with(|| MulticallRequests {
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
            });

        // Check for duplicate merge job based on block_number and sequences and replace if found
        if let Some(existing_index) = entry.create_merge_jobs.iter().position(|job| {
            job.block_number == request.block_number && job.sequences == request.sequences
        }) {
            debug!(
                "Replacing existing create merge job request for app_instance {} (block: {}, sequences: {:?})",
                app_instance, request.block_number, request.sequences
            );
            entry.create_merge_jobs[existing_index] = request;
        } else {
            entry.create_merge_jobs.push(request);
        }

        debug!(
            "Added create merge job request for app_instance {}",
            app_instance
        );
    }

    /// Check if any app instance has pending requests ready for execution
    pub async fn has_pending_multicall_requests(&self) -> Vec<String> {
        let requests = self.multicall_requests.lock().await;

        requests
            .iter()
            .filter(|(_, req)| {
                // Check if there are any pending operations (no interval wait)
                !req.create_jobs.is_empty()
                    || !req.start_jobs.is_empty()
                    || !req.complete_jobs.is_empty()
                    || !req.fail_jobs.is_empty()
                    || !req.terminate_jobs.is_empty()
                    || !req.update_state_for_sequences.is_empty()
                    || !req.submit_proofs.is_empty()
                    || !req.create_app_jobs.is_empty()
                    || !req.create_merge_jobs.is_empty()
            })
            .map(|(app_instance, _)| app_instance.clone())
            .collect()
    }

    /// Add started jobs to the buffer
    pub async fn add_started_jobs(&self, jobs: Vec<StartedJob>) {
        let mut buffer = self.started_jobs_buffer.lock().await;
        for job in jobs {
            debug!(
                "Adding started job to buffer: app_instance={}, sequence={}, memory={}",
                job.app_instance, job.job_sequence, job.memory_requirement
            );
            buffer.push_back(job);
        }
        debug!("Started jobs buffer now contains {} jobs", buffer.len());
    }

    /// Get the next started job from the buffer (thread-safe)
    #[allow(dead_code)]
    pub async fn get_next_started_job(&self) -> Option<StartedJob> {
        let mut buffer = self.started_jobs_buffer.lock().await;
        let job = buffer.pop_front();
        if let Some(ref j) = job {
            debug!(
                "Retrieved started job from buffer: app_instance={}, sequence={}, memory={}",
                j.app_instance, j.job_sequence, j.memory_requirement
            );
            debug!("Started jobs buffer now contains {} jobs", buffer.len());
        }
        job
    }

    /// Get the next started job from the buffer using prioritized selection
    /// Priority order:
    /// 1. Settlement jobs (regardless of block)
    /// 2. Jobs from lowest block (sorted by sequence array size, smallest first)
    /// 3. Jobs from next block (sorted by sequence array size), etc.
    pub async fn get_next_prioritized_started_job(&self) -> Option<StartedJob> {
        let mut buffer = self.started_jobs_buffer.lock().await;

        if buffer.is_empty() {
            return None;
        }

        // Convert to Vec for sorting
        let mut jobs: Vec<StartedJob> = buffer.drain(..).collect();

        // Sort jobs by priority
        jobs.sort_by(|a, b| {
            // First priority: settlement jobs
            let a_is_settlement = a.job_type == "settle";
            let b_is_settlement = b.job_type == "settle";

            match (a_is_settlement, b_is_settlement) {
                (true, false) => return std::cmp::Ordering::Less,
                (false, true) => return std::cmp::Ordering::Greater,
                _ => {} // Both settlement or both non-settlement, continue to next criteria
            }

            // Second priority: block number (lower is better)
            match (a.block_number, b.block_number) {
                (Some(a_block), Some(b_block)) => {
                    match a_block.cmp(&b_block) {
                        std::cmp::Ordering::Equal => {
                            // Same block, sort by sequences size (smaller first)
                            let a_size = a.sequences.as_ref().map(|s| s.len()).unwrap_or(0);
                            let b_size = b.sequences.as_ref().map(|s| s.len()).unwrap_or(0);
                            match a_size.cmp(&b_size) {
                                std::cmp::Ordering::Equal => {
                                    // Same size, sort by job sequence
                                    a.job_sequence.cmp(&b.job_sequence)
                                }
                                other => other,
                            }
                        }
                        other => other,
                    }
                }
                (None, Some(_)) => std::cmp::Ordering::Greater, // Jobs with no block go last
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, None) => {
                    // Both have no block, sort by sequences size then job sequence
                    let a_size = a.sequences.as_ref().map(|s| s.len()).unwrap_or(0);
                    let b_size = b.sequences.as_ref().map(|s| s.len()).unwrap_or(0);
                    match a_size.cmp(&b_size) {
                        std::cmp::Ordering::Equal => a.job_sequence.cmp(&b.job_sequence),
                        other => other,
                    }
                }
            }
        });

        // Take the first (highest priority) job
        let selected_job = jobs.remove(0);

        // Put remaining jobs back in buffer
        for job in jobs {
            buffer.push_back(job);
        }

        debug!(
            "Selected prioritized job from buffer: app_instance={}, sequence={}, type={}, block={:?}, sequences_len={}",
            selected_job.app_instance,
            selected_job.job_sequence,
            selected_job.job_type,
            selected_job.block_number,
            selected_job
                .sequences
                .as_ref()
                .map(|s| s.len())
                .unwrap_or(0)
        );
        debug!("Started jobs buffer now contains {} jobs", buffer.len());

        Some(selected_job)
    }

    /// Get a started job from buffer that matches specific agent details
    /// This requires fetching job details from blockchain to check agent match
    /// For settlement jobs, also validates chain consistency with session
    pub async fn get_started_job_for_agent(
        &self,
        developer: &str,
        agent: &str,
        agent_method: &str,
        session_id: &str,
    ) -> Option<StartedJob> {
        let mut buffer = self.started_jobs_buffer.lock().await;
        let mut checked_jobs = Vec::new();
        let mut matching_job = None;

        // Search through buffer for a matching job
        while let Some(started_job) = buffer.pop_front() {
            // Fetch app instance to get jobs table
            match sui::fetch::fetch_app_instance(&started_job.app_instance).await {
                Ok(app_instance) => {
                    if let Some(ref jobs) = app_instance.jobs {
                        // Fetch job details from blockchain
                        match sui::fetch::jobs::fetch_job_by_id(
                            &jobs.jobs_table_id,
                            started_job.job_sequence,
                        )
                        .await
                        {
                            Ok(Some(pending_job)) => {
                                // Check if this job matches the requested agent
                                if pending_job.developer == developer
                                    && pending_job.agent == agent
                                    && pending_job.agent_method == agent_method
                                {
                                    // For settlement jobs, check chain consistency
                                    if pending_job.app_instance_method == "settle" {
                                        // Get the settlement chain for this job
                                        let job_chain = match sui::fetch::app_instance::get_settlement_chain_by_job_sequence(
                                            &started_job.app_instance,
                                            started_job.job_sequence,
                                        ).await {
                                            Ok(chain) => chain,
                                            Err(e) => {
                                                warn!(
                                                    "Failed to get settlement chain for job {}: {}",
                                                    started_job.job_sequence, e
                                                );
                                                None
                                            }
                                        };

                                        // Check if session already has a settlement chain
                                        let current_agents = self.current_agents.read().await;
                                        if let Some(current_agent) = current_agents.get(session_id)
                                        {
                                            if let Some(session_chain) =
                                                &current_agent.settlement_chain
                                            {
                                                // Session is locked to a chain, only accept jobs for that chain
                                                if job_chain.as_ref() != Some(session_chain) {
                                                    debug!(
                                                        "Settlement job {} for chain {:?} doesn't match session chain {}, skipping",
                                                        started_job.job_sequence,
                                                        job_chain,
                                                        session_chain
                                                    );
                                                    checked_jobs.push(started_job);
                                                    continue;
                                                }
                                            }
                                        }
                                    }

                                    debug!(
                                        "Found matching buffer job: app_instance={}, sequence={}, dev={}, agent={}, method={}",
                                        started_job.app_instance,
                                        started_job.job_sequence,
                                        pending_job.developer,
                                        pending_job.agent,
                                        pending_job.agent_method
                                    );
                                    matching_job = Some(started_job);
                                    break;
                                } else {
                                    debug!(
                                        "Buffer job doesn't match: job(dev={}, agent={}, method={}) vs request(dev={}, agent={}, method={})",
                                        pending_job.developer,
                                        pending_job.agent,
                                        pending_job.agent_method,
                                        developer,
                                        agent,
                                        agent_method
                                    );
                                    // Keep this job to put back in buffer
                                    checked_jobs.push(started_job);
                                }
                            }
                            Ok(None) => {
                                error!(
                                    "Job {} not found in app instance {}",
                                    started_job.job_sequence, started_job.app_instance
                                );
                                checked_jobs.push(started_job);
                            }
                            Err(e) => {
                                warn!(
                                    "Failed to fetch job {} from app instance {}: {}",
                                    started_job.job_sequence, started_job.app_instance, e
                                );
                                // Put back jobs we couldn't fetch (might be temporary issue)
                                checked_jobs.push(started_job);
                            }
                        }
                    } else {
                        error!(
                            "App instance {} has no jobs table",
                            started_job.app_instance
                        );
                        checked_jobs.push(started_job);
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to fetch app instance {}: {}",
                        started_job.app_instance, e
                    );
                    // Put back jobs we couldn't fetch (might be temporary issue)
                    checked_jobs.push(started_job);
                }
            }
        }

        // Put back all non-matching jobs at the front of the buffer (preserve order)
        for job in checked_jobs.into_iter().rev() {
            buffer.push_front(job);
        }

        if matching_job.is_some() {
            info!("Started jobs buffer now contains {} jobs", buffer.len());
        } else {
            info!(
                "No matching buffer job found for dev={}, agent={}, method={}",
                developer, agent, agent_method
            );
        }

        matching_job
    }

    /// Get the number of jobs in the buffer
    pub async fn get_started_jobs_count(&self) -> usize {
        self.started_jobs_buffer.lock().await.len()
    }

    /// Get the current size of the started jobs buffer (for metrics)
    pub async fn get_started_jobs_buffer_size(&self) -> usize {
        self.started_jobs_buffer.lock().await.len()
    }

    /// Return operations back to the multicall queue (used when multicall fails)
    pub async fn return_operations_to_queue(&self, operations: sui::MulticallOperations) {
        let app_instance = normalize_app_instance_id(&operations.app_instance);
        let mut requests = self.multicall_requests.lock().await;

        let entry = requests
            .entry(app_instance.clone())
            .or_insert_with(|| MulticallRequests {
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
            });

        // Count items before consuming them
        let create_merge_count = operations.create_merge_jobs.len();
        let submit_proofs_count = operations.submit_proofs.len();
        let update_state_count = operations.update_state_for_sequences.len();
        let start_count = operations.start_job_sequences.len();
        let complete_count = operations.complete_job_sequences.len();
        let fail_count = operations.fail_job_sequences.len();
        let terminate_count = operations.terminate_job_sequences.len();

        // Return create merge jobs
        for (block_number, sequences, sequences1, sequences2, job_description) in
            operations.create_merge_jobs
        {
            entry.create_merge_jobs.push(CreateMergeJobRequest {
                block_number,
                sequences,
                sequences1,
                sequences2,
                job_description,
                _timestamp: Instant::now(),
            });
        }

        // Return submit proofs with incremented retry count
        for (
            block_number,
            sequences,
            merged_sequences_1,
            merged_sequences_2,
            job_id,
            da_hash,
            cpu_cores,
            prover_architecture,
            prover_memory,
            cpu_time,
        ) in operations.submit_proofs
        {
            // Find existing retry count for this proof
            let retry_count = entry
                .submit_proofs
                .iter()
                .find(|p| p.block_number == block_number && p.sequences == sequences)
                .map(|p| p.retry_count)
                .unwrap_or(0)
                + 1;

            // Skip if we've exceeded max retries
            const MAX_RETRY_COUNT: u32 = 5;
            if retry_count > MAX_RETRY_COUNT {
                error!(
                    "Proof submission for block {} sequences {:?} after {} retries",
                    block_number,
                    sequences,
                    retry_count - 1
                );
            }

            entry.submit_proofs.push(SubmitProofRequest {
                block_number,
                sequences,
                merged_sequences_1,
                merged_sequences_2,
                job_id,
                da_hash,
                cpu_cores,
                prover_architecture,
                prover_memory,
                cpu_time,
                _timestamp: Instant::now(),
                retry_count,
            });
        }

        // Return update state operations
        for (sequence, new_state_data, new_data_availability_hash) in
            operations.update_state_for_sequences
        {
            entry
                .update_state_for_sequences
                .push(UpdateStateForSequenceRequest {
                    sequence,
                    new_state_data,
                    new_data_availability_hash,
                    _timestamp: Instant::now(),
                });
        }

        // Return start jobs - we only have sequences and memory requirements, not full metadata
        for (i, job_sequence) in operations.start_job_sequences.iter().enumerate() {
            let memory_requirement = operations
                .start_job_memory_requirements
                .get(i)
                .copied()
                .unwrap_or(0);
            entry.start_jobs.push(StartJobRequest {
                job_sequence: *job_sequence,
                memory_requirement,
                job_type: String::new(), // Lost metadata
                block_number: None,      // Lost metadata
                sequences: None,         // Lost metadata
                layer_id: String::from("unknown"), // Lost metadata - will be recovered on retry
                _timestamp: Instant::now(),
            });
        }

        // Return complete jobs
        for job_sequence in operations.complete_job_sequences {
            entry.complete_jobs.push(CompleteJobRequest {
                job_sequence,
                _timestamp: Instant::now(),
            });
        }

        // Return fail jobs
        for (i, job_sequence) in operations.fail_job_sequences.iter().enumerate() {
            let error_message = operations
                .fail_errors
                .get(i)
                .cloned()
                .unwrap_or_else(|| "Requeued after multicall failure".to_string());
            entry.fail_jobs.push(FailJobRequest {
                job_sequence: *job_sequence,
                error: error_message,
                _timestamp: Instant::now(),
            });
        }

        // Return terminate jobs
        for job_sequence in operations.terminate_job_sequences {
            entry.terminate_jobs.push(TerminateJobRequest {
                job_sequence,
                _timestamp: Instant::now(),
            });
        }

        debug!(
            "Returned operations to queue for app_instance {}: {} create_merge, {} submit_proofs, {} update_state, {} start, {} complete, {} fail, {} terminate",
            app_instance,
            create_merge_count,
            submit_proofs_count,
            update_state_count,
            start_count,
            complete_count,
            fail_count,
            terminate_count,
        );
    }

    /// Get the total number of operations across all app instances
    pub async fn get_total_operations_count(&self) -> usize {
        let requests = self.multicall_requests.lock().await;
        let mut total = 0;
        for (_, req) in requests.iter() {
            total += req.create_jobs.len()
                + req.start_jobs.len()
                + req.complete_jobs.len()
                + req.fail_jobs.len()
                + req.terminate_jobs.len()
                + req.update_state_for_sequences.len()
                + req.submit_proofs.len()
                + req.create_app_jobs.len()
                + req.create_merge_jobs.len();
        }
        total
    }

    /// Update the last multicall execution timestamp
    pub async fn update_last_multicall_timestamp(&self) {
        let mut timestamp = self.last_multicall_timestamp.lock().await;
        *timestamp = Instant::now();
    }

    /// Check if enough time has passed since last multicall (MULTICALL_INTERVAL_SECS)
    pub async fn should_execute_multicall_by_time(&self) -> bool {
        use crate::constants::MULTICALL_INTERVAL_SECS;
        let timestamp = self.last_multicall_timestamp.lock().await;
        timestamp.elapsed().as_secs() >= MULTICALL_INTERVAL_SECS
    }

    /// Check if multicall should execute due to operation limits
    pub async fn should_execute_multicall_by_limit(&self) -> bool {
        let total_operations = self.get_total_operations_count().await;
        total_operations >= sui::get_max_operations_per_multicall()
    }

    /// Get seconds elapsed since last multicall
    pub async fn get_seconds_since_last_multicall(&self) -> u64 {
        let timestamp = self.last_multicall_timestamp.lock().await;
        timestamp.elapsed().as_secs()
    }

    /// Get mutable access to multicall requests (for advanced batching operations)
    pub async fn get_multicall_requests_mut(
        &self,
    ) -> tokio::sync::MutexGuard<'_, HashMap<String, MulticallRequests>> {
        self.multicall_requests.lock().await
    }

    /// Send CoordinationTxEvent to RPC service (fire and forget)
    pub fn send_coordination_tx_event(&self, tx_hash: String) {
        // Get coordinator ID
        let coordinator_id = match self.get_coordinator_id() {
            Some(id) => id,
            None => {
                debug!("Cannot send CoordinationTxEvent: coordinator_id not available");
                return;
            }
        };

        // Clone what we need for the async task
        let self_clone = self.clone();
        let event_timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Spawn async task to send the event
        tokio::spawn(async move {
            let rpc_client = self_clone.get_rpc_client().await;
            if let Some(mut client) = rpc_client {
                let event = proto::Event {
                    event: Some(proto::event::Event::CoordinationTx(
                        proto::CoordinationTxEvent {
                            coordinator_id,
                            tx_hash: tx_hash.clone(),
                            event_timestamp,
                        },
                    )),
                };

                let request = proto::SubmitEventRequest { event: Some(event) };

                match client.submit_event(request).await {
                    Ok(response) => {
                        let resp = response.into_inner();
                        if resp.success {
                            debug!("Sent CoordinationTxEvent for tx_hash {}", tx_hash);
                        } else {
                            warn!("Failed to send CoordinationTxEvent: {}", resp.message);
                        }
                    }
                    Err(e) => {
                        warn!("Failed to send CoordinationTxEvent: {}", e);
                    }
                }
            } else {
                debug!("No RPC client available to send CoordinationTxEvent");
            }
        });
    }

    /// Send CoordinatorMessageEvent to RPC service (fire and forget)
    pub fn send_coordinator_message_event(&self, level: proto::LogLevel, message: String) {
        // Get coordinator ID
        let coordinator_id = match self.get_coordinator_id() {
            Some(id) => id,
            None => {
                debug!("Cannot send CoordinatorMessageEvent: coordinator_id not available");
                return;
            }
        };

        // Clone what we need for the async task
        let self_clone = self.clone();
        let event_timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Spawn async task to send the event
        tokio::spawn(async move {
            let rpc_client = self_clone.get_rpc_client().await;
            if let Some(mut client) = rpc_client {
                let event = proto::Event {
                    event: Some(proto::event::Event::CoordinatorMessage(
                        proto::CoordinatorMessageEvent {
                            coordinator_id,
                            event_timestamp,
                            level: level as i32,
                            message: message.clone(),
                        },
                    )),
                };

                let request = proto::SubmitEventRequest { event: Some(event) };

                match client.submit_event(request).await {
                    Ok(response) => {
                        let resp = response.into_inner();
                        if resp.success {
                            debug!("Sent CoordinatorMessageEvent: {}", message);
                        } else {
                            warn!("Failed to send CoordinatorMessageEvent: {}", resp.message);
                        }
                    }
                    Err(e) => {
                        warn!("Failed to send CoordinatorMessageEvent: {}", e);
                    }
                }
            } else {
                debug!("No RPC client available to send CoordinatorMessageEvent");
            }
        });
    }
}
