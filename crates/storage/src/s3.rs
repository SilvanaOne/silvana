use anyhow::{Result, anyhow};
use aws_config::BehaviorVersion;
use aws_sdk_s3::Client;
use aws_sdk_s3::types::{Tag, Tagging};
use once_cell::sync::OnceCell;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info};

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
            Self::init_aws_client().await?;
        }

        let client = AWS_S3_CLIENT
            .get()
            .cloned()
            .expect("AWS client should be initialized");

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

        let mut tags = Vec::new();

        tags.push(
            Tag::builder()
                .key("expires_at")
                .value(expires_at.to_string())
                .build()?,
        );

        if let Some(metadata) = metadata {
            for (key, value) in metadata.iter() {
                tags.push(
                    Tag::builder()
                        .key(key.clone())
                        .value(value.clone())
                        .build()?,
                );
            }
        }

        let tagging = Tagging::builder().set_tag_set(Some(tags)).build()?;

        let tagging_string = tagging
            .tag_set()
            .iter()
            .map(|tag| format!("{}={}", tag.key(), tag.value()))
            .collect::<Vec<_>>()
            .join("&");

        self.client
            .put_object()
            .bucket(&self.bucket_name)
            .key(key)
            .body(data.into_bytes().into())
            .tagging(tagging_string)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to write to S3: {}", e))?;

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

        let get_object_output = self
            .client
            .get_object()
            .bucket(&self.bucket_name)
            .key(key)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to read from S3: {}", e))?;

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
}
