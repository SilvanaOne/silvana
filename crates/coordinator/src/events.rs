use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sui_rpc::proto::sui::rpc::v2::{Checkpoint, Event};
use tracing::{debug, error};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinationEvent {
    pub event_type: String,
    //pub package_id: String,
    //pub module: String,
    //pub sender: Option<String>,
    //pub timestamp_ms: u64,
    pub checkpoint_seq: u64,
    pub tx_index: usize,
    pub event_index: usize,
}

pub fn parse_coordination_events(
    checkpoint: &Checkpoint,
    checkpoint_seq: u64,
    target_package: &str,
    target_module: &str,
) -> Vec<CoordinationEvent> {
    let mut events = Vec::new();

    // let checkpoint_timestamp_ms = checkpoint
    //     .summary
    //     .as_ref()
    //     .and_then(|s| s.timestamp.as_ref())
    //     .map(|t| (t.seconds as u64 * 1000) + (t.nanos as u64 / 1_000_000))
    //     .unwrap_or(0);

    for (tx_index, transaction) in checkpoint.transactions.iter().enumerate() {
        if let Some(tx_events) = &transaction.events {
            for (event_index, event) in tx_events.events.iter().enumerate() {
                if let Some(event_type) = &event.event_type {
                    if event_type
                        == &format!("{}::{}::JobCreatedEvent", target_package, target_module)
                        || event_type
                            == &format!("{}::{}::JobFailedEvent", target_package, target_module)
                    {
                        let coordination_event = CoordinationEvent {
                            event_type: event.event_type.clone().unwrap_or_default(),
                            // package_id: target_package.to_string(),
                            // module: target_module.to_string(),
                            // sender: event.sender.clone(),
                            // timestamp_ms: checkpoint_timestamp_ms,
                            checkpoint_seq,
                            tx_index,
                            event_index,
                        };

                        debug!(
                            "Found {} event: type={}, checkpoint={}, tx={}, event={}",
                            target_module,
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
        debug!(
            "Found {} {} events in checkpoint {}",
            events.len(),
            target_module,
            checkpoint_seq
        );
    }

    events
}

pub fn parse_jobs_event_with_contents(event: &Event) -> Option<String> {
    if let Some(contents) = &event.contents {
        if let Some(value) = &contents.value {
            // Parse BCS data based on event type
            if let Some(event_type) = &event.event_type {
                debug!("Parsing BCS data for event type: {}", event_type);

                // Try to deserialize the BCS data
                if event_type.contains("JobCreatedEvent") {
                    match bcs::from_bytes::<JobCreatedEventBcs>(value) {
                        Ok(job_event) => {
                            let created_time =
                                DateTime::<Utc>::from_timestamp_millis(job_event.created_at as i64)
                                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                                    .unwrap_or_else(|| format!("{}ms", job_event.created_at));

                            let data_hex = hex::encode(&job_event.data);
                            let sequences_str = job_event
                                .sequences
                                .map(|s| format!("{:?}", s))
                                .unwrap_or_else(|| "None".to_string());
                            let sequences1_str = job_event
                                .sequences1
                                .map(|s| format!("{:?}", s))
                                .unwrap_or_else(|| "None".to_string());
                            let sequences2_str = job_event
                                .sequences2
                                .map(|s| format!("{:?}", s))
                                .unwrap_or_else(|| "None".to_string());
                            let description_str =
                                job_event.description.unwrap_or_else(|| "None".to_string());
                            let block_number_str = job_event
                                .block_number
                                .map(|b| b.to_string())
                                .unwrap_or_else(|| "None".to_string());

                            return Some(format!(
                                "JobCreatedEvent:\n  job_sequence: {}\n  description: {}\n  developer: {}\n  agent: {}\n  agent_method: {}\n  app: {}\n  app_instance: {}\n  app_instance_method: {}\n  block_number: {}\n  sequences: {}\n  sequences1: {}\n  sequences2: {}\n  data: 0x{}\n  status: {:?}\n  created_at: {}",
                                job_event.job_sequence,
                                description_str,
                                job_event.developer,
                                job_event.agent,
                                job_event.agent_method,
                                job_event.app,
                                job_event.app_instance,
                                job_event.app_instance_method,
                                block_number_str,
                                sequences_str,
                                sequences1_str,
                                sequences2_str,
                                data_hex,
                                job_event.status,
                                created_time
                            ));
                        }
                        Err(e) => {
                            error!("Failed to parse JobCreatedEvent BCS: {}", e);
                        }
                    }
                } else if event_type.contains("JobFailedEvent") {
                    match bcs::from_bytes::<JobFailedEventBcs>(value) {
                        Ok(job_event) => {
                            let failed_time =
                                DateTime::<Utc>::from_timestamp_millis(job_event.failed_at as i64)
                                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                                    .unwrap_or_else(|| format!("{}ms", job_event.failed_at));

                            return Some(format!(
                                "JobFailedEvent:\n  job_sequence: {}\n  app_instance: {}\n  error: {}\n  attempts: {}\n  failed_at: {}",
                                job_event.job_sequence,
                                job_event.app_instance,
                                job_event.error,
                                job_event.attempts,
                                failed_time
                            ));
                        }
                        Err(e) => {
                            error!("Failed to parse JobFailedEvent BCS: {}", e);
                        }
                    }
                }
            }
        }
    }
    None
}

// BCS structures matching the Move types
#[derive(Debug, Deserialize)]
struct JobCreatedEventBcs {
    job_sequence: u64,
    description: Option<String>,
    developer: String,
    agent: String,
    agent_method: String,
    app: String,
    app_instance: String,
    app_instance_method: String,
    block_number: Option<u64>,
    sequences: Option<Vec<u64>>,
    sequences1: Option<Vec<u64>>,
    sequences2: Option<Vec<u64>>,
    data: Vec<u8>,
    status: JobStatusBcs,
    created_at: u64,
}

#[derive(Debug, Deserialize)]
struct JobFailedEventBcs {
    job_sequence: u64,
    app_instance: String,
    #[allow(dead_code)]
    coordinator: [u8; 32], // Address is 32 bytes in Sui
    error: String,
    attempts: u8,
    failed_at: u64,
}

#[derive(Debug, Deserialize)]
enum JobStatusBcs {
    Pending,
    Running,
    Failed(String),
}

#[allow(dead_code)]
impl JobStatusBcs {
    fn failed_message(&self) -> Option<&str> {
        match self {
            JobStatusBcs::Failed(msg) => Some(msg),
            _ => None,
        }
    }
}
