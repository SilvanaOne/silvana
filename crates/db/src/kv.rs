use anyhow::{Result, anyhow};
use aws_sdk_dynamodb::Client;
use aws_sdk_dynamodb::types::{AttributeValue, DeleteRequest, PutRequest, WriteRequest};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, error, info};

/// Configuration storage service using DynamoDB
pub struct ConfigStorage {
    client: Arc<Client>,
    table_name: String,
}

impl ConfigStorage {
    /// Create a new ConfigStorage instance
    pub async fn new(table_name: String) -> Result<Self> {
        // Reuse the global DynamoDB client if available
        let client = if let Some(client) = super::DYNAMODB_CLIENT.get() {
            debug!("Reusing existing DynamoDB client for config storage");
            client.clone()
        } else {
            info!("Initializing new DynamoDB client for config storage");
            let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
            let new_client = Arc::new(Client::new(&config));

            // Try to set the client
            match super::DYNAMODB_CLIENT.set(new_client.clone()) {
                Ok(_) => {
                    info!("DynamoDB client initialized for config storage");
                    new_client
                }
                Err(_) => {
                    // Another thread initialized it first
                    super::DYNAMODB_CLIENT
                        .get()
                        .ok_or_else(|| {
                            anyhow!("DynamoDB client not initialized despite concurrent set")
                        })?
                        .clone()
                }
            }
        };

        Ok(Self { client, table_name })
    }

    /// Get all configuration values for a given chain
    pub async fn get_config(&self, chain: &str) -> Result<HashMap<String, String>> {
        debug!("Getting config for chain: {}", chain);

        // Query all items with the given chain as partition key
        let result = self
            .client
            .query()
            .table_name(&self.table_name)
            .key_condition_expression("chain = :chain")
            .expression_attribute_values(":chain", AttributeValue::S(chain.to_string()))
            .send()
            .await
            .map_err(|e| anyhow!("Failed to query config: {}", e))?;

        let mut config = HashMap::new();

        if let Some(items) = result.items {
            for item in items {
                // Extract key and value from the item
                if let (Some(key_attr), Some(value_attr)) =
                    (item.get("config_key"), item.get("config_value"))
                {
                    if let (AttributeValue::S(key), AttributeValue::S(value)) =
                        (key_attr, value_attr)
                    {
                        config.insert(key.clone(), value.clone());
                    }
                }
            }
        }

        info!(
            "Retrieved {} config items for chain: {}",
            config.len(),
            chain
        );
        Ok(config)
    }

    /// Write configuration values for a given chain
    /// This will replace all existing configuration for the chain
    pub async fn write_config(&self, chain: &str, config: HashMap<String, String>) -> Result<u32> {
        debug!(
            "Writing config for chain: {} ({} items)",
            chain,
            config.len()
        );

        // First, delete all existing items for this chain
        self.delete_all_config(chain).await?;

        // If config is empty, we're done
        if config.is_empty() {
            return Ok(0);
        }

        // Prepare batch write requests
        let mut write_requests = Vec::new();

        for (key, value) in config.iter() {
            let mut item = HashMap::new();
            item.insert("chain".to_string(), AttributeValue::S(chain.to_string()));
            item.insert("config_key".to_string(), AttributeValue::S(key.clone()));
            item.insert("config_value".to_string(), AttributeValue::S(value.clone()));

            write_requests.push(
                WriteRequest::builder()
                    .put_request(PutRequest::builder().set_item(Some(item)).build().unwrap())
                    .build(),
            );
        }

        // DynamoDB BatchWriteItem has a limit of 25 items per request
        let chunks: Vec<_> = write_requests.chunks(25).collect();
        let mut total_written = 0;

        for chunk in chunks {
            let mut request_items = HashMap::new();
            request_items.insert(self.table_name.clone(), chunk.to_vec());

            let result = self
                .client
                .batch_write_item()
                .set_request_items(Some(request_items))
                .send()
                .await
                .map_err(|e| anyhow!("Failed to write config batch: {}", e))?;

            // Check for unprocessed items
            if let Some(unprocessed) = result.unprocessed_items {
                if !unprocessed.is_empty() {
                    error!("Some items were not processed: {:?}", unprocessed);
                    return Err(anyhow!("Failed to write all config items"));
                }
            }

            total_written += chunk.len() as u32;
        }

        info!(
            "Successfully wrote {} config items for chain: {}",
            total_written, chain
        );
        Ok(total_written)
    }

    /// Delete all configuration items for a given chain
    async fn delete_all_config(&self, chain: &str) -> Result<()> {
        debug!("Deleting all config for chain: {}", chain);

        // First, query all items to get their keys
        let result = self
            .client
            .query()
            .table_name(&self.table_name)
            .key_condition_expression("chain = :chain")
            .expression_attribute_values(":chain", AttributeValue::S(chain.to_string()))
            .projection_expression("chain, config_key") // Only get the keys
            .send()
            .await
            .map_err(|e| anyhow!("Failed to query config for deletion: {}", e))?;

        let items = match result.items {
            Some(items) if !items.is_empty() => items,
            _ => {
                debug!("No existing config to delete for chain: {}", chain);
                return Ok(());
            }
        };

        // Prepare batch delete requests
        let mut delete_requests = Vec::new();

        for item in items {
            if let (Some(chain_attr), Some(key_attr)) = (item.get("chain"), item.get("config_key"))
            {
                let mut key = HashMap::new();
                key.insert("chain".to_string(), chain_attr.clone());
                key.insert("config_key".to_string(), key_attr.clone());

                delete_requests.push(
                    WriteRequest::builder()
                        .delete_request(
                            DeleteRequest::builder().set_key(Some(key)).build().unwrap(),
                        )
                        .build(),
                );
            }
        }

        // Batch delete in chunks of 25
        let chunks: Vec<_> = delete_requests.chunks(25).collect();

        for chunk in chunks {
            let mut request_items = HashMap::new();
            request_items.insert(self.table_name.clone(), chunk.to_vec());

            self.client
                .batch_write_item()
                .set_request_items(Some(request_items))
                .send()
                .await
                .map_err(|e| anyhow!("Failed to delete config batch: {}", e))?;
        }

        info!(
            "Deleted {} config items for chain: {}",
            delete_requests.len(),
            chain
        );
        Ok(())
    }
}
