//! State and proof management methods for the coordinator client

use crate::client::CoordinatorClient;
use crate::error::{Result, SdkError};
use crate::proto::coordinator::*;

impl CoordinatorClient {
    /// Get sequence states
    pub async fn get_sequence_states(&mut self, sequence: u64) -> Result<Vec<SequenceState>> {
        let job_id = self
            .current_job_id
            .as_ref()
            .ok_or(SdkError::NoActiveJob)?;

        let request = GetSequenceStatesRequest {
            session_id: self.config.session_id.clone(),
            job_id: job_id.clone(),
            sequence,
        };

        let response = self.client.get_sequence_states(request).await?.into_inner();

        if !response.success {
            return Err(SdkError::OperationFailed(response.message));
        }

        Ok(response.states)
    }

    /// Submit state data
    pub async fn submit_state(&mut self, params: SubmitStateParams) -> Result<SubmitStateResponse> {
        let job_id = self
            .current_job_id
            .as_ref()
            .ok_or(SdkError::NoActiveJob)?;

        let request = SubmitStateRequest {
            session_id: self.config.session_id.clone(),
            job_id: job_id.clone(),
            sequence: params.sequence,
            new_state_data: params.new_state_data,
            serialized_state: params.serialized_state,
        };

        let response = self.client.submit_state(request).await?.into_inner();

        if !response.success {
            return Err(SdkError::OperationFailed(response.message));
        }

        Ok(response)
    }

    /// Read data availability by hash
    pub async fn read_data_availability(&mut self, da_hash: &str) -> Result<Option<String>> {
        let request = ReadDataAvailabilityRequest {
            session_id: self.config.session_id.clone(),
            da_hash: da_hash.to_string(),
        };

        let response = self.client.read_data_availability(request).await?.into_inner();

        if !response.success {
            return Err(SdkError::OperationFailed(response.message));
        }

        Ok(response.data)
    }

    /// Submit a proof
    pub async fn submit_proof(&mut self, params: SubmitProofParams) -> Result<SubmitProofResponse> {
        let job_id = self
            .current_job_id
            .as_ref()
            .ok_or(SdkError::NoActiveJob)?;

        let request = SubmitProofRequest {
            session_id: self.config.session_id.clone(),
            job_id: job_id.clone(),
            block_number: params.block_number,
            sequences: params.sequences,
            proof: params.proof,
            cpu_time: params.cpu_time,
            merged_sequences_1: params.merged_sequences_1.unwrap_or_default(),
            merged_sequences_2: params.merged_sequences_2.unwrap_or_default(),
        };

        let response = self.client.submit_proof(request).await?.into_inner();

        if !response.success {
            return Err(SdkError::OperationFailed(response.message));
        }

        Ok(response)
    }

    /// Get a proof
    pub async fn get_proof(&mut self, params: GetProofParams) -> Result<Option<String>> {
        let job_id = self
            .current_job_id
            .as_ref()
            .ok_or(SdkError::NoActiveJob)?;

        let request = GetProofRequest {
            session_id: self.config.session_id.clone(),
            job_id: job_id.clone(),
            block_number: params.block_number,
            sequences: params.sequences,
        };

        let response = self.client.get_proof(request).await?.into_inner();

        if !response.success {
            return Ok(None);
        }

        Ok(response.proof)
    }

    /// Get a block proof
    pub async fn get_block_proof(&mut self, block_number: u64) -> Result<Option<String>> {
        let job_id = self
            .current_job_id
            .as_ref()
            .ok_or(SdkError::NoActiveJob)?;

        let request = GetBlockProofRequest {
            session_id: self.config.session_id.clone(),
            job_id: job_id.clone(),
            block_number,
        };

        let response = self.client.get_block_proof(request).await?.into_inner();

        if !response.success {
            return Ok(None);
        }

        Ok(response.block_proof)
    }

    /// Reject a proof
    pub async fn reject_proof(&mut self, params: RejectProofParams) -> Result<()> {
        let job_id = self
            .current_job_id
            .as_ref()
            .ok_or(SdkError::NoActiveJob)?;

        let request = RejectProofRequest {
            session_id: self.config.session_id.clone(),
            job_id: job_id.clone(),
            block_number: params.block_number,
            sequences: params.sequences,
        };

        let response = self.client.reject_proof(request).await?.into_inner();

        if !response.success {
            return Err(SdkError::OperationFailed(response.message));
        }

        Ok(())
    }

    /// Send a proof event
    pub async fn proof_event(&mut self, params: ProofEventParams) -> Result<()> {
        let job_id = self
            .current_job_id
            .as_ref()
            .ok_or(SdkError::NoActiveJob)?;

        let request = ProofEventRequest {
            job_id: job_id.clone(),
            session_id: self.config.session_id.clone(),
            data_availability: params.data_availability,
            block_number: params.block_number,
            proof_event_type: params.proof_event_type as i32,
            sequences: params.sequences,
            block_proof: params.block_proof,
            merged_sequences_1: params.merged_sequences_1.unwrap_or_default(),
            merged_sequences_2: params.merged_sequences_2.unwrap_or_default(),
        };

        let response = self.client.proof_event(request).await?.into_inner();

        if !response.success {
            return Err(SdkError::OperationFailed(response.message));
        }

        Ok(())
    }
}

// Parameter structs for complex methods

/// Parameters for submitting state
pub struct SubmitStateParams {
    /// Sequence number
    pub sequence: u64,
    /// Raw state data for update_state_for_sequence
    pub new_state_data: Option<Vec<u8>>,
    /// Serialized state data to write to Walrus
    pub serialized_state: Option<String>,
}

/// Parameters for submitting a proof
pub struct SubmitProofParams {
    /// Block number
    pub block_number: u64,
    /// Sorted sequence numbers
    pub sequences: Vec<u64>,
    /// Proof string
    pub proof: String,
    /// CPU time in milliseconds
    pub cpu_time: u64,
    /// Optional merged sequences (first set)
    pub merged_sequences_1: Option<Vec<u64>>,
    /// Optional merged sequences (second set)
    pub merged_sequences_2: Option<Vec<u64>>,
}

/// Parameters for getting a proof
pub struct GetProofParams {
    /// Block number
    pub block_number: u64,
    /// Sorted sequence numbers
    pub sequences: Vec<u64>,
}

/// Parameters for rejecting a proof
pub struct RejectProofParams {
    /// Block number
    pub block_number: u64,
    /// Sorted sequence numbers
    pub sequences: Vec<u64>,
}

/// Parameters for proof events
pub struct ProofEventParams {
    /// Data availability hash
    pub data_availability: String,
    /// Block number
    pub block_number: u64,
    /// Type of proof event
    pub proof_event_type: ProofEventType,
    /// Sorted sequence numbers
    pub sequences: Vec<u64>,
    /// Whether this is a block proof
    pub block_proof: Option<bool>,
    /// Optional merged sequences (first set)
    pub merged_sequences_1: Option<Vec<u64>>,
    /// Optional merged sequences (second set)
    pub merged_sequences_2: Option<Vec<u64>>,
}

// Global convenience functions

/// Get sequence states using the global client
pub async fn get_sequence_states(sequence: u64) -> Result<Vec<SequenceState>> {
    let client = CoordinatorClient::global().await?;
    let mut client = client.write();
    client.get_sequence_states(sequence).await
}

/// Submit state using the global client
pub async fn submit_state(params: SubmitStateParams) -> Result<SubmitStateResponse> {
    let client = CoordinatorClient::global().await?;
    let mut client = client.write();
    client.submit_state(params).await
}

/// Submit proof using the global client
pub async fn submit_proof(params: SubmitProofParams) -> Result<SubmitProofResponse> {
    let client = CoordinatorClient::global().await?;
    let mut client = client.write();
    client.submit_proof(params).await
}

/// Read data availability using the global client
pub async fn read_data_availability(da_hash: &str) -> Result<Option<String>> {
    let client = CoordinatorClient::global().await?;
    let mut client = client.write();
    client.read_data_availability(da_hash).await
}