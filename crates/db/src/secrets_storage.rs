use anyhow::{anyhow, Result};
use aws_sdk_dynamodb::{Client, primitives::Blob, types::AttributeValue};
use kms::{EncryptedData, KMS};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info, warn, error};

/// Key for identifying a unique secret
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct SecretKey {
    pub developer: String,
    pub agent: String,
    pub app: Option<String>,
    pub app_instance: Option<String>,
    pub name: Option<String>,
}

/// Value stored in the database
#[derive(Debug, Serialize, Deserialize)]
pub struct SecretValue {
    pub encrypted_secret: Vec<u8>,  // The secret encrypted with KMS
    pub created_at: i64,
    pub updated_at: i64,
}

pub struct SecureSecretsStorage {
    client: Arc<Client>,
    table_name: String,
    kms: Arc<KMS>,
}

impl SecureSecretsStorage {
    pub async fn new(table_name: String, kms_key_id: String) -> Result<Self> {
        // Reuse the global DynamoDB client if available
        let client = if let Some(client) = super::DYNAMODB_CLIENT.get() {
            debug!("Reusing existing DynamoDB client for secrets storage");
            client.clone()
        } else {
            info!("Initializing new DynamoDB client for secrets storage");
            let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
            let new_client = Arc::new(Client::new(&config));
            
            // Try to set the client
            match super::DYNAMODB_CLIENT.set(new_client.clone()) {
                Ok(_) => {
                    info!("DynamoDB client initialized for secrets storage");
                    new_client
                }
                Err(_) => {
                    // Another thread initialized it first
                    super::DYNAMODB_CLIENT.get()
                        .ok_or_else(|| anyhow!("DynamoDB client not initialized despite concurrent set"))?
                        .clone()
                }
            }
        };
        
        let kms = Arc::new(KMS::new(kms_key_id).await?);
        
        Ok(Self {
            client,
            table_name,
            kms,
        })
    }
    
    /// Store a new secret
    pub async fn store_secret(
        &self,
        developer: &str,
        agent: &str,
        app: Option<&str>,
        app_instance: Option<&str>,
        name: Option<&str>,
        secret_value: &str,
    ) -> Result<()> {
        let key = SecretKey {
            developer: developer.to_string(),
            agent: agent.to_string(),
            app: app.map(|s| s.to_string()),
            app_instance: app_instance.map(|s| s.to_string()),
            name: name.map(|s| s.to_string()),
        };
        
        let primary_key = self.create_primary_key(&key)?;
        
        // Encrypt the secret
        let encrypted_secret = self.kms.encrypt(secret_value.as_bytes()).await?;
        
        // Store in database
        let now = chrono::Utc::now().timestamp();
        let value = SecretValue {
            encrypted_secret: bincode::serialize(&encrypted_secret)?,
            created_at: now,
            updated_at: now,
        };
        
        self.store_secret_value(&primary_key, &value).await?;
        info!("Stored secret for {}:{} (app: {:?}, instance: {:?}, name: {:?})", 
              developer, agent, app, app_instance, name);
        
        Ok(())
    }
    
    /// Retrieve a secret
    pub async fn retrieve_secret(
        &self,
        developer: &str,
        agent: &str,
        app: Option<&str>,
        app_instance: Option<&str>,
        name: Option<&str>,
    ) -> Result<Option<String>> {
        let key = SecretKey {
            developer: developer.to_string(),
            agent: agent.to_string(),
            app: app.map(|s| s.to_string()),
            app_instance: app_instance.map(|s| s.to_string()),
            name: name.map(|s| s.to_string()),
        };
        
        let primary_key = self.create_primary_key(&key)?;
        
        if let Some(value) = self.get_secret_value(&primary_key).await? {
            // Decrypt the secret
            let encrypted_data: EncryptedData = bincode::deserialize(&value.encrypted_secret)?;
            let decrypted = self.kms.decrypt(&encrypted_data).await?;
            let secret_string = String::from_utf8(decrypted)?;
            
            info!("Retrieved secret for {}:{} (app: {:?}, instance: {:?}, name: {:?})", 
                  developer, agent, app, app_instance, name);
            Ok(Some(secret_string))
        } else {
            info!("Secret not found for {}:{} (app: {:?}, instance: {:?}, name: {:?})", 
                  developer, agent, app, app_instance, name);
            Ok(None)
        }
    }
    
    fn create_primary_key(&self, key: &SecretKey) -> Result<Vec<u8>> {
        bincode::serialize(key).map_err(|e| anyhow!("Failed to serialize key: {}", e))
    }
    
    async fn get_secret_value(&self, primary_key: &[u8]) -> Result<Option<SecretValue>> {
        let key = HashMap::from([
            ("id".to_string(), AttributeValue::B(Blob::new(primary_key.to_vec()))),
        ]);
        
        let result = self.client
            .get_item()
            .table_name(&self.table_name)
            .set_key(Some(key))
            .send()
            .await?;
        
        if let Some(item) = result.item {
            let value_attr = item.get("value")
                .ok_or_else(|| anyhow!("Missing 'value' attribute"))?;
            
            let value_blob = value_attr.as_b()
                .map_err(|_| anyhow!("Expected binary attribute for value"))?;
            
            let value: SecretValue = bincode::deserialize(value_blob.as_ref())?;
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }
    
    async fn store_secret_value(&self, primary_key: &[u8], value: &SecretValue) -> Result<()> {
        let value_bytes = bincode::serialize(value)?;
        
        let mut item = HashMap::new();
        item.insert("id".to_string(), AttributeValue::B(Blob::new(primary_key.to_vec())));
        item.insert("value".to_string(), AttributeValue::B(Blob::new(value_bytes)));
        item.insert("created_at".to_string(), AttributeValue::N(value.created_at.to_string()));
        item.insert("updated_at".to_string(), AttributeValue::N(value.updated_at.to_string()));
        
        self.client
            .put_item()
            .table_name(&self.table_name)
            .set_item(Some(item))
            .send()
            .await?;
        
        info!("Secret stored successfully");
        Ok(())
    }
}