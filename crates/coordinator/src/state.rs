use std::collections::HashSet;
use std::sync::Arc;
use sui_rpc::Client;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct CurrentAgent {
    pub developer: String,
    pub agent: String,
    pub agent_method: String,
}

#[derive(Clone)]
pub struct SharedState {
    current_agent: Arc<RwLock<Option<CurrentAgent>>>,
    app_instances: Arc<RwLock<HashSet<String>>>,
    current_app_instances: Arc<RwLock<HashSet<String>>>,  // App instances for current agent only
    sui_client: Client,  // Sui client (cloneable)
}

impl SharedState {
    pub fn new(sui_client: Client) -> Self {
        Self {
            current_agent: Arc::new(RwLock::new(None)),
            app_instances: Arc::new(RwLock::new(HashSet::new())),
            current_app_instances: Arc::new(RwLock::new(HashSet::new())),
            sui_client,
        }
    }

    pub async fn set_current_agent(&self, developer: String, agent: String, agent_method: String) {
        let mut current = self.current_agent.write().await;
        *current = Some(CurrentAgent {
            developer,
            agent,
            agent_method,
        });
        tracing::info!(
            "Set current agent: {}/{}/{}",
            current.as_ref().unwrap().developer,
            current.as_ref().unwrap().agent,
            current.as_ref().unwrap().agent_method
        );
    }

    pub async fn clear_current_agent(&self) {
        let mut current = self.current_agent.write().await;
        if let Some(agent) = current.as_ref() {
            tracing::info!(
                "Clearing current agent: {}/{}/{}",
                agent.developer,
                agent.agent,
                agent.agent_method
            );
        }
        *current = None;
        
        // Also clear current_app_instances when clearing current_agent
        let mut current_instances = self.current_app_instances.write().await;
        if !current_instances.is_empty() {
            tracing::info!("Clearing {} current_app_instances", current_instances.len());
            current_instances.clear();
        }
    }

    pub async fn get_current_agent(&self) -> Option<CurrentAgent> {
        let current = self.current_agent.read().await;
        current.clone()
    }

    pub async fn add_app_instance(
        &self, 
        app_instance_id: String,
        developer: String,
        agent: String,
        agent_method: String,
    ) -> bool {
        // Always add to global app_instances
        let mut instances = self.app_instances.write().await;
        let is_new = instances.insert(app_instance_id.clone());
        if is_new {
            tracing::info!("Added app_instance to tracking: {}", app_instance_id);
        } else {
            tracing::debug!("App_instance already tracked: {}", app_instance_id);
        }
        
        // Check if this matches the current agent and add to current_app_instances
        let current = self.current_agent.read().await;
        if let Some(current_agent) = current.as_ref() {
            if current_agent.developer == developer 
                && current_agent.agent == agent 
                && current_agent.agent_method == agent_method {
                
                let mut current_instances = self.current_app_instances.write().await;
                let is_new_current = current_instances.insert(app_instance_id.clone());
                if is_new_current {
                    tracing::info!(
                        "Added app_instance {} to current agent tracking ({}/{}/{})",
                        app_instance_id, developer, agent, agent_method
                    );
                }
            }
        }
        
        is_new
    }

    pub async fn remove_app_instance(&self, app_instance_id: &str) -> bool {
        // Remove from global app_instances
        let mut instances = self.app_instances.write().await;
        let was_removed = instances.remove(app_instance_id);
        if was_removed {
            tracing::info!("Removed app_instance from tracking: {}", app_instance_id);
        } else {
            tracing::debug!("App_instance was not tracked: {}", app_instance_id);
        }
        
        // Also remove from current_app_instances if present
        let mut current_instances = self.current_app_instances.write().await;
        let was_removed_current = current_instances.remove(app_instance_id);
        if was_removed_current {
            tracing::info!("Removed app_instance from current tracking: {}", app_instance_id);
        }
        
        was_removed
    }

    pub async fn get_app_instances(&self) -> HashSet<String> {
        let instances = self.app_instances.read().await;
        instances.clone()
    }

    pub async fn has_app_instance(&self, app_instance_id: &str) -> bool {
        let instances = self.app_instances.read().await;
        instances.contains(app_instance_id)
    }

    pub async fn app_instances_count(&self) -> usize {
        let instances = self.app_instances.read().await;
        instances.len()
    }

    pub async fn get_current_app_instances(&self) -> HashSet<String> {
        let instances = self.current_app_instances.read().await;
        instances.clone()
    }

    pub async fn has_current_app_instance(&self, app_instance_id: &str) -> bool {
        let instances = self.current_app_instances.read().await;
        instances.contains(app_instance_id)
    }

    pub async fn current_app_instances_count(&self) -> usize {
        let instances = self.current_app_instances.read().await;
        instances.len()
    }

    pub fn get_sui_client(&self) -> Client {
        self.sui_client.clone()
    }
}