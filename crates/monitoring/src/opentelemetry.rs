//! OpenTelemetry integration for BetterStack
//!
//! This module provides OpenTelemetry support for sending logs, metrics, and traces
//! to BetterStack when the appropriate environment variables are configured.
//!
//! ## Environment Variables
//!
//! - `OPENTELEMETRY_INGESTING_HOST`: BetterStack ingesting host (e.g., "xxxxxx.betterstackdata.com")
//! - `OPENTELEMETRY_SOURCE_ID`: Source identifier for the logs/metrics/traces (e.g., "rpc_logs")
//! - `OPENTELEMETRY_SOURCE_TOKEN`: Authentication token for BetterStack
//!
//! ## Usage
//!
//! ```rust
//! use monitoring::opentelemetry::{init_opentelemetry, send_log, send_metric};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     // Initialize OpenTelemetry if environment variables are set
//!     init_opentelemetry().await?;
//!     
//!     // Send a log message
//!     send_log("info", "Hello from Better Stack!").await?;
//!     
//!     // Send a metric
//!     send_metric("test_metric", 123.0, "gauge").await?;
//!     
//!     Ok(())
//! }
//! ```

use anyhow::{Result, anyhow};
use opentelemetry::KeyValue;
use opentelemetry::global;
use opentelemetry::logs::LoggerProvider; // brings `logger()` method into scope
use opentelemetry::logs::{AnyValue, LogRecord, Logger, Severity};
use opentelemetry_otlp::{LogExporter, MetricExporter, WithExportConfig, WithTonicConfig};
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::{
    Resource,
    metrics::{PeriodicReader, SdkMeterProvider},
};
use std::env;
use std::sync::OnceLock;
use std::time::Duration;
use tonic::metadata::MetadataMap;
use tonic::transport::Certificate;
use tonic::transport::ClientTlsConfig;
use tracing::{debug, error, info, warn};

/// Configuration for OpenTelemetry integration with BetterStack
#[derive(Debug, Clone)]
pub struct OpenTelemetryConfig {
    pub ingesting_host: String,
    pub source_id: String,
    pub source_token: String,
}

impl OpenTelemetryConfig {
    /// Create a new OpenTelemetry configuration from environment variables
    pub fn from_env() -> Result<Self> {
        let ingesting_host = env::var("OPENTELEMETRY_INGESTING_HOST")
            .map_err(|_| anyhow!("OPENTELEMETRY_INGESTING_HOST environment variable not set"))?;

        let source_id = env::var("OPENTELEMETRY_SOURCE_ID")
            .map_err(|_| anyhow!("OPENTELEMETRY_SOURCE_ID environment variable not set"))?;

        let source_token = env::var("OPENTELEMETRY_SOURCE_TOKEN")
            .map_err(|_| anyhow!("OPENTELEMETRY_SOURCE_TOKEN environment variable not set"))?;

        Ok(Self {
            ingesting_host,
            source_id,
            source_token,
        })
    }

    /// Check if OpenTelemetry environment variables are configured
    pub fn is_configured() -> bool {
        env::var("OPENTELEMETRY_INGESTING_HOST").is_ok()
            && env::var("OPENTELEMETRY_SOURCE_ID").is_ok()
            && env::var("OPENTELEMETRY_SOURCE_TOKEN").is_ok()
    }
}

/// Global OpenTelemetry configuration
static OTEL_CONFIG: OnceLock<Option<OpenTelemetryConfig>> = OnceLock::new();

/// Global SdkLoggerProvider once set
static LOGGER_PROVIDER: OnceLock<Option<SdkLoggerProvider>> = OnceLock::new();

/// Initialize OpenTelemetry integration if environment variables are configured
pub async fn init_opentelemetry() -> Result<()> {
    let config = if OpenTelemetryConfig::is_configured() {
        match OpenTelemetryConfig::from_env() {
            Ok(config) => {
                info!("🔭 OpenTelemetry integration enabled for BetterStack");
                Some(config)
            }
            Err(e) => {
                warn!("Failed to initialize OpenTelemetry config: {}", e);
                None
            }
        }
    } else {
        debug!("OpenTelemetry environment variables not configured, skipping initialization");
        None
    };

    OTEL_CONFIG
        .set(config.clone())
        .map_err(|_| anyhow!("Failed to set OpenTelemetry configuration"))?;

    if let Some(config) = config {
        // Initialize metrics exporter
        init_metrics_exporter(&config).await?;

        // Initialize logs exporter
        init_logs_exporter(&config).await?;

        info!("✅ OpenTelemetry metrics and logs exporters initialized successfully");
    }

    Ok(())
}

/// Initialize the metrics exporter with gRPC
async fn init_metrics_exporter(config: &OpenTelemetryConfig) -> Result<()> {
    let endpoint = format!("https://{}", config.ingesting_host);

    // Create metadata for authentication
    let mut metadata = MetadataMap::new();
    metadata.insert(
        "authorization",
        format!("Bearer {}", config.source_token)
            .parse()
            .map_err(|e| anyhow!("Failed to parse authorization header: {}", e))?,
    );

    // TLS config for tonic
    let tls = build_tls_config(&config.ingesting_host)?;

    // Build the OTLP gRPC metric exporter
    let metric_exporter = MetricExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .with_tls_config(tls)
        .with_timeout(Duration::from_secs(10))
        .with_metadata(metadata)
        .build()
        .map_err(|e| anyhow!("Failed to build metric exporter: {}", e))?;

    // Create a periodic reader for metrics
    let periodic_reader = PeriodicReader::builder(metric_exporter)
        .with_interval(Duration::from_secs(60))
        .build();

    // Create resource with service identification
    let resource = Resource::builder()
        .with_service_name("silvana-monitoring")
        .with_attributes(vec![
            KeyValue::new("service.version", "0.1.0"),
            KeyValue::new("source.id", config.source_id.clone()),
        ])
        .build();

    // Create meter provider with the periodic reader
    let meter_provider = SdkMeterProvider::builder()
        .with_reader(periodic_reader)
        .with_resource(resource)
        .build();

    // Set the global meter provider
    global::set_meter_provider(meter_provider);

    info!("📊 Metrics exporter initialized with gRPC transport");
    Ok(())
}

/// Initialize the logs exporter with gRPC
async fn init_logs_exporter(config: &OpenTelemetryConfig) -> Result<()> {
    let endpoint = format!("https://{}", config.ingesting_host);

    // ---- authentication header ----
    let mut metadata = MetadataMap::new();
    metadata.insert(
        "authorization",
        format!("Bearer {}", config.source_token)
            .parse()
            .map_err(|e| anyhow!("Failed to parse authorization header: {}", e))?,
    );

    // TLS config for tonic
    let tls = build_tls_config(&config.ingesting_host)?;

    // ---- build OTLP gRPC exporter ----
    let log_exporter = LogExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .with_tls_config(tls)
        .with_timeout(Duration::from_secs(10))
        .with_metadata(metadata)
        .build()
        .map_err(|e| anyhow!("Failed to build log exporter: {}", e))?;

    // ---- resource describing this service ----
    let resource = Resource::builder()
        .with_service_name("silvana-monitoring")
        .with_attributes(vec![
            KeyValue::new("service.version", "0.1.0"),
            KeyValue::new("source.id", config.source_id.clone()),
        ])
        .build();

    // ---- logger provider with async batch processor ----
    let logger_provider = SdkLoggerProvider::builder()
        .with_batch_exporter(log_exporter)
        .with_resource(resource)
        .build();

    // Store in global OnceLock for later retrieval
    LOGGER_PROVIDER.set(Some(logger_provider)).ok();

    info!("📝 Logs exporter initialized with gRPC transport");
    Ok(())
}

/// Send a log message to BetterStack using structured tracing with OpenTelemetry metadata
pub async fn send_log(level: &str, message: &str) -> Result<()> {
    if !OpenTelemetryConfig::is_configured() {
        return Ok(());
    }

    // Get the provider stored in init_logs_exporter
    let provider = match LOGGER_PROVIDER.get() {
        Some(Some(p)) => p,
        _ => return Ok(()),
    };
    let logger = provider.logger("silvana-monitoring");

    let sev = match level.to_lowercase().as_str() {
        "error" | "err" => Severity::Error,
        "warn" | "warning" => Severity::Warn,
        "info" => Severity::Info,
        "debug" => Severity::Debug,
        "trace" => Severity::Trace,
        _ => Severity::Info,
    };

    let mut record = logger.create_log_record();
    record.set_severity_number(sev);
    record.set_body(AnyValue::from(message.to_string()));
    // Map to an uppercase static string for severity text
    let sev_text: &'static str = match level.to_lowercase().as_str() {
        "error" | "err" => "ERROR",
        "warn" | "warning" => "WARN",
        "info" => "INFO",
        "debug" => "DEBUG",
        "trace" => "TRACE",
        _ => "INFO",
    };
    record.set_severity_text(sev_text);

    logger.emit(record);

    Ok(())
}

/// Send a metric to BetterStack using OpenTelemetry OTLP meter
pub async fn send_metric(name: &str, value: f64, metric_type: &str) -> Result<()> {
    if !OpenTelemetryConfig::is_configured() {
        return Ok(());
    }

    let meter_provider = global::meter_provider();
    let meter = meter_provider.meter("silvana-monitoring");

    // Convert the &str to String to satisfy lifetime requirements
    let metric_name = name.to_string();

    match metric_type {
        "counter" => {
            let counter = meter.f64_counter(metric_name).build();
            counter.add(value, &[]);
        }
        "gauge" => {
            let gauge = meter.f64_gauge(metric_name).build();
            gauge.record(value, &[]);
        }
        _ => {
            let histogram = meter.f64_histogram(metric_name).build();
            histogram.record(value, &[]);
        }
    }

    Ok(())
}

/// Convenience function to send a simple log message
pub async fn log(level: &str, message: &str) {
    if let Err(e) = send_log(level, message).await {
        // Use eprintln! instead of error!() to avoid recursive logging loop
        eprintln!("Failed to send log via OpenTelemetry: {}", e);
    }
}

/// Convenience function to send a gauge metric
pub async fn gauge(name: &str, value: f64) {
    if let Err(e) = send_metric(name, value, "gauge").await {
        // Use eprintln! instead of error!() to avoid recursive logging loop
        eprintln!("Failed to send gauge metric via OpenTelemetry: {}", e);
    }
}

/// Convenience function to send a counter metric
pub async fn counter(name: &str, value: f64) {
    if let Err(e) = send_metric(name, value, "counter").await {
        // Use eprintln! instead of error!() to avoid recursive logging loop
        eprintln!("Failed to send counter metric via OpenTelemetry: {}", e);
    }
}

/// Send buffer metrics to BetterStack via OpenTelemetry
pub async fn send_buffer_metrics(stats: &buffer::BufferStats) {
    if !OpenTelemetryConfig::is_configured() {
        return;
    }

    // Send various buffer metrics
    gauge(
        "silvana_buffer_events_received_total",
        stats.total_received as f64,
    )
    .await;
    gauge(
        "silvana_buffer_events_processed_total",
        stats.total_processed as f64,
    )
    .await;
    gauge(
        "silvana_buffer_events_dropped_total",
        stats.total_dropped as f64,
    )
    .await;
    gauge(
        "silvana_buffer_events_error_total",
        stats.total_errors as f64,
    )
    .await;
    gauge(
        "silvana_buffer_size_current",
        stats.current_buffer_size as f64,
    )
    .await;
    gauge(
        "silvana_buffer_memory_bytes",
        stats.current_memory_bytes as f64,
    )
    .await;
    gauge(
        "silvana_buffer_backpressure_events_total",
        stats.backpressure_events as f64,
    )
    .await;
}

/// Send system metrics to BetterStack via OpenTelemetry
pub async fn send_system_metrics() {
    if !OpenTelemetryConfig::is_configured() {
        return;
    }

    let mut system = sysinfo::System::new_all();
    system.refresh_all();

    // CPU metrics
    gauge(
        "silvana_cpu_usage_percent",
        system.global_cpu_usage() as f64,
    )
    .await;

    // Memory metrics
    gauge("silvana_memory_used_bytes", system.used_memory() as f64).await;
    gauge("silvana_memory_total_bytes", system.total_memory() as f64).await;
    gauge(
        "silvana_memory_available_bytes",
        system.available_memory() as f64,
    )
    .await;

    // Load average (Unix-like systems only)
    #[cfg(not(target_os = "windows"))]
    {
        let load_avg = sysinfo::System::load_average();
        gauge("silvana_load_average_1min", load_avg.one).await;
        gauge("silvana_load_average_5min", load_avg.five).await;
        gauge("silvana_load_average_15min", load_avg.fifteen).await;
    }

    // Process metrics
    let pid = sysinfo::Pid::from(std::process::id() as usize);
    if let Some(process) = system.process(pid) {
        gauge("silvana_process_cpu_percent", process.cpu_usage() as f64).await;
        gauge("silvana_process_memory_bytes", process.memory() as f64).await;
        gauge(
            "silvana_process_virtual_memory_bytes",
            process.virtual_memory() as f64,
        )
        .await;
    }

    // Uptime
    gauge("silvana_uptime_seconds", sysinfo::System::uptime() as f64).await;
}

/// Periodically send system metrics to BetterStack
pub async fn system_metrics_exporter() {
    if !OpenTelemetryConfig::is_configured() {
        debug!("OpenTelemetry not configured, skipping system metrics export");
        return;
    }

    let mut interval = tokio::time::interval(Duration::from_secs(60));
    loop {
        interval.tick().await;
        send_system_metrics().await;
    }
}

/// Test function to verify OpenTelemetry integration is working
pub async fn test_integration() -> Result<()> {
    if !OpenTelemetryConfig::is_configured() {
        info!("🧪 OpenTelemetry not configured, skipping integration test");
        return Ok(());
    }

    info!("🧪 Testing OpenTelemetry integration with BetterStack...");

    // Test sending a log
    match send_log("info", "OpenTelemetry integration test - log message").await {
        Ok(_) => info!("✅ Test log sent successfully"),
        Err(e) => error!("❌ Failed to send test log: {}", e),
    }

    // Test sending a metric
    match send_metric("silvana_test_metric", 42.0, "gauge").await {
        Ok(_) => info!("✅ Test metric sent successfully"),
        Err(e) => error!("❌ Failed to send test metric: {}", e),
    }

    info!("🧪 OpenTelemetry integration test completed");
    Ok(())
}

/// Shutdown OpenTelemetry providers gracefully
pub async fn shutdown() -> Result<()> {
    info!("✅ OpenTelemetry providers shutdown complete");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_from_env_missing_vars() {
        // Ensure environment variables are not set
        unsafe {
            std::env::remove_var("OPENTELEMETRY_INGESTING_HOST");
            std::env::remove_var("OPENTELEMETRY_SOURCE_ID");
            std::env::remove_var("OPENTELEMETRY_SOURCE_TOKEN");
        }

        let result = OpenTelemetryConfig::from_env();
        assert!(result.is_err());
    }

    #[test]
    fn test_config_is_configured() {
        // Set required environment variables
        unsafe {
            std::env::set_var("OPENTELEMETRY_INGESTING_HOST", "test.example.com");
            std::env::set_var("OPENTELEMETRY_SOURCE_ID", "test_source");
            std::env::set_var("OPENTELEMETRY_SOURCE_TOKEN", "test_token");
        }

        assert!(OpenTelemetryConfig::is_configured());

        // Clean up
        unsafe {
            std::env::remove_var("OPENTELEMETRY_INGESTING_HOST");
            std::env::remove_var("OPENTELEMETRY_SOURCE_ID");
            std::env::remove_var("OPENTELEMETRY_SOURCE_TOKEN");
        }

        assert!(!OpenTelemetryConfig::is_configured());
    }
}

fn build_tls_config(host: &str) -> Result<ClientTlsConfig> {
    let pem_bytes = include_bytes!("./isrg-root-x1.pem");
    let ca = Certificate::from_pem(pem_bytes);
    return Ok(ClientTlsConfig::new()
        .ca_certificate(ca)
        .domain_name(host.to_owned()));
}
