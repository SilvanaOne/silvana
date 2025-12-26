//! Health endpoint configuration from health.toml

use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::{debug, info};

/// Default timeout for endpoint fetches in seconds
fn default_timeout() -> u64 {
    5
}

/// Main configuration structure for health.toml
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct HealthTomlConfig {
    /// List of endpoints to fetch on each health check
    #[serde(default)]
    pub endpoints: Vec<EndpointConfig>,
}

/// Configuration for a single endpoint to fetch
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EndpointConfig {
    /// Name/identifier for this endpoint
    pub name: String,

    /// URL to fetch
    pub url: String,

    /// Timeout in seconds (default: 5)
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

impl HealthTomlConfig {
    /// Load configuration from a TOML file
    ///
    /// Returns the configuration if the file exists and is valid,
    /// or a default empty configuration if the file doesn't exist.
    pub fn from_file(path: &str) -> anyhow::Result<Self> {
        let path = Path::new(path);

        if !path.exists() {
            debug!("health.toml not found at {:?}, using default (no endpoints)", path);
            return Ok(Self::default());
        }

        let contents = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&contents)?;

        info!(
            "Loaded health.toml with {} endpoint(s)",
            config.endpoints.len()
        );

        for endpoint in &config.endpoints {
            info!(
                "  - {}: {} (timeout: {}s)",
                endpoint.name, endpoint.url, endpoint.timeout_secs
            );
        }

        Ok(config)
    }

    /// Try to load health.toml from the current directory
    pub fn try_load() -> Self {
        match Self::from_file("health.toml") {
            Ok(config) => config,
            Err(e) => {
                tracing::warn!("Failed to load health.toml: {}", e);
                Self::default()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = HealthTomlConfig::default();
        assert!(config.endpoints.is_empty());
    }

    #[test]
    fn test_parse_config() {
        let toml_content = r#"
[[endpoints]]
name = "tidb-status"
url = "http://localhost:10080/status"
timeout_secs = 10

[[endpoints]]
name = "pd-stores"
url = "http://localhost:2379/pd/api/v1/stores"
"#;

        let config: HealthTomlConfig = toml::from_str(toml_content).unwrap();
        assert_eq!(config.endpoints.len(), 2);
        assert_eq!(config.endpoints[0].name, "tidb-status");
        assert_eq!(config.endpoints[0].timeout_secs, 10);
        assert_eq!(config.endpoints[1].name, "pd-stores");
        assert_eq!(config.endpoints[1].timeout_secs, 5); // default
    }
}
