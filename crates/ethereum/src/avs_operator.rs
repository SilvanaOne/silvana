use crate::avs_contracts::{
    IAVSDirectory, IDelegationManager, IECDSAStakeRegistry, ISilvanaServiceManager,
};
use alloy::network::EthereumWallet;
use alloy::primitives::{Address, Bytes, FixedBytes, U256, keccak256};
use alloy::providers::{Provider, ProviderBuilder, WsConnect};
use alloy::rpc::types::{Filter, BlockNumberOrTag};
use alloy::signers::Signer;
use alloy::signers::local::PrivateKeySigner;
use alloy::sol_types::{SolValue, SolEvent};
use anyhow::{Result, anyhow};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::str::FromStr;


/// Configuration for AVS operator
#[derive(Debug, Clone)]
pub struct AvsConfig {
    pub rpc_url: String,
    pub ws_url: String,
    pub private_key: String,
    pub chain_id: u64,
}

/// Contract addresses for AVS
#[derive(Debug, Clone)]
pub struct AvsContracts {
    pub delegation_manager: Address,
    pub avs_directory: Address,
    pub silvana_service_manager: Address,
    pub stake_registry: Address,
}

/// Signature with salt and expiry for AVS registration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureWithSaltAndExpiry {
    pub signature: Bytes,
    pub salt: FixedBytes<32>,
    pub expiry: U256,
}

/// Task structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub name: String,
    pub task_created_block: u32,
}

/// Decoded task event
pub struct DecodedTaskEvent {
    pub task_index: u32,
    pub task: Task,
}

/// AVS operator client with actual contract interaction
pub struct AvsOperatorClient {
    pub config: AvsConfig,
    pub contracts: AvsContracts,
    pub signer: PrivateKeySigner,
}

impl AvsOperatorClient {
    /// Create a new AVS operator client
    pub async fn new(config: AvsConfig, contracts: AvsContracts) -> Result<Self> {
        let signer = PrivateKeySigner::from_str(&config.private_key)?;

        Ok(Self {
            config,
            contracts,
            signer,
        })
    }

    /// Get the operator address
    pub fn address(&self) -> Address {
        self.signer.address()
    }

    /// Get a provider for HTTP connections
    pub fn get_provider(&self) -> Result<impl Provider> {
        let wallet = EthereumWallet::from(self.signer.clone());

        let provider = ProviderBuilder::new()
            .wallet(wallet)
            .connect_http(self.config.rpc_url.parse()?);

        Ok(provider)
    }

    /// Get websocket provider for event monitoring
    pub async fn get_ws_provider(&self) -> Result<impl Provider> {
        let ws = WsConnect::new(&self.config.ws_url);
        let provider = ProviderBuilder::new().connect_ws(ws).await?;

        Ok(provider)
    }

    /// Register as operator in EigenLayer
    pub async fn register_as_operator(&self) -> Result<String> {
        let provider = self.get_provider()?;

        // Create contract instance
        let delegation_manager =
            IDelegationManager::new(self.contracts.delegation_manager, &provider);

        // Send transaction
        let pending_tx = delegation_manager
            .registerAsOperator(
                Address::ZERO,  // initDelegationApprover
                0,              // allocationDelay
                "".to_string(), // metadataURI
            )
            .send()
            .await?;

        // Get transaction hash
        let tx_hash = *pending_tx.tx_hash();

        // Wait for confirmation and get receipt
        let receipt = pending_tx.get_receipt().await?;

        // Check if transaction was successful
        if !receipt.status() {
            return Err(anyhow!(
                "Transaction failed on-chain. Tx hash: {:?}",
                tx_hash
            ));
        }

        Ok(format!("{:?}", tx_hash))
    }

    /// Check if address is registered as operator in EigenLayer
    pub async fn is_operator(&self, address: Address) -> Result<bool> {
        let provider = self.get_provider()?;
        let delegation_manager =
            IDelegationManager::new(self.contracts.delegation_manager, &provider);

        let is_op = delegation_manager.isOperator(address).call().await?;

        Ok(is_op)
    }

    /// Calculate operator AVS registration digest hash
    pub async fn calculate_operator_avs_registration_digest_hash(
        &self,
        operator: Address,
        avs: Address,
        salt: FixedBytes<32>,
        expiry: U256,
    ) -> Result<FixedBytes<32>> {
        let provider = self.get_provider()?;
        let avs_directory = IAVSDirectory::new(self.contracts.avs_directory, &provider);

        let digest = avs_directory
            .calculateOperatorAVSRegistrationDigestHash(operator, avs, salt, expiry)
            .call()
            .await?;

        Ok(digest)
    }

    /// Register operator with signature to AVS
    pub async fn register_operator_with_signature(
        &self,
        signature_with_salt_and_expiry: SignatureWithSaltAndExpiry,
    ) -> Result<String> {
        let provider = self.get_provider()?;
        let stake_registry = IECDSAStakeRegistry::new(self.contracts.stake_registry, &provider);

        let operator_sig = IECDSAStakeRegistry::SignatureWithSaltAndExpiry {
            signature: signature_with_salt_and_expiry.signature,
            salt: signature_with_salt_and_expiry.salt,
            expiry: signature_with_salt_and_expiry.expiry,
        };

        // Send transaction
        let pending_tx = stake_registry
            .registerOperatorWithSignature(operator_sig, self.signer.address())
            .gas(500000)
            .send()
            .await?;

        // Get transaction hash
        let tx_hash = *pending_tx.tx_hash();

        // Wait for confirmation and get receipt
        let receipt = pending_tx.get_receipt().await?;

        // Check if transaction was successful
        if !receipt.status() {
            return Err(anyhow!(
                "Transaction failed on-chain. Tx hash: {:?}. This may indicate the operator is already registered to this AVS or the signature is invalid.",
                tx_hash
            ));
        }

        Ok(format!("{:?}", tx_hash))
    }

    /// Create a new task
    pub async fn create_new_task(&self, task_name: String) -> Result<String> {
        let provider = self.get_provider()?;
        let service_manager =
            ISilvanaServiceManager::new(self.contracts.silvana_service_manager, &provider);

        // Send transaction
        let pending_tx = service_manager.createNewTask(task_name).send().await?;

        // Get transaction hash
        let tx_hash = *pending_tx.tx_hash();

        // Wait for confirmation and get receipt
        let receipt = pending_tx.get_receipt().await?;

        // Check if transaction was successful
        if !receipt.status() {
            return Err(anyhow!(
                "Transaction failed on-chain. Tx hash: {:?}",
                tx_hash
            ));
        }

        Ok(format!("{:?}", tx_hash))
    }

    /// Respond to a task
    pub async fn respond_to_task(
        &self,
        task: Task,
        task_index: u32,
        signature_data: Bytes,
    ) -> Result<String> {
        let provider = self.get_provider()?;
        let service_manager =
            ISilvanaServiceManager::new(self.contracts.silvana_service_manager, &provider);

        let task_struct = ISilvanaServiceManager::Task {
            name: task.name,
            taskCreatedBlock: task.task_created_block,
        };

        // Send transaction
        let pending_tx = service_manager
            .respondToTask(task_struct, task_index, signature_data)
            .gas(500000)
            .send()
            .await?;

        // Get transaction hash
        let tx_hash = *pending_tx.tx_hash();

        // Wait for confirmation and get receipt
        let receipt = pending_tx.get_receipt().await?;

        // Check if transaction was successful
        if !receipt.status() {
            return Err(anyhow!(
                "Transaction failed on-chain. Tx hash: {:?}",
                tx_hash
            ));
        }

        Ok(format!("{:?}", tx_hash))
    }

    /// Sign a message for task response
    pub async fn sign_task_message(&self, task_name: &str) -> Result<Bytes> {
        use alloy::primitives::eip191_hash_message;
        
        let message = format!("Hello, {}", task_name);
        // First hash the message with keccak256, then apply EIP-191
        let message_hash = keccak256(message.abi_encode_packed());
        let eip191_hash = eip191_hash_message(message_hash);
        self.sign_hash(&eip191_hash).await
    }

    /// Sign a message hash
    pub async fn sign_hash(&self, hash: &FixedBytes<32>) -> Result<Bytes> {
        let signature = self.signer.sign_hash(hash).await?;
        Ok(Bytes::from(signature.as_bytes()))
    }

    /// Get the current block number
    pub async fn get_current_block(&self) -> Result<u64> {
        let provider = self.get_provider()?;
        let block = provider.get_block_number().await?;
        Ok(block)
    }
    
    /// Get logs for monitoring events
    pub async fn get_logs(&self, from_block: u64, to_block: u64) -> Result<Vec<DecodedTaskEvent>> {
        let provider = self.get_provider()?;
        
        // Use the event signature from the generated contract
        let event_signature = ISilvanaServiceManager::NewTaskCreated::SIGNATURE_HASH;
        
        let filter = Filter::new()
            .address(self.contracts.silvana_service_manager)
            .event_signature(event_signature)
            .from_block(BlockNumberOrTag::Number(from_block))
            .to_block(BlockNumberOrTag::Number(to_block));
        
        let logs = provider.get_logs(&filter).await?;
        
        let mut decoded_events = Vec::new();
        for log in logs {
            // Check if this is a NewTaskCreated event by comparing topic0
            if let Some(&event_sig) = log.topic0() {
                if event_sig == event_signature {
                    // Decode the event using the generated contract types
                    match log.log_decode::<ISilvanaServiceManager::NewTaskCreated>() {
                        Ok(decoded) => {
                            let ISilvanaServiceManager::NewTaskCreated { taskIndex, task } = decoded.inner.data;
                            decoded_events.push(DecodedTaskEvent {
                                task_index: taskIndex,
                                task: Task {
                                    name: task.name,
                                    task_created_block: task.taskCreatedBlock,
                                },
                            });
                        }
                        Err(e) => {
                            eprintln!("Failed to decode NewTaskCreated event: {}", e);
                        }
                    }
                }
            }
        }
        
        Ok(decoded_events)
    }

    /// Calculate digest hash for operator registration (offline)
    pub fn calculate_digest_hash(
        &self,
        operator: Address,
        avs: Address,
        salt: FixedBytes<32>,
        expiry: U256,
    ) -> FixedBytes<32> {
        // This is a simplified version - actual implementation would need proper ABI encoding
        let data = (operator, avs, salt, expiry).abi_encode_packed();
        let hash = keccak256(data);
        FixedBytes::from(hash)
    }
}

/// Generate a signature with salt and expiry for AVS registration
pub async fn generate_operator_signature(
    signer: &PrivateKeySigner,
    digest_hash: FixedBytes<32>,
    salt: FixedBytes<32>,
    expiry_seconds: u64,
) -> Result<SignatureWithSaltAndExpiry> {
    let signature = signer.sign_hash(&digest_hash).await?;
    let expiry = U256::from(Utc::now().timestamp() as u64 + expiry_seconds);

    Ok(SignatureWithSaltAndExpiry {
        signature: Bytes::from(signature.as_bytes()),
        salt,
        expiry,
    })
}

/// Generate a random salt
pub fn generate_random_salt() -> FixedBytes<32> {
    let mut salt = [0u8; 32];
    use rand::RngCore;
    rand::thread_rng().fill_bytes(&mut salt);
    FixedBytes::from(salt)
}

/// Helper function to parse an address from string
pub fn parse_address(address: &str) -> Result<Address> {
    Address::from_str(address).map_err(|e| anyhow!("Invalid address {}: {}", address, e))
}

/// Encode task response data
pub fn encode_task_response(
    operators: Vec<Address>,
    signatures: Vec<Bytes>,
    block_number: U256,
) -> Bytes {
    use alloy::dyn_abi::DynSolValue;
    
    // Create dynamic ABI values for proper encoding
    let operators_values: Vec<DynSolValue> = operators
        .into_iter()
        .map(|addr| DynSolValue::Address(addr))
        .collect();
    
    let signatures_values: Vec<DynSolValue> = signatures
        .into_iter()
        .map(|sig| DynSolValue::Bytes(sig.to_vec()))
        .collect();
    
    // Encode as a tuple with arrays and uint32
    let encoded = DynSolValue::Tuple(vec![
        DynSolValue::Array(operators_values),
        DynSolValue::Array(signatures_values),
        DynSolValue::Uint(block_number, 32),
    ])
    .abi_encode_params();
    
    Bytes::from(encoded)
}
