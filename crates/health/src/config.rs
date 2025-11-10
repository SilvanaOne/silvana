//! Configuration types for health metrics exporter

/// Configuration for the health metrics exporter
#[derive(Debug, Clone)]
pub struct ExporterConfig {
    /// Collection interval in seconds
    pub interval_secs: u64,
    /// Maximum number of retries for sending metrics
    pub max_retries: u32,
    /// Initial backoff delay in seconds for retries
    pub initial_backoff_secs: u64,
    /// Maximum backoff delay in seconds for retries
    pub max_backoff_secs: u64,
    /// HTTP request timeout in seconds
    pub http_timeout_secs: u64,
}

impl Default for ExporterConfig {
    fn default() -> Self {
        Self {
            interval_secs: 300, // 5 minutes
            max_retries: 3,
            initial_backoff_secs: 1,
            max_backoff_secs: 30,
            http_timeout_secs: 30,
        }
    }
}

impl ExporterConfig {
    /// Create a new configuration with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the collection interval
    pub fn with_interval(mut self, interval_secs: u64) -> Self {
        self.interval_secs = interval_secs;
        self
    }

    /// Set the maximum number of retries
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Set the HTTP timeout
    pub fn with_http_timeout(mut self, timeout_secs: u64) -> Self {
        self.http_timeout_secs = timeout_secs;
        self
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), String> {
        const MIN_INTERVAL_SECS: u64 = 60;
        const MAX_INTERVAL_SECS: u64 = 86400; // 24 hours

        if self.interval_secs < MIN_INTERVAL_SECS {
            return Err(format!(
                "Interval must be at least {} seconds, got {}",
                MIN_INTERVAL_SECS, self.interval_secs
            ));
        }

        if self.interval_secs > MAX_INTERVAL_SECS {
            return Err(format!(
                "Interval must be at most {} seconds, got {}",
                MAX_INTERVAL_SECS, self.interval_secs
            ));
        }

        if self.http_timeout_secs == 0 {
            return Err("HTTP timeout must be greater than 0".to_string());
        }

        Ok(())
    }
}
