use anyhow::{Result, anyhow};
use serde_json::Value;
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
            .unwrap_or_else(|_| "20".to_string())
            .parse::<u32>()
            .unwrap_or(10)
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
            min_epochs: 14,
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

    pub async fn save_to_walrus(&self, params: SaveToWalrusParams) -> Result<Option<String>> {
        let send_to_param = params
            .address
            .map(|addr| format!("&send_object_to={}", addr))
            .unwrap_or_default();

        let epochs = params
            .num_epochs
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
            let data_clone = params.data.clone();

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

                if let Some(ref id) = blob_id {
                    debug!("Successfully saved to Walrus. BlobId: {}", id);
                    return Ok(Some(id.clone()));
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

    pub async fn read_from_walrus(&self, params: ReadFromWalrusParams) -> Result<Option<String>> {
        if params.blob_id.is_empty() {
            return Err(anyhow!("blobId is not provided"));
        }

        let url = format!("{}{}", self.config.reader_url(), params.blob_id);

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
}

impl Default for WalrusClient {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct SaveToWalrusParams {
    pub data: String,
    pub address: Option<String>,
    pub num_epochs: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct ReadFromWalrusParams {
    pub blob_id: String,
}

#[derive(Debug, Clone)]
pub struct GetWalrusUrlParams {
    pub blob_id: String,
}
