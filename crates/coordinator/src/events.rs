use serde::{Deserialize, Serialize};
use sui_rpc::proto::sui::rpc::v2beta2::{Checkpoint, Event};
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinationEvent {
    pub event_type: String,
    pub package_id: String,
    pub module: String,
    pub sender: Option<String>,
    pub timestamp_ms: u64,
    pub checkpoint_seq: u64,
    pub tx_index: usize,
    pub event_index: usize,
    pub parsed_json: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodAddedEvent {
    pub agent_id: String,
    pub developer_name: String,
    pub agent_name: String,
    pub method_name: String,
    pub docker_image: String,
    pub docker_sha256: Option<String>,
    pub min_memory_gb: u16,
    pub min_cpu_cores: u16,
    pub requires_tee: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInstanceEvent {
    pub instance_id: String,
    pub agent_id: String,
    pub method_name: String,
    pub user_action: String,
    pub timestamp: u64,
}

pub fn parse_coordination_events(
    checkpoint: &Checkpoint,
    checkpoint_seq: u64,
    target_package: &str,
    target_module: &str,
) -> Vec<CoordinationEvent> {
    let mut events = Vec::new();
    
    let checkpoint_timestamp_ms = checkpoint
        .summary
        .as_ref()
        .and_then(|s| s.timestamp.as_ref())
        .map(|t| (t.seconds as u64 * 1000) + (t.nanos as u64 / 1_000_000))
        .unwrap_or(0);

    for (tx_index, transaction) in checkpoint.transactions.iter().enumerate() {
        if let Some(tx_events) = &transaction.events {
            for (event_index, event) in tx_events.events.iter().enumerate() {
                if let (Some(package_id), Some(module)) = (&event.package_id, &event.module) {
                    if package_id == target_package && module == target_module {
                        let coordination_event = CoordinationEvent {
                            event_type: event.event_type.clone().unwrap_or_default(),
                            package_id: package_id.clone(),
                            module: module.clone(),
                            sender: event.sender.clone(),
                            timestamp_ms: checkpoint_timestamp_ms,
                            checkpoint_seq,
                            tx_index,
                            event_index,
                            parsed_json: parse_event_json(event),
                        };
                        
                        debug!(
                            "Found coordination event: type={}, checkpoint={}, tx={}, event={}",
                            coordination_event.event_type,
                            checkpoint_seq,
                            tx_index,
                            event_index
                        );
                        
                        events.push(coordination_event);
                    }
                }
            }
        }
    }
    
    if !events.is_empty() {
        info!(
            "Found {} coordination events in checkpoint {}",
            events.len(),
            checkpoint_seq
        );
    }
    
    events
}

fn parse_event_json(_event: &Event) -> Option<serde_json::Value> {
    // The event data would need to be parsed from BCS format
    // For now, we'll return None but this should be implemented
    // based on the actual event structure
    None
}

pub fn extract_docker_config(event: &CoordinationEvent) -> Option<DockerConfig> {
    // Parse the event to extract docker configuration
    // This would parse MethodAddedEvent or similar events
    if event.event_type.contains("MethodAdded") {
        // Parse the JSON data to extract docker image info
        if let Some(json) = &event.parsed_json {
            if let Ok(method_event) = serde_json::from_value::<MethodAddedEvent>(json.clone()) {
                return Some(DockerConfig {
                    image: method_event.docker_image,
                    sha256: method_event.docker_sha256,
                    memory_gb: method_event.min_memory_gb,
                    cpu_cores: method_event.min_cpu_cores,
                    requires_tee: method_event.requires_tee,
                });
            }
        }
    }
    None
}

#[derive(Debug, Clone)]
pub struct DockerConfig {
    pub image: String,
    pub sha256: Option<String>,
    pub memory_gb: u16,
    pub cpu_cores: u16,
    pub requires_tee: bool,
}