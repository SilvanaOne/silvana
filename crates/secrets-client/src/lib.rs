use anyhow::Result;
use rpc_client::{RpcClientConfig, SilvanaRpcClient, RpcClientError};
use tracing::{debug, info, error};

#[derive(thiserror::Error, Debug)]
pub enum SecretsClientError {
    #[error("RPC client error: {0}")]
    RpcError(#[from] RpcClientError),
    #[error("Secret not found")]
    SecretNotFound,
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),
}

#[derive(Clone)]
pub struct SecretsClient {
    client: SilvanaRpcClient,
}

impl SecretsClient {
    /// Create a new secrets client with the RPC service endpoint
    pub async fn new(endpoint: impl AsRef<str>) -> Result<Self, SecretsClientError> {
        let config = RpcClientConfig::new(endpoint.as_ref());
        let client = SilvanaRpcClient::new(config).await?;
        Ok(Self { client })
    }
    
    /// Store a secret for a given reference
    pub async fn store_secret(
        &mut self,
        developer: &str,
        agent: &str,
        app: Option<&str>,
        app_instance: Option<&str>,
        name: Option<&str>,
        secret_value: &str,
        signature: &[u8],
    ) -> Result<(), SecretsClientError> {
        if developer.is_empty() || agent.is_empty() {
            return Err(SecretsClientError::InvalidArgument(
                "Developer and agent are required".to_string(),
            ));
        }
        
        if secret_value.is_empty() {
            return Err(SecretsClientError::InvalidArgument(
                "Secret value cannot be empty".to_string(),
            ));
        }
        
        debug!("Storing secret for {}:{}", developer, agent);
        
        let response = self.client
            .store_secret(developer, agent, app, app_instance, name, secret_value, signature)
            .await?;
        
        if response.success {
            info!("Successfully stored secret for {}:{}", developer, agent);
            Ok(())
        } else {
            error!("Failed to store secret: {}", response.message);
            Err(SecretsClientError::InvalidArgument(response.message))
        }
    }
    
    /// Retrieve a secret for a given reference
    pub async fn retrieve_secret(
        &mut self,
        developer: &str,
        agent: &str,
        app: Option<&str>,
        app_instance: Option<&str>,
        name: Option<&str>,
        signature: &[u8],
    ) -> Result<String, SecretsClientError> {
        if developer.is_empty() || agent.is_empty() {
            return Err(SecretsClientError::InvalidArgument(
                "Developer and agent are required".to_string(),
            ));
        }
        
        debug!("Retrieving secret for {}:{}", developer, agent);
        
        let response = self.client
            .retrieve_secret(developer, agent, app, app_instance, name, signature)
            .await?;
        
        if response.success {
            info!("Successfully retrieved secret for {}:{}", developer, agent);
            Ok(response.secret_value)
        } else {
            if response.message.contains("not found") {
                debug!("Secret not found for {}:{}", developer, agent);
                Err(SecretsClientError::SecretNotFound)
            } else {
                error!("Failed to retrieve secret: {}", response.message);
                Err(SecretsClientError::InvalidArgument(response.message))
            }
        }
    }
    
    /// Check if a secret exists (retrieve but don't return the value)
    pub async fn secret_exists(
        &mut self,
        developer: &str,
        agent: &str,
        app: Option<&str>,
        app_instance: Option<&str>,
        name: Option<&str>,
        signature: &[u8],
    ) -> Result<bool, SecretsClientError> {
        match self.retrieve_secret(developer, agent, app, app_instance, name, signature).await {
            Ok(_) => Ok(true),
            Err(SecretsClientError::SecretNotFound) => Ok(false),
            Err(e) => Err(e),
        }
    }
}