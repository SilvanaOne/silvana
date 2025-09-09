use anyhow::{Result, anyhow};
use serde_json::{self, Value};
use std::env;
use std::sync::OnceLock;
use std::time::Instant;
use tokio::time::{Duration, sleep};
use tracing::{debug, error, warn};

/// Get the maximum number of retries for Walrus operations from env var or default
fn get_max_retries() -> u32 {
    static MAX_RETRIES_CACHE: OnceLock<u32> = OnceLock::new();
    *MAX_RETRIES_CACHE.get_or_init(|| {
        env::var("WALRUS_MAX_RETRIES")
            .unwrap_or_else(|_| "4".to_string())
            .parse::<u32>()
            .unwrap_or(4)
    })
}

#[derive(Debug, Clone)]
pub enum Daemon {
    Local,
    Testnet,
}

impl Default for Daemon {
    fn default() -> Self {
        Daemon::Testnet
    }
}

#[derive(Debug, Clone)]
pub struct WalrusConfig {
    pub daemon: Daemon,
    pub min_epochs: u32,
    pub max_epochs: u32,
}

impl Default for WalrusConfig {
    fn default() -> Self {
        Self {
            daemon: Daemon::Testnet,
            min_epochs: 2,
            max_epochs: 53,
        }
    }
}

impl WalrusConfig {
    pub fn base_publisher_url(&self) -> String {
        match self.daemon {
            Daemon::Local => "http://127.0.0.1:31415".to_string(),
            Daemon::Testnet => env::var("WALRUS_PUBLISHER")
                .unwrap_or_else(|_| "https://wal-publisher-testnet.staketab.org".to_string()),
        }
    }

    pub fn reader_url(&self) -> String {
        match self.daemon {
            Daemon::Local => "http://127.0.0.1:31415/v1/blobs/".to_string(),
            Daemon::Testnet => env::var("WALRUS_AGGREGATOR").unwrap_or_else(|_| {
                "https://wal-aggregator-testnet.staketab.org/v1/blobs/".to_string()
            }),
        }
    }
}

pub struct WalrusClient {
    config: WalrusConfig,
    client: reqwest::Client,
}

impl WalrusClient {
    pub fn new() -> Self {
        Self {
            config: WalrusConfig::default(),
            client: reqwest::Client::new(),
        }
    }

    pub fn with_config(config: WalrusConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    pub async fn save_to_walrus(&self, params: SaveToWalrusParams) -> Result<Option<WalrusStoreResult>> {
        match params.data {
            WalrusData::Blob(data) => self.save_blob_to_walrus(data, params.address, params.num_epochs).await,
            WalrusData::Quilt(quilt_data) => self.save_quilt_to_walrus(quilt_data, params.address, params.num_epochs).await,
        }
    }

    async fn save_blob_to_walrus(&self, data: String, address: Option<String>, num_epochs: Option<u32>) -> Result<Option<WalrusStoreResult>> {
        let send_to_param = address
            .map(|addr| format!("&send_object_to={}", addr))
            .unwrap_or_default();

        let epochs = num_epochs
            .unwrap_or(2)
            .clamp(self.config.min_epochs, self.config.max_epochs);

        let url = format!(
            "{}/v1/blobs?epochs={}{}",
            self.config.base_publisher_url(),
            epochs,
            send_to_param
        );

        let max_retries = get_max_retries();
        const BASE_RETRY_DELAY_SECS: u64 = 5;

        for attempt in 1..=max_retries {
            debug!("Writing to Walrus (attempt {}/{})", attempt, max_retries);
            let start = Instant::now();

            // Clone the data for each attempt since body() consumes it
            let data_clone = data.clone();

            let response = match self.client.put(&url).body(data_clone).send().await {
                Ok(resp) => resp,
                Err(e) => {
                    if attempt < max_retries {
                        // Exponential backoff: double the delay for each retry
                        let retry_delay = BASE_RETRY_DELAY_SECS * 2_u64.pow((attempt - 1) as u32);
                        warn!(
                            "Failed to send request to Walrus (attempt {}/{}): {}. Retrying in {} seconds...",
                            attempt, max_retries, e, retry_delay
                        );
                        sleep(Duration::from_secs(retry_delay)).await;
                        continue;
                    } else {
                        error!(
                            "Failed to send request to Walrus after {} attempts: {}",
                            max_retries, e
                        );
                        return Err(anyhow!(
                            "Failed to send request after {} attempts: {}",
                            max_retries,
                            e
                        ));
                    }
                }
            };

            let elapsed = start.elapsed();
            debug!("Request completed in {:?}", elapsed);

            if response.status().is_success() {
                let info: Value = match response.json().await {
                    Ok(json) => json,
                    Err(e) => {
                        if attempt < max_retries {
                            // Exponential backoff: double the delay for each retry
                            let retry_delay = BASE_RETRY_DELAY_SECS * 2_u64.pow((attempt - 1) as u32);
                            warn!(
                                "Failed to parse response (attempt {}/{}): {}. Retrying in {} seconds...",
                                attempt, max_retries, e, retry_delay
                            );
                            sleep(Duration::from_secs(retry_delay)).await;
                            continue;
                        } else {
                            error!(
                                "Failed to parse response after {} attempts: {}",
                                max_retries, e
                            );
                            return Err(anyhow!(
                                "Failed to parse response after {} attempts: {}",
                                max_retries,
                                e
                            ));
                        }
                    }
                };

                let blob_id = info
                    .pointer("/newlyCreated/blobObject/blobId")
                    .or_else(|| info.pointer("/alreadyCertified/blobId"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let tx_digest = info
                    .pointer("/newlyCreated/event/txDigest")
                    .or_else(|| info.pointer("/alreadyCertified/event/txDigest"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                if let Some(id) = blob_id {
                    debug!("Successfully saved to Walrus. BlobId: {}", id);
                    return Ok(Some(WalrusStoreResult {
                        blob_id: id,
                        tx_digest,
                        quilt_patches: None,
                    }));
                } else {
                    if attempt < max_retries {
                        // Exponential backoff: double the delay for each retry
                        let retry_delay = BASE_RETRY_DELAY_SECS * 2_u64.pow((attempt - 1) as u32);
                        warn!(
                            "No blob_id in response (attempt {}/{}). Retrying in {} seconds...",
                            attempt, max_retries, retry_delay
                        );
                        sleep(Duration::from_secs(retry_delay)).await;
                        continue;
                    } else {
                        error!("No blob_id in response after {} attempts", max_retries);
                        return Ok(None);
                    }
                }
            } else {
                let status = response.status();
                let status_text = response
                    .status()
                    .canonical_reason()
                    .unwrap_or("Unknown error");

                // For 500 errors and other server errors, retry
                if status.is_server_error() && attempt < max_retries {
                    // Exponential backoff: double the delay for each retry
                    let retry_delay = BASE_RETRY_DELAY_SECS * 2_u64.pow((attempt - 1) as u32);
                    warn!(
                        "Walrus returned error {} {} (attempt {}/{}). Retrying in {} seconds...",
                        status, status_text, attempt, max_retries, retry_delay
                    );
                    sleep(Duration::from_secs(retry_delay)).await;
                    continue;
                } else if attempt >= max_retries {
                    error!(
                        "Walrus saveToDA failed after {} attempts: {} {}",
                        max_retries, status, status_text
                    );
                    return Ok(None);
                } else {
                    // For client errors (4xx), don't retry
                    error!(
                        "Walrus saveToDA failed with client error: {} {}. Not retrying.",
                        status, status_text
                    );
                    return Ok(None);
                }
            }
        }

        // This should not be reached, but just in case
        error!("Walrus saveToDA failed after all retry attempts");
        Ok(None)
    }

    async fn save_quilt_to_walrus(&self, quilt_data: Vec<(String, String)>, address: Option<String>, num_epochs: Option<u32>) -> Result<Option<WalrusStoreResult>> {
        use reqwest::multipart;
        
        let send_to_param = address
            .map(|addr| format!("&send_object_to={}", addr))
            .unwrap_or_default();

        let epochs = num_epochs
            .unwrap_or(2)
            .clamp(self.config.min_epochs, self.config.max_epochs);

        let url = format!(
            "{}/v1/quilts?epochs={}{}",
            self.config.base_publisher_url(),
            epochs,
            send_to_param
        );

        let max_retries = get_max_retries();
        const BASE_RETRY_DELAY_SECS: u64 = 5;

        for attempt in 1..=max_retries {
            debug!("Writing quilt to Walrus (attempt {}/{})", attempt, max_retries);
            let start = Instant::now();

            // Build multipart form
            let mut form = multipart::Form::new();
            
            // Add metadata field
            let mut metadata_json = Vec::new();
            for (identifier, _) in &quilt_data {
                metadata_json.push(serde_json::json!({
                    "identifier": identifier
                }));
            }
            form = form.text("_metadata", serde_json::to_string(&metadata_json)?);
            
            // Add data parts
            for (identifier, data) in &quilt_data {
                form = form.text(identifier.clone(), data.clone());
            }

            let response = match self.client.put(&url).multipart(form).send().await {
                Ok(resp) => resp,
                Err(e) => {
                    if attempt < max_retries {
                        let retry_delay = BASE_RETRY_DELAY_SECS * 2_u64.pow((attempt - 1) as u32);
                        warn!(
                            "Failed to send quilt request to Walrus (attempt {}/{}): {}. Retrying in {} seconds...",
                            attempt, max_retries, e, retry_delay
                        );
                        sleep(Duration::from_secs(retry_delay)).await;
                        continue;
                    } else {
                        error!(
                            "Failed to send quilt request to Walrus after {} attempts: {}",
                            max_retries, e
                        );
                        return Err(anyhow!(
                            "Failed to send quilt request after {} attempts: {}",
                            max_retries,
                            e
                        ));
                    }
                }
            };

            let elapsed = start.elapsed();
            debug!("Request completed in {:?}", elapsed);

            if response.status().is_success() {
                let info: Value = match response.json().await {
                    Ok(json) => json,
                    Err(e) => {
                        if attempt < max_retries {
                            let retry_delay = BASE_RETRY_DELAY_SECS * 2_u64.pow((attempt - 1) as u32);
                            warn!(
                                "Failed to parse quilt response (attempt {}/{}): {}. Retrying in {} seconds...",
                                attempt, max_retries, e, retry_delay
                            );
                            sleep(Duration::from_secs(retry_delay)).await;
                            continue;
                        } else {
                            error!(
                                "Failed to parse quilt response after {} attempts: {}",
                                max_retries, e
                            );
                            return Err(anyhow!(
                                "Failed to parse quilt response after {} attempts: {}",
                                max_retries,
                                e
                            ));
                        }
                    }
                };

                // For quilts, the main blob ID is returned in blobStoreResult
                let blob_id = info
                    .pointer("/blobStoreResult/newlyCreated/blobObject/blobId")
                    .or_else(|| info.pointer("/blobStoreResult/alreadyCertified/blobId"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let tx_digest = info
                    .pointer("/blobStoreResult/newlyCreated/event/txDigest")
                    .or_else(|| info.pointer("/blobStoreResult/alreadyCertified/event/txDigest"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                // Extract quilt patches
                let quilt_patches = info
                    .pointer("/storedQuiltBlobs")
                    .and_then(|v| v.as_array())
                    .map(|patches| {
                        patches
                            .iter()
                            .filter_map(|patch| {
                                let identifier = patch.get("identifier")?.as_str()?.to_string();
                                let quilt_patch_id = patch.get("quiltPatchId")?.as_str()?.to_string();
                                Some(QuiltPatch {
                                    identifier,
                                    quilt_patch_id,
                                })
                            })
                            .collect::<Vec<_>>()
                    });

                if let Some(id) = blob_id {
                    debug!("Successfully saved quilt to Walrus. QuiltId: {}", id);
                    return Ok(Some(WalrusStoreResult {
                        blob_id: id,
                        tx_digest,
                        quilt_patches,
                    }));
                } else {
                    if attempt < max_retries {
                        let retry_delay = BASE_RETRY_DELAY_SECS * 2_u64.pow((attempt - 1) as u32);
                        warn!(
                            "No quilt_id in response (attempt {}/{}). Retrying in {} seconds...",
                            attempt, max_retries, retry_delay
                        );
                        sleep(Duration::from_secs(retry_delay)).await;
                        continue;
                    } else {
                        error!("No quilt_id in response after {} attempts", max_retries);
                        return Ok(None);
                    }
                }
            } else {
                let status = response.status();
                let status_text = response
                    .status()
                    .canonical_reason()
                    .unwrap_or("Unknown error");

                if status.is_server_error() && attempt < max_retries {
                    let retry_delay = BASE_RETRY_DELAY_SECS * 2_u64.pow((attempt - 1) as u32);
                    warn!(
                        "Walrus returned error {} {} (attempt {}/{}). Retrying in {} seconds...",
                        status, status_text, attempt, max_retries, retry_delay
                    );
                    sleep(Duration::from_secs(retry_delay)).await;
                    continue;
                } else if attempt >= max_retries {
                    error!(
                        "Walrus saveQuiltToDA failed after {} attempts: {} {}",
                        max_retries, status, status_text
                    );
                    return Ok(None);
                } else {
                    // Try to get error details from response body
                    let error_body = response.text().await.unwrap_or_else(|e| {
                        format!("Failed to read error body: {}", e)
                    });
                    
                    error!(
                        "Walrus saveQuiltToDA failed with client error: {} {} - Body: {}. Not retrying.",
                        status, status_text, error_body
                    );
                    
                    // Log additional details with tracing
                    tracing::error!(
                        status_code = %status,
                        status_text = %status_text,
                        error_body = %error_body,
                        quilt_pieces = quilt_data.len(),
                        "Walrus saveQuiltToDA detailed error"
                    );
                    
                    return Ok(None);
                }
            }
        }

        error!("Walrus saveQuiltToDA failed after all retry attempts");
        Ok(None)
    }

    pub async fn read_from_walrus(&self, params: ReadFromWalrusParams) -> Result<Option<String>> {
        if params.blob_id.is_empty() {
            return Err(anyhow!("blobId is not provided"));
        }

        // If quilt_identifier is provided, read specific quilt piece
        let url = if let Some(identifier) = params.quilt_identifier {
            format!("{}by-quilt-id/{}/{}", 
                self.config.reader_url().replace("/v1/blobs/", "/v1/blobs/"),
                params.blob_id, 
                identifier
            )
        } else {
            format!("{}{}", self.config.reader_url(), params.blob_id)
        };

        let max_retries = get_max_retries();
        const BASE_RETRY_DELAY_SECS: u64 = 5;

        for attempt in 1..=max_retries {
            debug!(
                "Reading walrus blob: {} (attempt {}/{})",
                params.blob_id, attempt, max_retries
            );
            let start = Instant::now();

            let response = match self.client.get(&url).send().await {
                Ok(resp) => resp,
                Err(e) => {
                    if attempt < max_retries {
                        // Exponential backoff: double the delay for each retry
                        let retry_delay = BASE_RETRY_DELAY_SECS * 2_u64.pow((attempt - 1) as u32);
                        warn!(
                            "Failed to read from Walrus (attempt {}/{}): {}. Retrying in {} seconds...",
                            attempt, max_retries, e, retry_delay
                        );
                        sleep(Duration::from_secs(retry_delay)).await;
                        continue;
                    } else {
                        error!(
                            "Failed to read from Walrus after {} attempts: {}",
                            max_retries, e
                        );
                        return Err(anyhow!(
                            "Failed to read from Walrus after {} attempts: {}",
                            max_retries,
                            e
                        ));
                    }
                }
            };

            let elapsed = start.elapsed();
            debug!("Request completed in {:?}", elapsed);

            if response.status().is_success() {
                let blob = match response.text().await {
                    Ok(text) => text,
                    Err(e) => {
                        if attempt < max_retries {
                            // Exponential backoff: double the delay for each retry
                            let retry_delay = BASE_RETRY_DELAY_SECS * 2_u64.pow((attempt - 1) as u32);
                            warn!(
                                "Failed to read response body (attempt {}/{}): {}. Retrying in {} seconds...",
                                attempt, max_retries, e, retry_delay
                            );
                            sleep(Duration::from_secs(retry_delay)).await;
                            continue;
                        } else {
                            error!(
                                "Failed to read response body after {} attempts: {}",
                                max_retries, e
                            );
                            return Err(anyhow!(
                                "Failed to read response after {} attempts: {}",
                                max_retries,
                                e
                            ));
                        }
                    }
                };
                debug!("Successfully read blob from Walrus");
                return Ok(Some(blob));
            } else {
                let status = response.status();
                let status_text = response
                    .status()
                    .canonical_reason()
                    .unwrap_or("Unknown error");

                // For 500 errors and other server errors, retry
                if status.is_server_error() && attempt < max_retries {
                    // Exponential backoff: double the delay for each retry
                    let retry_delay = BASE_RETRY_DELAY_SECS * 2_u64.pow((attempt - 1) as u32);
                    warn!(
                        "Walrus returned error {} {} (attempt {}/{}). Retrying in {} seconds...",
                        status, status_text, attempt, max_retries, retry_delay
                    );
                    sleep(Duration::from_secs(retry_delay)).await;
                    continue;
                } else if attempt >= max_retries {
                    error!(
                        "Walrus readFromDA failed after {} attempts: {} {}",
                        max_retries, status, status_text
                    );
                    return Ok(None);
                } else {
                    // For client errors (4xx), don't retry
                    error!(
                        "Walrus readFromDA failed with client error: {} {}. Not retrying.",
                        status, status_text
                    );
                    return Ok(None);
                }
            }
        }

        // This should not be reached, but just in case
        error!("Walrus readFromDA failed after all retry attempts");
        Ok(None)
    }

    pub fn get_walrus_url(&self, params: GetWalrusUrlParams) -> Result<String> {
        if params.blob_id.is_empty() {
            return Err(anyhow!("blobId is not set"));
        }

        let url = format!("{}{}", self.config.reader_url(), params.blob_id);
        Ok(url)
    }

    pub async fn list_quilt_patches(&self, quilt_id: &str) -> Result<Vec<QuiltPatch>> {
        if quilt_id.is_empty() {
            return Err(anyhow!("quilt_id is not provided"));
        }

        // First, we need to read the quilt metadata
        // The Walrus CLI uses a special endpoint to list patches
        let url = format!("{}by-quilt-id/{}", 
            self.config.reader_url().replace("/v1/blobs/", "/v1/blobs/"),
            quilt_id
        );

        let max_retries = get_max_retries();
        const BASE_RETRY_DELAY_SECS: u64 = 5;

        for attempt in 1..=max_retries {
            debug!("Listing quilt patches for: {} (attempt {}/{})", quilt_id, attempt, max_retries);
            
            // Try to get quilt metadata by making a HEAD request
            let response = match self.client.head(&url).send().await {
                Ok(resp) => resp,
                Err(e) => {
                    if attempt < max_retries {
                        let retry_delay = BASE_RETRY_DELAY_SECS * 2_u64.pow((attempt - 1) as u32);
                        warn!(
                            "Failed to list quilt patches (attempt {}/{}): {}. Retrying in {} seconds...",
                            attempt, max_retries, e, retry_delay
                        );
                        sleep(Duration::from_secs(retry_delay)).await;
                        continue;
                    } else {
                        error!("Failed to list quilt patches after {} attempts: {}", max_retries, e);
                        return Err(anyhow!("Failed to list quilt patches after {} attempts: {}", max_retries, e));
                    }
                }
            };

            if response.status().is_success() {
                // Parse quilt patch info from headers if available
                // Note: The actual implementation depends on what Walrus returns
                // For now, return an empty vec as we'd need more info about the API
                warn!("Quilt patch listing requires additional API information");
                return Ok(Vec::new());
            } else {
                if attempt < max_retries {
                    let retry_delay = BASE_RETRY_DELAY_SECS * 2_u64.pow((attempt - 1) as u32);
                    warn!(
                        "Failed to list quilt patches (attempt {}/{}). Retrying in {} seconds...",
                        attempt, max_retries, retry_delay
                    );
                    sleep(Duration::from_secs(retry_delay)).await;
                    continue;
                } else {
                    return Err(anyhow!("Failed to list quilt patches after {} attempts", max_retries));
                }
            }
        }

        Err(anyhow!("Failed to list quilt patches after all retry attempts"))
    }
}

impl Default for WalrusClient {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub enum WalrusData {
    Blob(String),
    Quilt(Vec<(String, String)>), // (identifier, data)
}

#[derive(Debug, Clone)]
pub struct SaveToWalrusParams {
    pub data: WalrusData,
    pub address: Option<String>,
    pub num_epochs: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct WalrusStoreResult {
    pub blob_id: String,
    pub tx_digest: Option<String>,
    pub quilt_patches: Option<Vec<QuiltPatch>>,
}

#[derive(Debug, Clone)]
pub struct QuiltPatch {
    pub identifier: String,
    pub quilt_patch_id: String,
}

#[derive(Debug, Clone)]
pub struct ReadFromWalrusParams {
    pub blob_id: String,
    pub quilt_identifier: Option<String>, // Optional: read specific quilt piece by identifier
}

#[derive(Debug, Clone)]
pub struct GetWalrusUrlParams {
    pub blob_id: String,
}
