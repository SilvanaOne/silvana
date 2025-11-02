//! Wrapper trait for unifying different coordination layer implementations

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use silvana_coordination_trait::{
    AppInstance, Block, Job, SequenceState,
};

/// Unified coordination wrapper that abstracts over different coordination layer types
/// All coordination layers are accessed through this common interface
#[async_trait]
pub trait CoordinationWrapper: Send + Sync {
    /// Get the layer ID
    fn layer_id(&self) -> &str;

    /// Get the chain ID
    fn chain_id(&self) -> String;

    /// Check if this layer supports multicall
    fn supports_multicall(&self) -> bool;

    // Job Management
    async fn fetch_pending_jobs(&self, app_instance: &str) -> Result<Vec<Job<String>>>;
    async fn fetch_failed_jobs(&self, app_instance: &str) -> Result<Vec<Job<String>>>;
    async fn get_failed_jobs_count(&self, app_instance: &str) -> Result<u64>;
    async fn fetch_job_by_id(&self, app_instance: &str, job_id: &str) -> Result<Option<Job<String>>>;
    async fn start_job(&self, app_instance: &str, job_id: &str) -> Result<bool>;
    async fn complete_job(&self, app_instance: &str, job_id: &str) -> Result<String>;
    async fn fail_job(&self, app_instance: &str, job_id: &str, error: &str) -> Result<String>;

    // Sequence State Management
    async fn fetch_sequence_state(&self, app_instance: &str, sequence: u64) -> Result<Option<SequenceState>>;
    async fn get_current_sequence(&self, app_instance: &str) -> Result<u64>;

    // Block Management
    async fn fetch_block(&self, app_instance: &str, block_number: u64) -> Result<Option<Block>>;
    async fn try_create_block(&self, app_instance: &str) -> Result<Option<String>>;

    // App Instance Operations
    async fn fetch_app_instance(&self, app_instance: &str) -> Result<AppInstance>;
    async fn is_app_paused(&self, app_instance: &str) -> Result<bool>;

    // KV Storage
    async fn get_kv_string(&self, app_instance: &str, key: &str) -> Result<Option<String>>;
    async fn set_kv_string(&self, app_instance: &str, key: String, value: String) -> Result<String>;
}

/// Wrapper for SuiCoordination
pub struct SuiCoordinationWrapper {
    inner: sui::coordination::SuiCoordination,
    layer_id: String,
}

impl SuiCoordinationWrapper {
    pub fn new(layer_id: String) -> Self {
        let inner = sui::coordination::SuiCoordination::with_chain_id(layer_id.clone());
        Self { inner, layer_id }
    }
}

#[async_trait]
impl CoordinationWrapper for SuiCoordinationWrapper {
    fn layer_id(&self) -> &str {
        &self.layer_id
    }

    fn chain_id(&self) -> String {
        use silvana_coordination_trait::Coordination;
        self.inner.chain_id()
    }

    fn supports_multicall(&self) -> bool {
        use silvana_coordination_trait::Coordination;
        self.inner.supports_multicall()
    }

    async fn fetch_pending_jobs(&self, app_instance: &str) -> Result<Vec<Job<String>>> {
        use silvana_coordination_trait::Coordination;
        let jobs = self.inner.fetch_pending_jobs(app_instance).await
            .map_err(|e| anyhow!("Sui error: {}", e))?;

        // Convert Job<u64> to Job<String>
        Ok(jobs.into_iter().map(|j| Job {
            id: j.id.to_string(),
            job_sequence: j.job_sequence,
            description: j.description,
            developer: j.developer,
            agent: j.agent,
            agent_method: j.agent_method,
            app: j.app,
            app_instance: j.app_instance,
            app_instance_method: j.app_instance_method,
            block_number: j.block_number,
            sequences: j.sequences,
            sequences1: j.sequences1,
            sequences2: j.sequences2,
            data: j.data,
            status: j.status,
            attempts: j.attempts,
            interval_ms: j.interval_ms,
            next_scheduled_at: j.next_scheduled_at,
            created_at: j.created_at,
            updated_at: j.updated_at,
        }).collect())
    }

    async fn fetch_failed_jobs(&self, app_instance: &str) -> Result<Vec<Job<String>>> {
        use silvana_coordination_trait::Coordination;
        let jobs = self.inner.fetch_failed_jobs(app_instance).await
            .map_err(|e| anyhow!("Sui error: {}", e))?;

        Ok(jobs.into_iter().map(|j| Job {
            id: j.id.to_string(),
            job_sequence: j.job_sequence,
            description: j.description,
            developer: j.developer,
            agent: j.agent,
            agent_method: j.agent_method,
            app: j.app,
            app_instance: j.app_instance,
            app_instance_method: j.app_instance_method,
            block_number: j.block_number,
            sequences: j.sequences,
            sequences1: j.sequences1,
            sequences2: j.sequences2,
            data: j.data,
            status: j.status,
            attempts: j.attempts,
            interval_ms: j.interval_ms,
            next_scheduled_at: j.next_scheduled_at,
            created_at: j.created_at,
            updated_at: j.updated_at,
        }).collect())
    }

    async fn get_failed_jobs_count(&self, app_instance: &str) -> Result<u64> {
        use silvana_coordination_trait::Coordination;
        self.inner.get_failed_jobs_count(app_instance).await
            .map_err(|e| anyhow!("Sui error: {}", e))
    }

    async fn fetch_job_by_id(&self, app_instance: &str, job_id: &str) -> Result<Option<Job<String>>> {
        use silvana_coordination_trait::Coordination;
        let job_id_u64 = job_id.parse::<u64>()
            .map_err(|e| anyhow!("Invalid job ID for Sui: {}", e))?;

        let job = self.inner.fetch_job_by_id(app_instance, &job_id_u64).await
            .map_err(|e| anyhow!("Sui error: {}", e))?;

        Ok(job.map(|j| Job {
            id: j.id.to_string(),
            job_sequence: j.job_sequence,
            description: j.description,
            developer: j.developer,
            agent: j.agent,
            agent_method: j.agent_method,
            app: j.app,
            app_instance: j.app_instance,
            app_instance_method: j.app_instance_method,
            block_number: j.block_number,
            sequences: j.sequences,
            sequences1: j.sequences1,
            sequences2: j.sequences2,
            data: j.data,
            status: j.status,
            attempts: j.attempts,
            interval_ms: j.interval_ms,
            next_scheduled_at: j.next_scheduled_at,
            created_at: j.created_at,
            updated_at: j.updated_at,
        }))
    }

    async fn start_job(&self, app_instance: &str, job_id: &str) -> Result<bool> {
        use silvana_coordination_trait::Coordination;
        let job_id_u64 = job_id.parse::<u64>()
            .map_err(|e| anyhow!("Invalid job ID for Sui: {}", e))?;

        self.inner.start_job(app_instance, &job_id_u64).await
            .map_err(|e| anyhow!("Sui error: {}", e))
    }

    async fn complete_job(&self, app_instance: &str, job_id: &str) -> Result<String> {
        use silvana_coordination_trait::Coordination;
        let job_id_u64 = job_id.parse::<u64>()
            .map_err(|e| anyhow!("Invalid job ID for Sui: {}", e))?;

        self.inner.complete_job(app_instance, &job_id_u64).await
            .map_err(|e| anyhow!("Sui error: {}", e))
    }

    async fn fail_job(&self, app_instance: &str, job_id: &str, error: &str) -> Result<String> {
        use silvana_coordination_trait::Coordination;
        let job_id_u64 = job_id.parse::<u64>()
            .map_err(|e| anyhow!("Invalid job ID for Sui: {}", e))?;

        self.inner.fail_job(app_instance, &job_id_u64, error).await
            .map_err(|e| anyhow!("Sui error: {}", e))
    }

    async fn fetch_sequence_state(&self, app_instance: &str, sequence: u64) -> Result<Option<SequenceState>> {
        use silvana_coordination_trait::Coordination;
        self.inner.fetch_sequence_state(app_instance, sequence).await
            .map_err(|e| anyhow!("Sui error: {}", e))
    }

    async fn get_current_sequence(&self, app_instance: &str) -> Result<u64> {
        use silvana_coordination_trait::Coordination;
        self.inner.get_current_sequence(app_instance).await
            .map_err(|e| anyhow!("Sui error: {}", e))
    }

    async fn fetch_block(&self, app_instance: &str, block_number: u64) -> Result<Option<Block>> {
        use silvana_coordination_trait::Coordination;
        self.inner.fetch_block(app_instance, block_number).await
            .map_err(|e| anyhow!("Sui error: {}", e))
    }

    async fn try_create_block(&self, app_instance: &str) -> Result<Option<String>> {
        use silvana_coordination_trait::Coordination;
        let block_id = self.inner.try_create_block(app_instance).await
            .map_err(|e| anyhow!("Sui error: {}", e))?;

        Ok(block_id.map(|id| id.to_string()))
    }

    async fn fetch_app_instance(&self, app_instance: &str) -> Result<AppInstance> {
        use silvana_coordination_trait::Coordination;
        self.inner.fetch_app_instance(app_instance).await
            .map_err(|e| anyhow!("Sui error: {}", e))
    }

    async fn is_app_paused(&self, app_instance: &str) -> Result<bool> {
        use silvana_coordination_trait::Coordination;
        self.inner.is_app_paused(app_instance).await
            .map_err(|e| anyhow!("Sui error: {}", e))
    }

    async fn get_kv_string(&self, app_instance: &str, key: &str) -> Result<Option<String>> {
        use silvana_coordination_trait::Coordination;
        self.inner.get_kv_string(app_instance, key).await
            .map_err(|e| anyhow!("Sui error: {}", e))
    }

    async fn set_kv_string(&self, app_instance: &str, key: String, value: String) -> Result<String> {
        use silvana_coordination_trait::Coordination;
        self.inner.set_kv_string(app_instance, key, value).await
            .map_err(|e| anyhow!("Sui error: {}", e))
    }
}

/// Wrapper for PrivateCoordination
pub struct PrivateCoordinationWrapper {
    inner: silvana_coordination_private::PrivateCoordination,
    layer_id: String,
}

impl PrivateCoordinationWrapper {
    pub async fn new(layer_id: String, config: silvana_coordination_private::PrivateCoordinationConfig) -> Result<Self> {
        let inner = silvana_coordination_private::PrivateCoordination::new(config).await
            .map_err(|e| anyhow!("Failed to create PrivateCoordination: {}", e))?;
        Ok(Self { inner, layer_id })
    }
}

#[async_trait]
impl CoordinationWrapper for PrivateCoordinationWrapper {
    fn layer_id(&self) -> &str {
        &self.layer_id
    }

    fn chain_id(&self) -> String {
        use silvana_coordination_trait::Coordination;
        self.inner.chain_id()
    }

    fn supports_multicall(&self) -> bool {
        use silvana_coordination_trait::Coordination;
        self.inner.supports_multicall()
    }

    async fn fetch_pending_jobs(&self, app_instance: &str) -> Result<Vec<Job<String>>> {
        use silvana_coordination_trait::Coordination;
        self.inner.fetch_pending_jobs(app_instance).await
            .map_err(|e| anyhow!("Private error: {}", e))
    }

    async fn fetch_failed_jobs(&self, app_instance: &str) -> Result<Vec<Job<String>>> {
        use silvana_coordination_trait::Coordination;
        self.inner.fetch_failed_jobs(app_instance).await
            .map_err(|e| anyhow!("Private error: {}", e))
    }

    async fn get_failed_jobs_count(&self, app_instance: &str) -> Result<u64> {
        use silvana_coordination_trait::Coordination;
        self.inner.get_failed_jobs_count(app_instance).await
            .map_err(|e| anyhow!("Private error: {}", e))
    }

    async fn fetch_job_by_id(&self, app_instance: &str, job_id: &str) -> Result<Option<Job<String>>> {
        use silvana_coordination_trait::Coordination;
        self.inner.fetch_job_by_id(app_instance, &job_id.to_string()).await
            .map_err(|e| anyhow!("Private error: {}", e))
    }

    async fn start_job(&self, app_instance: &str, job_id: &str) -> Result<bool> {
        use silvana_coordination_trait::Coordination;
        self.inner.start_job(app_instance, &job_id.to_string()).await
            .map_err(|e| anyhow!("Private error: {}", e))
    }

    async fn complete_job(&self, app_instance: &str, job_id: &str) -> Result<String> {
        use silvana_coordination_trait::Coordination;
        self.inner.complete_job(app_instance, &job_id.to_string()).await
            .map_err(|e| anyhow!("Private error: {}", e))
    }

    async fn fail_job(&self, app_instance: &str, job_id: &str, error: &str) -> Result<String> {
        use silvana_coordination_trait::Coordination;
        self.inner.fail_job(app_instance, &job_id.to_string(), error).await
            .map_err(|e| anyhow!("Private error: {}", e))
    }

    async fn fetch_sequence_state(&self, app_instance: &str, sequence: u64) -> Result<Option<SequenceState>> {
        use silvana_coordination_trait::Coordination;
        self.inner.fetch_sequence_state(app_instance, sequence).await
            .map_err(|e| anyhow!("Private error: {}", e))
    }

    async fn get_current_sequence(&self, app_instance: &str) -> Result<u64> {
        use silvana_coordination_trait::Coordination;
        self.inner.get_current_sequence(app_instance).await
            .map_err(|e| anyhow!("Private error: {}", e))
    }

    async fn fetch_block(&self, app_instance: &str, block_number: u64) -> Result<Option<Block>> {
        use silvana_coordination_trait::Coordination;
        self.inner.fetch_block(app_instance, block_number).await
            .map_err(|e| anyhow!("Private error: {}", e))
    }

    async fn try_create_block(&self, app_instance: &str) -> Result<Option<String>> {
        use silvana_coordination_trait::Coordination;
        let block_id = self.inner.try_create_block(app_instance).await
            .map_err(|e| anyhow!("Private error: {}", e))?;

        Ok(block_id.map(|id| id.to_string()))
    }

    async fn fetch_app_instance(&self, app_instance: &str) -> Result<AppInstance> {
        use silvana_coordination_trait::Coordination;
        self.inner.fetch_app_instance(app_instance).await
            .map_err(|e| anyhow!("Private error: {}", e))
    }

    async fn is_app_paused(&self, app_instance: &str) -> Result<bool> {
        use silvana_coordination_trait::Coordination;
        self.inner.is_app_paused(app_instance).await
            .map_err(|e| anyhow!("Private error: {}", e))
    }

    async fn get_kv_string(&self, app_instance: &str, key: &str) -> Result<Option<String>> {
        use silvana_coordination_trait::Coordination;
        self.inner.get_kv_string(app_instance, key).await
            .map_err(|e| anyhow!("Private error: {}", e))
    }

    async fn set_kv_string(&self, app_instance: &str, key: String, value: String) -> Result<String> {
        use silvana_coordination_trait::Coordination;
        self.inner.set_kv_string(app_instance, key, value).await
            .map_err(|e| anyhow!("Private error: {}", e))
    }
}

/// Wrapper for EthereumCoordination
pub struct EthereumCoordinationWrapper {
    inner: silvana_coordination_ethereum::EthereumCoordination,
    layer_id: String,
}

impl EthereumCoordinationWrapper {
    pub fn new(layer_id: String, config: silvana_coordination_ethereum::EthereumCoordinationConfig) -> Result<Self> {
        let inner = silvana_coordination_ethereum::EthereumCoordination::new(config)
            .map_err(|e| anyhow!("Failed to create EthereumCoordination: {}", e))?;
        Ok(Self { inner, layer_id })
    }
}

#[async_trait]
impl CoordinationWrapper for EthereumCoordinationWrapper {
    fn layer_id(&self) -> &str {
        &self.layer_id
    }

    fn chain_id(&self) -> String {
        use silvana_coordination_trait::Coordination;
        self.inner.chain_id()
    }

    fn supports_multicall(&self) -> bool {
        use silvana_coordination_trait::Coordination;
        self.inner.supports_multicall()
    }

    async fn fetch_pending_jobs(&self, app_instance: &str) -> Result<Vec<Job<String>>> {
        use silvana_coordination_trait::Coordination;
        self.inner.fetch_pending_jobs(app_instance).await
            .map_err(|e| anyhow!("Ethereum error: {}", e))
    }

    async fn fetch_failed_jobs(&self, app_instance: &str) -> Result<Vec<Job<String>>> {
        use silvana_coordination_trait::Coordination;
        self.inner.fetch_failed_jobs(app_instance).await
            .map_err(|e| anyhow!("Ethereum error: {}", e))
    }

    async fn get_failed_jobs_count(&self, app_instance: &str) -> Result<u64> {
        use silvana_coordination_trait::Coordination;
        self.inner.get_failed_jobs_count(app_instance).await
            .map_err(|e| anyhow!("Ethereum error: {}", e))
    }

    async fn fetch_job_by_id(&self, app_instance: &str, job_id: &str) -> Result<Option<Job<String>>> {
        use silvana_coordination_trait::Coordination;
        self.inner.fetch_job_by_id(app_instance, &job_id.to_string()).await
            .map_err(|e| anyhow!("Ethereum error: {}", e))
    }

    async fn start_job(&self, app_instance: &str, job_id: &str) -> Result<bool> {
        use silvana_coordination_trait::Coordination;
        self.inner.start_job(app_instance, &job_id.to_string()).await
            .map_err(|e| anyhow!("Ethereum error: {}", e))
    }

    async fn complete_job(&self, app_instance: &str, job_id: &str) -> Result<String> {
        use silvana_coordination_trait::Coordination;
        self.inner.complete_job(app_instance, &job_id.to_string()).await
            .map_err(|e| anyhow!("Ethereum error: {}", e))
    }

    async fn fail_job(&self, app_instance: &str, job_id: &str, error: &str) -> Result<String> {
        use silvana_coordination_trait::Coordination;
        self.inner.fail_job(app_instance, &job_id.to_string(), error).await
            .map_err(|e| anyhow!("Ethereum error: {}", e))
    }

    async fn fetch_sequence_state(&self, app_instance: &str, sequence: u64) -> Result<Option<SequenceState>> {
        use silvana_coordination_trait::Coordination;
        self.inner.fetch_sequence_state(app_instance, sequence).await
            .map_err(|e| anyhow!("Ethereum error: {}", e))
    }

    async fn get_current_sequence(&self, app_instance: &str) -> Result<u64> {
        use silvana_coordination_trait::Coordination;
        self.inner.get_current_sequence(app_instance).await
            .map_err(|e| anyhow!("Ethereum error: {}", e))
    }

    async fn fetch_block(&self, app_instance: &str, block_number: u64) -> Result<Option<Block>> {
        use silvana_coordination_trait::Coordination;
        self.inner.fetch_block(app_instance, block_number).await
            .map_err(|e| anyhow!("Ethereum error: {}", e))
    }

    async fn try_create_block(&self, app_instance: &str) -> Result<Option<String>> {
        use silvana_coordination_trait::Coordination;
        let block_id = self.inner.try_create_block(app_instance).await
            .map_err(|e| anyhow!("Ethereum error: {}", e))?;

        Ok(block_id.map(|id| id.to_string()))
    }

    async fn fetch_app_instance(&self, app_instance: &str) -> Result<AppInstance> {
        use silvana_coordination_trait::Coordination;
        self.inner.fetch_app_instance(app_instance).await
            .map_err(|e| anyhow!("Ethereum error: {}", e))
    }

    async fn is_app_paused(&self, app_instance: &str) -> Result<bool> {
        use silvana_coordination_trait::Coordination;
        self.inner.is_app_paused(app_instance).await
            .map_err(|e| anyhow!("Ethereum error: {}", e))
    }

    async fn get_kv_string(&self, app_instance: &str, key: &str) -> Result<Option<String>> {
        use silvana_coordination_trait::Coordination;
        self.inner.get_kv_string(app_instance, key).await
            .map_err(|e| anyhow!("Ethereum error: {}", e))
    }

    async fn set_kv_string(&self, app_instance: &str, key: String, value: String) -> Result<String> {
        use silvana_coordination_trait::Coordination;
        self.inner.set_kv_string(app_instance, key, value).await
            .map_err(|e| anyhow!("Ethereum error: {}", e))
    }
}