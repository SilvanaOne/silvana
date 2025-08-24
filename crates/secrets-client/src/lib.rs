use anyhow::Result;
use proto::{
    SecretReference, StoreSecretRequest, 
    RetrieveSecretRequest,
    silvana_events_service_client::SilvanaEventsServiceClient,
};
use tonic::transport::Channel;
use tracing::{debug, info, error};

#[derive(thiserror::Error, Debug)]
pub enum SecretsClientError {
    #[error("gRPC error: {0}")]
    GrpcError(#[from] tonic::Status),
    #[error("Transport error: {0}")]
    TransportError(#[from] tonic::transport::Error),
    #[error("Invalid URI: {0}")]
    InvalidUri(String),
    #[error("Secret not found")]
    SecretNotFound,
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),
}

pub struct SecretsClient {
    client: SilvanaEventsServiceClient<Channel>,
}

impl SecretsClient {
    /// Create a new secrets client with the RPC service endpoint
    pub async fn new(endpoint: impl AsRef<str>) -> Result<Self, SecretsClientError> {
        let channel = Channel::from_shared(endpoint.as_ref().to_string())
            .map_err(|e| SecretsClientError::InvalidUri(e.to_string()))?
            .connect()
            .await?;
        
        let client = SilvanaEventsServiceClient::new(channel);
        
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
        
        let reference = SecretReference {
            developer: developer.to_string(),
            agent: agent.to_string(),
            app: app.map(|s| s.to_string()),
            app_instance: app_instance.map(|s| s.to_string()),
            name: name.map(|s| s.to_string()),
        };
        
        let request = StoreSecretRequest {
            reference: Some(reference),
            secret_value: secret_value.to_string(),
            signature: signature.to_vec(),
        };
        
        debug!("Storing secret for {}:{}", developer, agent);
        
        let response = self.client.store_secret(request).await?;
        let inner = response.into_inner();
        
        if inner.success {
            info!("Successfully stored secret for {}:{}", developer, agent);
            Ok(())
        } else {
            error!("Failed to store secret: {}", inner.message);
            Err(SecretsClientError::InvalidArgument(inner.message))
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
        
        let reference = SecretReference {
            developer: developer.to_string(),
            agent: agent.to_string(),
            app: app.map(|s| s.to_string()),
            app_instance: app_instance.map(|s| s.to_string()),
            name: name.map(|s| s.to_string()),
        };
        
        let request = RetrieveSecretRequest {
            reference: Some(reference),
            signature: signature.to_vec(),
        };
        
        debug!("Retrieving secret for {}:{}", developer, agent);
        
        let response = self.client.retrieve_secret(request).await?;
        let inner = response.into_inner();
        
        if inner.success {
            info!("Successfully retrieved secret for {}:{}", developer, agent);
            Ok(inner.secret_value)
        } else {
            if inner.message.contains("not found") {
                debug!("Secret not found for {}:{}", developer, agent);
                Err(SecretsClientError::SecretNotFound)
            } else {
                error!("Failed to retrieve secret: {}", inner.message);
                Err(SecretsClientError::InvalidArgument(inner.message))
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