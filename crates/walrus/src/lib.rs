use anyhow::{anyhow, Result};
use serde_json::Value;
use std::time::Instant;
use tracing::{info, error};

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
    pub fn base_publisher_url(&self) -> &'static str {
        match self.daemon {
            Daemon::Local => "http://127.0.0.1:31415",
            Daemon::Testnet => "https://wal-publisher-testnet.staketab.org",
        }
    }

    pub fn reader_url(&self) -> &'static str {
        match self.daemon {
            Daemon::Local => "http://127.0.0.1:31415/v1/blobs/",
            Daemon::Testnet => "https://wal-aggregator-testnet.staketab.org/v1/blobs/",
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

        info!("Writing to Walrus");
        let start = Instant::now();

        let url = format!(
            "{}/v1/blobs?epochs={}{}",
            self.config.base_publisher_url(),
            epochs,
            send_to_param
        );

        let response = self
            .client
            .put(&url)
            .body(params.data)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to send request: {}", e))?;

        let elapsed = start.elapsed();
        info!("Written in {:?}", elapsed);

        if response.status().is_success() {
            let info: Value = response
                .json()
                .await
                .map_err(|e| anyhow!("Failed to parse response: {}", e))?;

            let blob_id = info
                .pointer("/newlyCreated/blobObject/blobId")
                .or_else(|| info.pointer("/alreadyCertified/blobId"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            if let Some(ref id) = blob_id {
                info!("Walrus blobId: {}", id);
            }

            Ok(blob_id)
        } else {
            error!(
                "saveToDA failed: {} {}",
                response.status(),
                response.status().canonical_reason().unwrap_or("Unknown error")
            );
            Ok(None)
        }
    }

    pub async fn read_from_walrus(&self, params: ReadFromWalrusParams) -> Result<Option<String>> {
        if params.blob_id.is_empty() {
            return Err(anyhow!("blobId is not provided"));
        }

        info!("Reading walrus blob: {}", params.blob_id);
        let start = Instant::now();

        let url = format!("{}{}", self.config.reader_url(), params.blob_id);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to send request: {}", e))?;

        let elapsed = start.elapsed();
        info!("Read in {:?}", elapsed);

        if response.status().is_success() {
            let blob = response
                .text()
                .await
                .map_err(|e| anyhow!("Failed to read response: {}", e))?;
            Ok(Some(blob))
        } else {
            error!(
                "readFromDA failed: {} {}",
                response.status(),
                response.status().canonical_reason().unwrap_or("Unknown error")
            );
            Ok(None)
        }
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

