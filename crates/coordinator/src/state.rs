use crate::agent::AgentJobDatabase;
use crate::jobs::JobsTracker;
use rpc_client::{RpcClient, RpcClientConfig, create_rpc_client};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::RwLock;
use tracing::{debug, error, info};

// Maximum number of concurrent Docker containers/agents that can run simultaneously
pub const MAX_CONCURRENT_AGENTS: usize = 2;

#[derive(Debug, Clone)]
pub struct CurrentAgent {
    pub session_id: String,
    pub developer: String,
    pub agent: String,
    pub agent_method: String,
}

#[derive(Clone)]
pub struct SharedState {
    // Track multiple current agents by session_id
    current_agents: Arc<RwLock<HashMap<String, CurrentAgent>>>,
    jobs_tracker: JobsTracker,
    agent_job_db: AgentJobDatabase, // Memory database for agent job tracking
    has_pending_jobs: Arc<AtomicBool>, // Fast check for pending jobs availability
    rpc_client: Arc<RwLock<Option<RpcClient>>>, // Silvana RPC service client
}

impl SharedState {
    pub fn new() -> Self {
        // Initialize the Silvana RPC client asynchronously
        let rpc_client = Arc::new(RwLock::new(None));
        let rpc_client_clone = rpc_client.clone();
        tokio::spawn(async move {
            let config = RpcClientConfig::from_env();
            match create_rpc_client(config).await {
                Ok(client) => {
                    let mut lock = rpc_client_clone.write().await;
                    *lock = Some(client);
                    info!("Silvana RPC client initialized successfully");
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
        };
        current_agents.insert(session_id, current_agent.clone());
        info!(
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
            info!(
                "Clearing current agent: {}/{}/{} (session: {})",
                agent.developer, agent.agent, agent.agent_method, session_id
            );
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

    pub async fn is_agent_running(&self, developer: &str, agent: &str, agent_method: &str) -> bool {
        let current_agents = self.current_agents.read().await;
        current_agents
            .values()
            .any(|a| a.developer == developer && a.agent == agent && a.agent_method == agent_method)
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

        info!(
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
    pub async fn get_rpc_client(&self) -> Option<RpcClient> {
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
    pub fn has_pending_jobs_available(&self) -> bool {
        self.has_pending_jobs.load(Ordering::Acquire)
    }

    /// Update the pending jobs flag based on current app_instances count
    pub async fn update_pending_jobs_flag(&self) {
        let count = self.jobs_tracker.app_instances_count().await;
        self.has_pending_jobs.store(count > 0, Ordering::Release);
    }
}
