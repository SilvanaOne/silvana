use anyhow::{Result, anyhow};
use aws_config::BehaviorVersion;
use aws_sdk_s3::Client;
use aws_sdk_s3::types::{Tag, Tagging};
use once_cell::sync::OnceCell;
use sha2::{Sha256, Digest};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

use crate::constants::{S3_AVAILABILITY_MAX_RETRIES, S3_AVAILABILITY_RETRY_DELAY_MS};

static AWS_S3_CLIENT: OnceCell<Arc<Client>> = OnceCell::new();

pub struct S3Client {
    client: Arc<Client>,
    bucket_name: String,
}

impl S3Client {
    /// Initialize the global AWS S3 client (internal use only)
    async fn init_aws_client() -> Result<()> {
        let config = aws_config::defaults(BehaviorVersion::latest()).load().await;

        let client = Arc::new(Client::new(&config));

        AWS_S3_CLIENT
            .set(client)
            .map_err(|_| anyhow!("AWS S3 client already initialized"))?;

        info!("AWS S3 client initialized");
        Ok(())
    }

    /// Create a new S3Client instance for a specific bucket
    /// Automatically initializes the AWS client if not already done
    pub async fn new(bucket_name: String) -> Result<Self> {
        // Initialize AWS client if not already initialized
        if AWS_S3_CLIENT.get().is_none() {
            // Ignore error if client was already initialized (happens in parallel tests)
            let _ = Self::init_aws_client().await;
        }

        let client = AWS_S3_CLIENT
            .get()
            .cloned()
            .ok_or_else(|| anyhow!("Failed to initialize AWS S3 client"))?;

        Ok(Self {
            client,
            bucket_name,
        })
    }

    /// Write data to S3 with optional metadata and expiration
    ///
    /// # Arguments
    /// * `key` - The S3 object key
    /// * `data` - The data to store
    /// * `metadata` - Optional metadata as key-value pairs
    /// * `expires_at` - Unix timestamp in milliseconds for expiration
    /// 
    /// # Note
    /// S3 has a limit of 10 tags per object. This method ensures we don't exceed that limit
    /// by prioritizing tags and truncating if necessary.
    pub async fn write(
        &self,
        key: &str,
        data: String,
        metadata: Option<Vec<(String, String)>>,
        expires_at: u64,
    ) -> Result<()> {
        debug!(
            "Writing to S3 bucket {} with key: {}",
            self.bucket_name, key
        );

        const MAX_S3_TAGS: usize = 10;
        let mut tags = Vec::new();

        // Always add expires_at as the first tag (high priority)
        tags.push(
            Tag::builder()
                .key("expires_at")
                .value(expires_at.to_string())
                .build()?,
        );

        if let Some(metadata) = metadata {
            // We can add up to (MAX_S3_TAGS - 1) more tags since expires_at takes one slot
            let max_additional_tags = MAX_S3_TAGS - 1;
            
            if metadata.len() > max_additional_tags {
                warn!(
                    "S3 tag limit exceeded: {} tags provided, but S3 allows max {}. Truncating to {} metadata tags.",
                    metadata.len() + 1, MAX_S3_TAGS, max_additional_tags
                );
            }
            
            for (key, value) in metadata.iter().take(max_additional_tags) {
                // S3 tag limits: key max 128 chars, value max 256 chars
                // Also, S3 tags can only contain: letters, numbers, spaces, and + - = . _ : / @
                
                let tag_key = if key.len() > 128 {
                    warn!("S3 tag key '{}' exceeds 128 chars, truncating", key);
                    &key[..128]
                } else {
                    key.as_str()
                };
                
                // Sanitize tag value - replace invalid characters with underscores
                let sanitized_value = value.chars()
                    .map(|c| {
                        if c.is_alphanumeric() || c == ' ' || c == '+' || c == '-' || c == '=' 
                            || c == '.' || c == '_' || c == ':' || c == '/' || c == '@' {
                            c
                        } else {
                            '_' // Replace invalid characters with underscore
                        }
                    })
                    .collect::<String>();
                
                let tag_value = if sanitized_value.len() > 256 {
                    warn!("S3 tag value for key '{}' exceeds 256 chars ({}), truncating", key, sanitized_value.len());
                    sanitized_value[..256].to_string()
                } else {
                    sanitized_value
                };
                
                // Log if we had to sanitize the value
                if value != &tag_value {
                    debug!("Sanitized S3 tag value for key '{}': '{}' -> '{}'", 
                        key, 
                        if value.len() > 50 { format!("{}...", &value[..50]) } else { value.clone() },
                        if tag_value.len() > 50 { format!("{}...", &tag_value[..50]) } else { tag_value.clone() }
                    );
                }
                
                tags.push(
                    Tag::builder()
                        .key(tag_key.to_string())
                        .value(tag_value)
                        .build()?,
                );
            }
        }

        // Store data size and tags count before moving them
        let data_size = data.len();
        let tags_count = tags.len();
        
        let tagging = Tagging::builder().set_tag_set(Some(tags.clone())).build()?;

        let tagging_string = tagging
            .tag_set()
            .iter()
            .map(|tag| format!("{}={}", tag.key(), tag.value()))
            .collect::<Vec<_>>()
            .join("&");

        match self.client
            .put_object()
            .bucket(&self.bucket_name)
            .key(key)
            .body(data.into_bytes().into())
            .tagging(tagging_string)
            .send()
            .await {
            Ok(_) => {},
            Err(e) => {
                // Log detailed error information
                error!("S3 PUT failed - Bucket: {}, Key: {}, Error: {:?}", 
                    self.bucket_name, key, e);
                
                // Check for specific error types
                if let Some(service_error) = e.as_service_error() {
                    error!("S3 Service Error Details: {:?}", service_error);
                    
                    // Log additional context
                    error!("S3 Request - Data size: {} bytes, Tags count: {}", 
                        data_size, tags_count);
                    
                    // Log tag details if they might be the issue
                    if !tags.is_empty() {
                        error!("S3 Tags being sent:");
                        for tag in &tags {
                            error!("  Tag: {}={} (key_len={}, val_len={})", 
                                tag.key(), tag.value(), 
                                tag.key().len(), tag.value().len());
                        }
                    }
                }
                
                return Err(anyhow!("Failed to write to S3: {}", e));
            }
        }

        info!(
            "Successfully wrote to S3 bucket {} with key: {}",
            self.bucket_name, key
        );

        Ok(())
    }

    pub async fn read(&self, key: &str) -> Result<(String, HashMap<String, String>)> {
        debug!(
            "Reading from S3 bucket {} with key: {}",
            self.bucket_name, key
        );

        let get_object_output = match self
            .client
            .get_object()
            .bucket(&self.bucket_name)
            .key(key)
            .send()
            .await {
            Ok(output) => output,
            Err(e) => {
                error!("S3 GET failed - Bucket: {}, Key: {}, Error: {:?}", 
                    self.bucket_name, key, e);
                if let Some(service_error) = e.as_service_error() {
                    error!("S3 Service Error Details: {:?}", service_error);
                }
                return Err(anyhow!("Failed to read from S3: {}", e));
            }
        };

        let body = get_object_output
            .body
            .collect()
            .await
            .map_err(|e| anyhow!("Failed to collect body: {}", e))?;
        let data = String::from_utf8(body.to_vec())
            .map_err(|e| anyhow!("Failed to convert body to string: {}", e))?;

        let tagging_output = self
            .client
            .get_object_tagging()
            .bucket(&self.bucket_name)
            .key(key)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to get tags from S3: {}", e))?;

        let mut metadata = HashMap::new();

        for tag in tagging_output.tag_set() {
            let key = tag.key().to_string();
            let value = tag.value().to_string();
            metadata.insert(key, value);
        }

        info!(
            "Successfully read from S3 bucket {} with key: {}",
            self.bucket_name, key
        );

        Ok((data, metadata))
    }

    pub async fn delete(&self, key: &str) -> Result<()> {
        debug!(
            "Deleting from S3 bucket {} with key: {}",
            self.bucket_name, key
        );

        self.client
            .delete_object()
            .bucket(&self.bucket_name)
            .key(key)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to delete from S3: {}", e))?;

        info!(
            "Successfully deleted from S3 bucket {} with key: {}",
            self.bucket_name, key
        );

        Ok(())
    }

    pub async fn exists(&self, key: &str) -> Result<bool> {
        debug!(
            "Checking if exists in S3 bucket {} with key: {}",
            self.bucket_name, key
        );

        match self
            .client
            .head_object()
            .bucket(&self.bucket_name)
            .key(key)
            .send()
            .await
        {
            Ok(_) => Ok(true),
            Err(e) => {
                let service_error = e.into_service_error();
                if service_error.is_not_found() {
                    Ok(false)
                } else {
                    Err(anyhow!(
                        "Failed to check existence in S3: {}",
                        service_error
                    ))
                }
            }
        }
    }

    /// Write binary data to S3 with SHA256 verification
    ///
    /// # Arguments
    /// * `data` - The binary data to store
    /// * `file_name` - The S3 object key/filename
    /// * `mime_type` - The MIME type of the content
    /// * `expected_sha256` - Optional expected SHA256 hash for verification
    ///
    /// # Returns
    /// The SHA256 hash of the stored file or an error if verification fails
    pub async fn write_binary(
        &self,
        data: Vec<u8>,
        file_name: &str,
        mime_type: &str,
        expected_sha256: Option<String>,
    ) -> Result<String> {
        debug!(
            "Writing binary to S3 bucket {} with key: {}, size: {} bytes",
            self.bucket_name, file_name, data.len()
        );

        // Calculate SHA256 hash of the data
        let mut hasher = Sha256::new();
        hasher.update(&data);
        let calculated_hash = format!("{:x}", hasher.finalize());

        // Verify hash if provided
        if let Some(ref expected) = expected_sha256 {
            if &calculated_hash != expected {
                return Err(anyhow!(
                    "SHA256 hash mismatch: expected {}, calculated {}",
                    expected,
                    calculated_hash
                ));
            }
        }

        // Upload to S3
        match self.client
            .put_object()
            .bucket(&self.bucket_name)
            .key(file_name)
            .body(data.into())
            .content_type(mime_type)
            .metadata("sha256", calculated_hash.clone())
            .send()
            .await {
            Ok(_) => {},
            Err(e) => {
                error!("S3 PUT failed - Bucket: {}, Key: {}, Error: {:?}", 
                    self.bucket_name, file_name, e);
                return Err(anyhow!("Failed to write binary to S3: {}", e));
            }
        }

        // Wait for the object to be available (with retries)
        let max_retries = S3_AVAILABILITY_MAX_RETRIES;
        let retry_delay = Duration::from_millis(S3_AVAILABILITY_RETRY_DELAY_MS);
        
        for attempt in 1..=max_retries {
            debug!("Checking if object is available (attempt {}/{})", attempt, max_retries);
            
            match self.exists(file_name).await {
                Ok(true) => {
                    debug!("Object is available after {} attempts", attempt);
                    break;
                }
                Ok(false) if attempt < max_retries => {
                    debug!("Object not yet available, retrying...");
                    sleep(retry_delay).await;
                }
                Ok(false) => {
                    return Err(anyhow!("Object not available after {} retries", max_retries));
                }
                Err(e) if attempt < max_retries => {
                    warn!("Error checking object existence: {}, retrying...", e);
                    sleep(retry_delay).await;
                }
                Err(e) => {
                    return Err(anyhow!("Failed to verify object availability: {}", e));
                }
            }
        }

        // Read back and verify
        let (read_data, read_hash) = self.read_binary(file_name).await?;
        
        // Verify the stored data matches
        if read_hash != calculated_hash {
            return Err(anyhow!(
                "Verification failed: stored file hash {} doesn't match original {}",
                read_hash,
                calculated_hash
            ));
        }

        // Additional verification: check data integrity
        let mut verify_hasher = Sha256::new();
        verify_hasher.update(&read_data);
        let verify_hash = format!("{:x}", verify_hasher.finalize());
        
        if verify_hash != calculated_hash {
            return Err(anyhow!(
                "Data integrity check failed: read data hash {} doesn't match original {}",
                verify_hash,
                calculated_hash
            ));
        }

        info!(
            "Successfully wrote binary to S3 bucket {} with key: {}, SHA256: {}",
            self.bucket_name, file_name, calculated_hash
        );

        Ok(calculated_hash)
    }

    /// Read binary data from S3 and calculate its SHA256 hash
    ///
    /// # Arguments
    /// * `file_name` - The S3 object key/filename to read
    ///
    /// # Returns
    /// A tuple of (binary data, SHA256 hash)
    pub async fn read_binary(&self, file_name: &str) -> Result<(Vec<u8>, String)> {
        debug!(
            "Reading binary from S3 bucket {} with key: {}",
            self.bucket_name, file_name
        );

        let get_object_output = match self
            .client
            .get_object()
            .bucket(&self.bucket_name)
            .key(file_name)
            .send()
            .await {
            Ok(output) => output,
            Err(e) => {
                error!("S3 GET failed - Bucket: {}, Key: {}, Error: {:?}", 
                    self.bucket_name, file_name, e);
                if let Some(service_error) = e.as_service_error() {
                    error!("S3 Service Error Details: {:?}", service_error);
                }
                return Err(anyhow!("Failed to read binary from S3: {}", e));
            }
        };

        // Collect the binary data
        let body = get_object_output
            .body
            .collect()
            .await
            .map_err(|e| anyhow!("Failed to collect body: {}", e))?;
        let data = body.to_vec();

        // Calculate SHA256 hash
        let mut hasher = Sha256::new();
        hasher.update(&data);
        let calculated_hash = format!("{:x}", hasher.finalize());

        info!(
            "Successfully read binary from S3 bucket {} with key: {}, size: {} bytes, SHA256: {}",
            self.bucket_name, file_name, data.len(), calculated_hash
        );

        Ok((data, calculated_hash))
    }
}
