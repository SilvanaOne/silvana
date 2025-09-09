use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq)]
pub enum ContainerState {
    Loading,  // Image pulling, preparing
    Running,  // Docker container is actually running
}

#[derive(Debug, Clone)]
pub struct TrackedContainer {
    pub session_id: String,
    pub job_id: String,
    pub state: ContainerState,
}

/// Global Docker state for tracking containers
pub struct DockerState {
    containers: Arc<RwLock<HashMap<String, TrackedContainer>>>,
}

impl DockerState {
    fn new() -> Self {
        Self {
            containers: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Track a new container as loading
    pub async fn track_loading(&self, session_id: String, job_id: String) {
        let mut containers = self.containers.write().await;
        containers.insert(
            session_id.clone(),
            TrackedContainer {
                session_id: session_id.clone(),
                job_id,
                state: ContainerState::Loading,
            },
        );
    }
    
    /// Update container state to running
    pub async fn mark_running(&self, session_id: &str) {
        let mut containers = self.containers.write().await;
        if let Some(container) = containers.get_mut(session_id) {
            container.state = ContainerState::Running;
        }
    }
    
    /// Remove container from tracking
    pub async fn remove_container(&self, session_id: &str) {
        let mut containers = self.containers.write().await;
        containers.remove(session_id);
    }
    
    /// Get count of loading and running containers
    pub async fn get_container_counts(&self) -> (usize, usize) {
        let containers = self.containers.read().await;
        let loading = containers
            .values()
            .filter(|c| c.state == ContainerState::Loading)
            .count();
        let running = containers
            .values()
            .filter(|c| c.state == ContainerState::Running)
            .count();
        (loading, running)
    }
    
    /// Get total container count
    pub async fn get_total_count(&self) -> usize {
        let containers = self.containers.read().await;
        containers.len()
    }
}

/// Global singleton instance
static DOCKER_STATE: OnceLock<DockerState> = OnceLock::new();

/// Get the global Docker state instance
pub fn get_docker_state() -> &'static DockerState {
    DOCKER_STATE.get_or_init(DockerState::new)
}