//! Block and settlement management methods for the coordinator client

use crate::client::CoordinatorClient;
use crate::error::{Result, SdkError};
use crate::proto::coordinator::*;

impl CoordinatorClient {
    /// Get a block by block number
    pub async fn get_block(&mut self, block_number: u64) -> Result<Option<Block>> {
        let job_id = self
            .current_job_id
            .as_ref()
            .ok_or(SdkError::NoActiveJob)?;

        let request = GetBlockRequest {
            job_id: job_id.clone(),
            session_id: self.config.session_id.clone(),
            block_number,
        };

        let response = self.client.get_block(request).await?.into_inner();

        if !response.success {
            return Err(SdkError::OperationFailed(response.message));
        }

        Ok(response.block)
    }

    /// Try to create a new block
    pub async fn try_create_block(&mut self) -> Result<Option<u64>> {
        let job_id = self
            .current_job_id
            .as_ref()
            .ok_or(SdkError::NoActiveJob)?;

        let request = TryCreateBlockRequest {
            job_id: job_id.clone(),
            session_id: self.config.session_id.clone(),
        };

        let response = self.client.try_create_block(request).await?.into_inner();

        if !response.success {
            return Err(SdkError::OperationFailed(response.message));
        }

        Ok(response.block_number)
    }

    /// Update block state data availability
    pub async fn update_block_state_data_availability(
        &mut self,
        block_number: u64,
        state_data_availability: &str,
    ) -> Result<String> {
        let job_id = self
            .current_job_id
            .as_ref()
            .ok_or(SdkError::NoActiveJob)?;

        let request = UpdateBlockStateDataAvailabilityRequest {
            job_id: job_id.clone(),
            session_id: self.config.session_id.clone(),
            block_number,
            state_data_availability: state_data_availability.to_string(),
        };

        let response = self
            .client
            .update_block_state_data_availability(request)
            .await?
            .into_inner();

        if !response.success {
            return Err(SdkError::OperationFailed(response.message));
        }

        Ok(response.tx_hash)
    }

    /// Update block proof data availability
    pub async fn update_block_proof_data_availability(
        &mut self,
        block_number: u64,
        proof_data_availability: &str,
    ) -> Result<String> {
        let job_id = self
            .current_job_id
            .as_ref()
            .ok_or(SdkError::NoActiveJob)?;

        let request = UpdateBlockProofDataAvailabilityRequest {
            job_id: job_id.clone(),
            session_id: self.config.session_id.clone(),
            block_number,
            proof_data_availability: proof_data_availability.to_string(),
        };

        let response = self
            .client
            .update_block_proof_data_availability(request)
            .await?
            .into_inner();

        if !response.success {
            return Err(SdkError::OperationFailed(response.message));
        }

        Ok(response.tx_hash)
    }

    /// Get settlement proof for a specific block and chain
    pub async fn get_settlement_proof(
        &mut self,
        block_number: u64,
        settlement_chain: &str,
    ) -> Result<Option<String>> {
        let job_id = self
            .current_job_id
            .as_ref()
            .ok_or(SdkError::NoActiveJob)?;

        let request = GetSettlementProofRequest {
            session_id: self.config.session_id.clone(),
            block_number,
            job_id: job_id.clone(),
            settlement_chain: settlement_chain.to_string(),
        };

        let response = self.client.get_settlement_proof(request).await?.into_inner();

        if !response.success {
            return Ok(None);
        }

        Ok(response.proof)
    }

    /// Submit a settlement proof
    pub async fn submit_settlement_proof(
        &mut self,
        params: SubmitSettlementProofParams,
    ) -> Result<()> {
        let job_id = self
            .current_job_id
            .as_ref()
            .ok_or(SdkError::NoActiveJob)?;

        let request = SubmitSettlementProofRequest {
            session_id: self.config.session_id.clone(),
            block_number: params.block_number,
            job_id: job_id.clone(),
            proof: params.proof,
            cpu_time: params.cpu_time,
            chain: params.chain,
        };

        let response = self.client.submit_settlement_proof(request).await?.into_inner();

        if !response.success {
            return Err(SdkError::OperationFailed(response.message));
        }

        Ok(())
    }

    /// Get block settlement info for a specific chain
    pub async fn get_block_settlement(
        &mut self,
        block_number: u64,
        chain: &str,
    ) -> Result<Option<BlockSettlement>> {
        let job_id = self
            .current_job_id
            .as_ref()
            .ok_or(SdkError::NoActiveJob)?;

        let request = GetBlockSettlementRequest {
            job_id: job_id.clone(),
            session_id: self.config.session_id.clone(),
            block_number,
            chain: chain.to_string(),
        };

        let response = self.client.get_block_settlement(request).await?.into_inner();

        if !response.success {
            return Ok(None);
        }

        Ok(response.block_settlement)
    }

    /// Update block settlement info for a specific chain
    pub async fn update_block_settlement(
        &mut self,
        params: UpdateBlockSettlementParams,
    ) -> Result<String> {
        let job_id = self
            .current_job_id
            .as_ref()
            .ok_or(SdkError::NoActiveJob)?;

        let request = UpdateBlockSettlementRequest {
            job_id: job_id.clone(),
            session_id: self.config.session_id.clone(),
            block_number: params.block_number,
            chain: params.chain,
            settlement_tx_hash: params.settlement_tx_hash,
            settlement_tx_included_in_block: params.settlement_tx_included_in_block,
            sent_to_settlement_at: params.sent_to_settlement_at,
            settled_at: params.settled_at,
        };

        let response = self.client.update_block_settlement(request).await?.into_inner();

        if !response.success {
            return Err(SdkError::OperationFailed(response.message));
        }

        Ok(response.tx_hash)
    }

    /// Update block settlement transaction hash
    pub async fn update_block_settlement_tx_hash(
        &mut self,
        block_number: u64,
        settlement_tx_hash: &str,
        chain: &str,
    ) -> Result<String> {
        let job_id = self
            .current_job_id
            .as_ref()
            .ok_or(SdkError::NoActiveJob)?;

        let request = UpdateBlockSettlementTxHashRequest {
            job_id: job_id.clone(),
            session_id: self.config.session_id.clone(),
            block_number,
            settlement_tx_hash: settlement_tx_hash.to_string(),
            chain: chain.to_string(),
        };

        let response = self
            .client
            .update_block_settlement_tx_hash(request)
            .await?
            .into_inner();

        if !response.success {
            return Err(SdkError::OperationFailed(response.message));
        }

        Ok(response.tx_hash)
    }

    /// Update block settlement transaction included in block
    pub async fn update_block_settlement_tx_included_in_block(
        &mut self,
        block_number: u64,
        settled_at: u64,
        chain: &str,
    ) -> Result<String> {
        let job_id = self
            .current_job_id
            .as_ref()
            .ok_or(SdkError::NoActiveJob)?;

        let request = UpdateBlockSettlementTxIncludedInBlockRequest {
            job_id: job_id.clone(),
            session_id: self.config.session_id.clone(),
            block_number,
            settled_at,
            chain: chain.to_string(),
        };

        let response = self
            .client
            .update_block_settlement_tx_included_in_block(request)
            .await?
            .into_inner();

        if !response.success {
            return Err(SdkError::OperationFailed(response.message));
        }

        Ok(response.tx_hash)
    }
}

// Parameter structs

/// Parameters for submitting a settlement proof
pub struct SubmitSettlementProofParams {
    /// Block number
    pub block_number: u64,
    /// Proof string
    pub proof: String,
    /// CPU time in milliseconds
    pub cpu_time: u64,
    /// Chain identifier for settlement
    pub chain: String,
}

/// Parameters for updating block settlement
pub struct UpdateBlockSettlementParams {
    /// Block number
    pub block_number: u64,
    /// Chain identifier
    pub chain: String,
    /// Settlement transaction hash
    pub settlement_tx_hash: Option<String>,
    /// Whether settlement transaction was included in block
    pub settlement_tx_included_in_block: bool,
    /// Timestamp when sent to settlement
    pub sent_to_settlement_at: Option<u64>,
    /// Timestamp when settled
    pub settled_at: Option<u64>,
}

// Global convenience functions

/// Get a block using the global client
pub async fn get_block(block_number: u64) -> Result<Option<Block>> {
    let client = CoordinatorClient::global().await?;
    let mut client = client.write();
    client.get_block(block_number).await
}

/// Try to create a block using the global client
pub async fn try_create_block() -> Result<Option<u64>> {
    let client = CoordinatorClient::global().await?;
    let mut client = client.write();
    client.try_create_block().await
}

/// Get block settlement using the global client
pub async fn get_block_settlement(
    block_number: u64,
    chain: &str,
) -> Result<Option<BlockSettlement>> {
    let client = CoordinatorClient::global().await?;
    let mut client = client.write();
    client.get_block_settlement(block_number, chain).await
}

/// Update block settlement using the global client
pub async fn update_block_settlement(params: UpdateBlockSettlementParams) -> Result<String> {
    let client = CoordinatorClient::global().await?;
    let mut client = client.write();
    client.update_block_settlement(params).await
}