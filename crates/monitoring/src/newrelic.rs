//! New Relic integration for OpenTelemetry
//!
//! This module provides OpenTelemetry support for sending logs, metrics, and traces
//! to New Relic using the OTLP (OpenTelemetry Protocol) endpoint.

use anyhow::{Result, anyhow};
use opentelemetry::KeyValue;
use opentelemetry::global;
use opentelemetry::logs::LoggerProvider;
use opentelemetry::logs::{AnyValue, LogRecord, Logger, Severity};
use opentelemetry_otlp::{LogExporter, MetricExporter, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::{
    Resource,
    metrics::{PeriodicReader, SdkMeterProvider},
};
use std::env;
use std::sync::OnceLock;
use std::time::Duration;
use tracing::{debug, info, warn};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

/// Configuration for New Relic OpenTelemetry integration
#[derive(Debug, Clone)]
pub struct NewRelicConfig {
    pub endpoint: String,
    pub api_key: String,
    pub sui_address: String,
}

impl NewRelicConfig {
    /// Create a new New Relic configuration from environment variables
    pub fn from_env() -> Result<Self> {
        let endpoint = env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
            .unwrap_or_else(|_| "https://otlp.eu01.nr-data.net".to_string());

        let api_key = env::var("OTEL_EXPORTER_OTLP_HEADERS")
            .map_err(|_| anyhow!("OTEL_EXPORTER_OTLP_HEADERS environment variable not set"))?
            .strip_prefix("api-key=")
            .ok_or_else(|| anyhow!("OTEL_EXPORTER_OTLP_HEADERS must start with 'api-key='"))?
            .to_string();

        let sui_address = env::var("SUI_ADDRESS")
            .unwrap_or_else(|_| "unknown".to_string());

        Ok(Self {
            endpoint,
            api_key,
            sui_address,
        })
    }

    /// Check if New Relic environment variables are configured
    pub fn is_configured() -> bool {
        env::var("OTEL_EXPORTER_OTLP_HEADERS").is_ok()
    }
}

/// Global New Relic configuration
static NR_CONFIG: OnceLock<Option<NewRelicConfig>> = OnceLock::new();

/// Global SdkLoggerProvider for New Relic
static NR_LOGGER_PROVIDER: OnceLock<Option<SdkLoggerProvider>> = OnceLock::new();

/// Initialize New Relic OpenTelemetry integration
pub async fn init_newrelic() -> Result<()> {
    let config = if NewRelicConfig::is_configured() {
        match NewRelicConfig::from_env() {
            Ok(config) => {
                info!("🚀 New Relic OpenTelemetry integration enabled");
                info!("   Endpoint: {}", config.endpoint);
                info!("   SUI Address: {}", config.sui_address);
                Some(config)
            }
            Err(e) => {
                warn!("Failed to initialize New Relic config: {}", e);
                None
            }
        }
    } else {
        debug!("New Relic environment variables not configured, skipping initialization");
        None
    };

    NR_CONFIG
        .set(config.clone())
        .map_err(|_| anyhow!("Failed to set New Relic configuration"))?;

    if let Some(config) = config {
        // Initialize metrics exporter
        init_metrics_exporter(&config).await?;

        // Initialize logs exporter
        init_logs_exporter(&config).await?;

        info!("✅ New Relic metrics and logs exporters initialized successfully");
    }

    Ok(())
}

/// Initialize the metrics exporter with HTTP/protobuf
async fn init_metrics_exporter(config: &NewRelicConfig) -> Result<()> {
    // Build the OTLP HTTP metric exporter
    let mut headers = std::collections::HashMap::new();
    headers.insert("api-key".to_string(), config.api_key.clone());
    
    let metric_exporter = MetricExporter::builder()
        .with_http()
        .with_endpoint(format!("{}/v1/metrics", config.endpoint))
        .with_headers(headers)
        .with_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| anyhow!("Failed to build metric exporter: {}", e))?;

    // Create a periodic reader for metrics
    let periodic_reader = PeriodicReader::builder(metric_exporter)
        .with_interval(Duration::from_secs(60))
        .build();

    // Create resource with service identification and SUI address
    let resource = Resource::builder()
        .with_service_name("silvana-coordinator")
        .with_attributes(vec![
            KeyValue::new("service.version", "0.1.0"),
            KeyValue::new("host.sui_address", config.sui_address.clone()),
            KeyValue::new("deployment.environment", env::var("ENVIRONMENT").unwrap_or_else(|_| "production".to_string())),
        ])
        .build();

    // Create meter provider with the periodic reader
    let meter_provider = SdkMeterProvider::builder()
        .with_reader(periodic_reader)
        .with_resource(resource)
        .build();

    // Set the global meter provider
    global::set_meter_provider(meter_provider);

    info!("📊 New Relic metrics exporter initialized");
    Ok(())
}

/// Initialize the logs exporter with HTTP/protobuf
async fn init_logs_exporter(config: &NewRelicConfig) -> Result<()> {
    // Build OTLP HTTP exporter
    let mut headers = std::collections::HashMap::new();
    headers.insert("api-key".to_string(), config.api_key.clone());
    
    let log_exporter = LogExporter::builder()
        .with_http()
        .with_endpoint(format!("{}/v1/logs", config.endpoint))
        .with_headers(headers)
        .with_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| anyhow!("Failed to build log exporter: {}", e))?;

    // Resource describing this service
    let resource = Resource::builder()
        .with_service_name("silvana-coordinator")
        .with_attributes(vec![
            KeyValue::new("service.version", "0.1.0"),
            KeyValue::new("host.sui_address", config.sui_address.clone()),
            KeyValue::new("deployment.environment", env::var("ENVIRONMENT").unwrap_or_else(|_| "production".to_string())),
        ])
        .build();

    // Logger provider with async batch processor
    let logger_provider = SdkLoggerProvider::builder()
        .with_batch_exporter(log_exporter)
        .with_resource(resource)
        .build();

    // Store in global OnceLock for later retrieval
    NR_LOGGER_PROVIDER.set(Some(logger_provider)).ok();

    info!("📝 New Relic logs exporter initialized");
    Ok(())
}

/// Send a log message to New Relic
pub async fn send_log_to_newrelic(level: &str, message: &str) -> Result<()> {
    if !NewRelicConfig::is_configured() {
        return Ok(());
    }

    // Get the provider stored in init_logs_exporter
    let provider = match NR_LOGGER_PROVIDER.get() {
        Some(Some(p)) => p,
        _ => return Ok(()),
    };
    let logger = provider.logger("silvana-coordinator");

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
    
    let sev_text: &'static str = match level.to_lowercase().as_str() {
        "error" | "err" => "ERROR",
        "warn" | "warning" => "WARN",
        "info" => "INFO",
        "debug" => "DEBUG",
        "trace" => "TRACE",
        _ => "INFO",
    };
    record.set_severity_text(sev_text);

    // Add SUI address as an attribute to the log
    if let Some(config) = NR_CONFIG.get().and_then(|c| c.as_ref()) {
        record.add_attribute("sui_address", AnyValue::from(config.sui_address.clone()));
    }

    logger.emit(record);

    Ok(())
}

/// Initialize logging with dual outputs: console/file and New Relic for warn/error
pub async fn init_logging_with_newrelic() -> Result<()> {
    // Initialize New Relic if configured
    if NewRelicConfig::is_configured() {
        init_newrelic().await?;
    }

    // Determine log destination from environment
    let log_destination = env::var("LOG_DESTINATION").unwrap_or_else(|_| "console".to_string());
    
    // Base filter for all logs
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    match log_destination.as_str() {
        "file" => {
            // File logging configuration
            let file_appender = tracing_appender::rolling::daily("logs", "coordinator.log");
            let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
            
            let file_layer = tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false)
                .with_target(false)  // Don't show full module paths
                .with_thread_ids(false)  // Don't show thread IDs
                .with_thread_names(false);  // Don't show thread names

            // Initialize subscriber with file layer
            tracing_subscriber::registry()
                .with(env_filter)
                .with(file_layer)
                .with(NewRelicTracingLayer)
                .init();
            
            info!("📝 Logging to file: logs/coordinator.log");
        }
        _ => {
            // Console logging configuration - preserve original format
            let console_layer = tracing_subscriber::fmt::layer()
                .with_ansi(true)
                .with_target(false)  // Don't show full module paths
                .with_thread_ids(false)  // Don't show thread IDs
                .with_thread_names(false);  // Don't show thread names

            // Initialize subscriber with console layer
            tracing_subscriber::registry()
                .with(env_filter)
                .with(console_layer)
                .with(NewRelicTracingLayer)
                .init();
            
            info!("📝 Logging to console");
        }
    }

    if NewRelicConfig::is_configured() {
        info!("🚀 New Relic logging enabled for warn and error levels");
    }

    Ok(())
}

/// Custom tracing layer that sends warn and error logs to New Relic
struct NewRelicTracingLayer;

impl<S> Layer<S> for NewRelicTracingLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if !NewRelicConfig::is_configured() {
            return;
        }

        let metadata = event.metadata();
        let level = metadata.level();
        
        // Only send warn and error logs to New Relic
        if *level <= tracing::Level::WARN {
            // Format the message
            let mut visitor = MessageVisitor::default();
            event.record(&mut visitor);
            
            let message = format!(
                "[{}] {} - {}",
                metadata.target(),
                metadata.name(),
                visitor.message
            );
            
            let level_str = match *level {
                tracing::Level::ERROR => "error",
                tracing::Level::WARN => "warn",
                _ => return,
            };
            
            // Send to New Relic asynchronously
            let message_clone = message.clone();
            tokio::spawn(async move {
                if let Err(e) = send_log_to_newrelic(level_str, &message_clone).await {
                    eprintln!("Failed to send log to New Relic: {}", e);
                }
            });
        }
    }
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
        } else {
            if !self.message.is_empty() {
                self.message.push_str(", ");
            }
            self.message.push_str(&format!("{}={:?}", field.name(), value));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            if !self.message.is_empty() {
                self.message.push_str(", ");
            }
            self.message.push_str(&format!("{}={}", field.name(), value));
        }
    }
}

/// Test function to verify New Relic integration
pub async fn test_newrelic_integration() -> Result<()> {
    if !NewRelicConfig::is_configured() {
        info!("🧪 New Relic not configured, skipping integration test");
        return Ok(());
    }

    info!("🧪 Testing New Relic OpenTelemetry integration...");

    // Test sending logs
    send_log_to_newrelic("info", "New Relic integration test - info log").await?;
    send_log_to_newrelic("warn", "New Relic integration test - warning log").await?;
    send_log_to_newrelic("error", "New Relic integration test - error log").await?;

    // Test sending a metric
    let meter = global::meter("silvana-coordinator");
    let counter = meter.f64_counter("test_counter").build();
    counter.add(1.0, &[]);

    info!("🧪 New Relic integration test completed");
    Ok(())
}