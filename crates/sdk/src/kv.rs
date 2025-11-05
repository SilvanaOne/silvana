//! Key-value store and metadata management methods

use crate::client::CoordinatorClient;
use crate::error::{Result, SdkError};
use crate::proto::coordinator::*;

impl CoordinatorClient {
    /// Set a key-value pair
    pub async fn set_kv(&mut self, key: &str, value: &str) -> Result<String> {
        let job_id = self
            .current_job_id
            .as_ref()
            .ok_or(SdkError::NoActiveJob)?;

        let request = SetKvRequest {
            job_id: job_id.clone(),
            session_id: self.config.session_id.clone(),
            key: key.to_string(),
            value: value.to_string(),
        };

        let response = self.client.set_kv(request).await?.into_inner();

        if !response.success {
            return Err(SdkError::OperationFailed(response.message));
        }

        Ok(response.tx_hash)
    }

    /// Get a value by key
    pub async fn get_kv(&mut self, key: &str) -> Result<Option<String>> {
        let job_id = self
            .current_job_id
            .as_ref()
            .ok_or(SdkError::NoActiveJob)?;

        let request = GetKvRequest {
            job_id: job_id.clone(),
            session_id: self.config.session_id.clone(),
            key: key.to_string(),
        };

        let response = self.client.get_kv(request).await?.into_inner();

        if !response.success {
            return Ok(None);
        }

        Ok(response.value)
    }

    /// Delete a key-value pair
    pub async fn delete_kv(&mut self, key: &str) -> Result<String> {
        let job_id = self
            .current_job_id
            .as_ref()
            .ok_or(SdkError::NoActiveJob)?;

        let request = DeleteKvRequest {
            job_id: job_id.clone(),
            session_id: self.config.session_id.clone(),
            key: key.to_string(),
        };

        let response = self.client.delete_kv(request).await?.into_inner();

        if !response.success {
            return Err(SdkError::OperationFailed(response.message));
        }

        Ok(response.tx_hash)
    }

    /// Add metadata (write-once)
    pub async fn add_metadata(&mut self, key: &str, value: &str) -> Result<String> {
        let job_id = self
            .current_job_id
            .as_ref()
            .ok_or(SdkError::NoActiveJob)?;

        let request = AddMetadataRequest {
            job_id: job_id.clone(),
            session_id: self.config.session_id.clone(),
            key: key.to_string(),
            value: value.to_string(),
        };

        let response = self.client.add_metadata(request).await?.into_inner();

        if !response.success {
            return Err(SdkError::OperationFailed(response.message));
        }

        Ok(response.tx_hash)
    }

    /// Get metadata by key (optional key returns app instance info)
    pub async fn get_metadata(&mut self, key: Option<String>) -> Result<Option<Metadata>> {
        let job_id = self
            .current_job_id
            .as_ref()
            .ok_or(SdkError::NoActiveJob)?;

        let request = GetMetadataRequest {
            job_id: job_id.clone(),
            session_id: self.config.session_id.clone(),
            key,
        };

        let response = self.client.get_metadata(request).await?.into_inner();

        if !response.success {
            return Ok(None);
        }

        Ok(response.metadata)
    }

    /// Get app instance information (metadata without key)
    pub async fn get_app_instance_info(&mut self) -> Result<Option<Metadata>> {
        self.get_metadata(None).await
    }

    /// Get full app instance data
    pub async fn get_app_instance(&mut self) -> Result<Option<AppInstanceData>> {
        let job_id = self
            .current_job_id
            .as_ref()
            .ok_or(SdkError::NoActiveJob)?;

        let request = GetAppInstanceRequest {
            job_id: job_id.clone(),
            session_id: self.config.session_id.clone(),
        };

        let response = self.client.get_app_instance(request).await?.into_inner();

        if !response.success {
            return Ok(None);
        }

        Ok(response.app_instance)
    }

    /// Create a new app job
    pub async fn create_app_job(&mut self, params: CreateAppJobParams) -> Result<u64> {
        let job_id = self
            .current_job_id
            .as_ref()
            .ok_or(SdkError::NoActiveJob)?;

        let request = CreateAppJobRequest {
            job_id: job_id.clone(),
            session_id: self.config.session_id.clone(),
            method_name: params.method_name,
            job_description: params.job_description,
            block_number: params.block_number,
            sequences: params.sequences.unwrap_or_default(),
            sequences1: params.sequences1.unwrap_or_default(),
            sequences2: params.sequences2.unwrap_or_default(),
            data: params.data,
            interval_ms: params.interval_ms,
            next_scheduled_at: params.next_scheduled_at,
            settlement_chain: params.settlement_chain,
        };

        let response = self.client.create_app_job(request).await?.into_inner();

        if !response.success {
            return Err(SdkError::OperationFailed(response.message));
        }

        Ok(response.job_sequence)
    }
}

/// Parameters for creating an app job
pub struct CreateAppJobParams {
    /// App instance method name to call
    pub method_name: String,
    /// Job data payload
    pub data: Vec<u8>,
    /// Optional job description
    pub job_description: Option<String>,
    /// Optional block number
    pub block_number: Option<u64>,
    /// Optional sequence numbers
    pub sequences: Option<Vec<u64>>,
    /// Optional first set of sequence numbers
    pub sequences1: Option<Vec<u64>>,
    /// Optional second set of sequence numbers
    pub sequences2: Option<Vec<u64>>,
    /// Optional interval in milliseconds for recurring jobs
    pub interval_ms: Option<u64>,
    /// Optional next scheduled execution timestamp
    pub next_scheduled_at: Option<u64>,
    /// Optional chain identifier for settlement jobs
    pub settlement_chain: Option<String>,
}

impl CreateAppJobParams {
    /// Create a new CreateAppJobParams with required fields
    pub fn new(method_name: String, data: Vec<u8>) -> Self {
        Self {
            method_name,
            data,
            job_description: None,
            block_number: None,
            sequences: None,
            sequences1: None,
            sequences2: None,
            interval_ms: None,
            next_scheduled_at: None,
            settlement_chain: None,
        }
    }

    /// Set job description
    pub fn with_description(mut self, description: String) -> Self {
        self.job_description = Some(description);
        self
    }

    /// Set block number
    pub fn with_block_number(mut self, block_number: u64) -> Self {
        self.block_number = Some(block_number);
        self
    }

    /// Set sequences
    pub fn with_sequences(mut self, sequences: Vec<u64>) -> Self {
        self.sequences = Some(sequences);
        self
    }

    /// Set interval for recurring jobs
    pub fn with_interval_ms(mut self, interval_ms: u64) -> Self {
        self.interval_ms = Some(interval_ms);
        self
    }

    /// Set settlement chain
    pub fn with_settlement_chain(mut self, chain: String) -> Self {
        self.settlement_chain = Some(chain);
        self
    }
}

// Global convenience functions

/// Set a key-value pair using the global client
pub async fn set_kv(key: &str, value: &str) -> Result<String> {
    let client = CoordinatorClient::global().await?;
    let mut client = client.write();
    client.set_kv(key, value).await
}

/// Get a value by key using the global client
pub async fn get_kv(key: &str) -> Result<Option<String>> {
    let client = CoordinatorClient::global().await?;
    let mut client = client.write();
    client.get_kv(key).await
}

/// Delete a key-value pair using the global client
pub async fn delete_kv(key: &str) -> Result<String> {
    let client = CoordinatorClient::global().await?;
    let mut client = client.write();
    client.delete_kv(key).await
}

/// Add metadata using the global client
pub async fn add_metadata(key: &str, value: &str) -> Result<String> {
    let client = CoordinatorClient::global().await?;
    let mut client = client.write();
    client.add_metadata(key, value).await
}

/// Get metadata using the global client
pub async fn get_metadata(key: Option<String>) -> Result<Option<Metadata>> {
    let client = CoordinatorClient::global().await?;
    let mut client = client.write();
    client.get_metadata(key).await
}

/// Get app instance info using the global client
pub async fn get_app_instance_info() -> Result<Option<Metadata>> {
    let client = CoordinatorClient::global().await?;
    let mut client = client.write();
    client.get_app_instance_info().await
}

/// Get full app instance data using the global client
pub async fn get_app_instance() -> Result<Option<AppInstanceData>> {
    let client = CoordinatorClient::global().await?;
    let mut client = client.write();
    client.get_app_instance().await
}

/// Create an app job using the global client
pub async fn create_app_job(params: CreateAppJobParams) -> Result<u64> {
    let client = CoordinatorClient::global().await?;
    let mut client = client.write();
    client.create_app_job(params).await
}