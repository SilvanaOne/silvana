use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct PendingJob {
    pub job_id: u64,
    pub description: Option<String>,
    pub developer: String,
    pub agent: String,
    pub agent_method: String,
    pub app: String,
    pub app_instance: String,
    pub app_instance_method: String,
    pub sequences: Option<Vec<u64>>,
    pub data: Vec<u8>,
    pub status: JobStatus,
    pub attempts: u8,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub enum JobStatus {
    Pending,
    Running,
    Failed(String),
}

