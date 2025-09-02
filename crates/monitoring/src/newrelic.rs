//! New Relic integration for OpenTelemetry
//!
//! This module provides OpenTelemetry support for sending logs, metrics, and traces
//! to New Relic using the OTLP (OpenTelemetry Protocol) endpoint.

use anyhow::{Result, anyhow};
use opentelemetry::KeyValue;
use opentelemetry::global;
use opentelemetry::logs::LoggerProvider;
use opentelemetry::logs::{AnyValue, LogRecord, Logger, Severity};
use opentelemetry::trace::{Span, Tracer};
use opentelemetry_otlp::{
    LogExporter, MetricExporter, SpanExporter, WithExportConfig, WithHttpConfig,
};
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::{
    Resource,
    metrics::{PeriodicReader, SdkMeterProvider},
};
use std::env;
use std::sync::OnceLock;
use std::time::Duration;
use tracing::{debug, error, info, warn};
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

        let sui_address = env::var("SUI_ADDRESS").unwrap_or_else(|_| "unknown".to_string());

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

        // Initialize traces exporter for APM
        init_traces_exporter(&config).await?;

        info!("✅ New Relic metrics, logs, and traces exporters initialized successfully");
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
    // Following OpenTelemetry semantic conventions for APM
    let resource = Resource::builder()
        .with_service_name("silvana-coordinator")
        .with_attributes(vec![
            // Required service attributes for APM
            KeyValue::new("service.name", "silvana-coordinator"),
            KeyValue::new("service.version", "0.1.0"),
            KeyValue::new("service.instance.id", config.sui_address.clone()),
            // Additional service attributes
            KeyValue::new("service.namespace", "silvana"),
            KeyValue::new(
                "deployment.environment",
                env::var("ENVIRONMENT").unwrap_or_else(|_| "production".to_string()),
            ),
            // Host attributes
            KeyValue::new("host.name", config.sui_address.clone()),
            KeyValue::new("host.type", "coordinator"),
            // Custom attributes
            KeyValue::new("sui.address", config.sui_address.clone()),
            KeyValue::new(
                "sui.network",
                env::var("SUI_CHAIN").unwrap_or_else(|_| "mainnet".to_string()),
            ),
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

/// Initialize the traces exporter with HTTP/protobuf for APM
async fn init_traces_exporter(config: &NewRelicConfig) -> Result<()> {
    // Build the OTLP HTTP trace exporter
    let mut headers = std::collections::HashMap::new();
    headers.insert("api-key".to_string(), config.api_key.clone());

    let trace_exporter = SpanExporter::builder()
        .with_http()
        .with_endpoint(format!("{}/v1/traces", config.endpoint))
        .with_headers(headers)
        .with_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| anyhow!("Failed to build trace exporter: {}", e))?;

    // Create resource with service identification - matching metrics and logs
    let resource = Resource::builder()
        .with_service_name("silvana-coordinator")
        .with_attributes(vec![
            // Required service attributes for APM
            KeyValue::new("service.name", "silvana-coordinator"),
            KeyValue::new("service.version", "0.1.0"),
            KeyValue::new("service.instance.id", config.sui_address.clone()),
            // Additional service attributes
            KeyValue::new("service.namespace", "silvana"),
            KeyValue::new(
                "deployment.environment",
                env::var("ENVIRONMENT").unwrap_or_else(|_| "production".to_string()),
            ),
            // Host attributes
            KeyValue::new("host.name", config.sui_address.clone()),
            KeyValue::new("host.type", "coordinator"),
            // Custom attributes
            KeyValue::new("sui.address", config.sui_address.clone()),
            KeyValue::new(
                "sui.network",
                env::var("SUI_CHAIN").unwrap_or_else(|_| "mainnet".to_string()),
            ),
        ])
        .build();

    // Create tracer provider with batch processor
    use opentelemetry_sdk::trace::SdkTracerProvider;
    let tracer_provider = SdkTracerProvider::builder()
        .with_simple_exporter(trace_exporter)
        .with_resource(resource)
        .build();

    // Set global tracer provider
    global::set_tracer_provider(tracer_provider);

    info!("📊 New Relic traces exporter initialized for APM");
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

    // Resource describing this service - matching metrics resource
    let resource = Resource::builder()
        .with_service_name("silvana-coordinator")
        .with_attributes(vec![
            // Required service attributes for APM
            KeyValue::new("service.name", "silvana-coordinator"),
            KeyValue::new("service.version", "0.1.0"),
            KeyValue::new("service.instance.id", config.sui_address.clone()),
            // Additional service attributes
            KeyValue::new("service.namespace", "silvana"),
            KeyValue::new(
                "deployment.environment",
                env::var("ENVIRONMENT").unwrap_or_else(|_| "production".to_string()),
            ),
            // Host attributes
            KeyValue::new("host.name", config.sui_address.clone()),
            KeyValue::new("host.type", "coordinator"),
            // Custom attributes
            KeyValue::new("sui.address", config.sui_address.clone()),
            KeyValue::new(
                "sui.network",
                env::var("SUI_CHAIN").unwrap_or_else(|_| "mainnet".to_string()),
            ),
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

/// Custom tracing layer that sends warn and error logs to New Relic
struct NewRelicTracingLayer;

impl NewRelicTracingLayer {
    fn new() -> Self {
        NewRelicTracingLayer
    }
}

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

            // Create a cleaner message without redundant metadata
            let message = if visitor.message.is_empty() {
                format!("[{}]", metadata.target())
            } else {
                visitor.message.clone()
            };

            let level_str = match *level {
                tracing::Level::ERROR => "error",
                tracing::Level::WARN => "warn",
                _ => return,
            };

            // Get the logger provider if it's initialized
            if let Some(provider) = NR_LOGGER_PROVIDER.get().and_then(|p| p.as_ref()) {
                let logger = provider.logger("silvana-coordinator");

                let sev = match level_str {
                    "error" | "err" => Severity::Error,
                    "warn" | "warning" => Severity::Warn,
                    _ => return,
                };

                let mut record = logger.create_log_record();
                record.set_severity_number(sev);
                record.set_body(AnyValue::from(message.clone()));

                let sev_text: &'static str = match level_str {
                    "error" | "err" => "ERROR",
                    "warn" | "warning" => "WARN",
                    _ => return,
                };
                record.set_severity_text(sev_text);

                // Add attributes
                record.add_attribute("log.source", AnyValue::from(metadata.target().to_string()));
                record.add_attribute(
                    "log.module",
                    AnyValue::from(metadata.module_path().unwrap_or("unknown").to_string()),
                );

                // Add SUI address as an attribute
                if let Some(config) = NR_CONFIG.get().and_then(|c| c.as_ref()) {
                    record.add_attribute("sui_address", AnyValue::from(config.sui_address.clone()));
                }

                // Emit the log record
                logger.emit(record);
            }
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
            self.message
                .push_str(&format!("{}={:?}", field.name(), value));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            if !self.message.is_empty() {
                self.message.push_str(", ");
            }
            self.message
                .push_str(&format!("{}={}", field.name(), value));
        }
    }
}

/// Send APM-specific metrics that New Relic expects
pub async fn send_apm_metrics() {
    if !NewRelicConfig::is_configured() {
        return;
    }

    let meter = global::meter("silvana-coordinator");

    // HTTP server metrics (required for APM)
    let request_counter = meter.u64_counter("http.server.request.count").build();
    request_counter.add(
        1,
        &[
            KeyValue::new("http.method", "POST"),
            KeyValue::new("http.scheme", "grpc"),
            KeyValue::new("http.status_code", 200),
        ],
    );

    // Process metrics
    let cpu_gauge = meter.f64_gauge("process.cpu.utilization").build();
    cpu_gauge.record(0.5, &[]);

    let memory_gauge = meter.u64_gauge("process.memory.usage").build();
    memory_gauge.record(1024 * 1024 * 100, &[]); // 100MB

    // Custom business metrics
    let jobs_counter = meter.u64_counter("silvana.jobs.processed").build();
    jobs_counter.add(
        1,
        &[
            KeyValue::new("job.type", "proof"),
            KeyValue::new("job.status", "success"),
        ],
    );
}

/// Force flush all OpenTelemetry data to New Relic
pub async fn flush_telemetry() {
    // Flush logs
    if let Some(provider) = NR_LOGGER_PROVIDER.get().and_then(|p| p.as_ref()) {
        if let Err(e) = provider.force_flush() {
            debug!("Failed to flush logs: {:?}", e);
        }
    }

    // Flush metrics (global meter provider handles this)
    // Metrics are sent periodically every 60 seconds

    // Note: Traces don't have a force_flush method in the current OpenTelemetry version
    // They're flushed automatically when spans end
}

/// Initialize logging with New Relic tracing layer
/// This function sets up the tracing subscriber with the NewRelicTracingLayer
/// to capture warn/error logs and send them to New Relic
pub async fn init_logging_with_newrelic() -> Result<()> {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

    let log_destination = std::env::var("LOG_DESTINATION").unwrap_or_else(|_| "file".to_string());

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        warn!("Failed to parse RUST_LOG environment variable, defaulting to 'info' level");
        "info".into()
    });

    match log_destination.to_lowercase().as_str() {
        "console" => {
            // Console logging - output to stdout
            let subscriber = tracing_subscriber::registry().with(env_filter).with(
                tracing_subscriber::fmt::layer()
                    .with_writer(std::io::stdout)
                    .with_ansi(true)
                    .with_target(true)
                    .compact()
                    .with_line_number(false)
                    .with_file(false)
                    .with_thread_ids(false)
                    .with_thread_names(false),
            );

            // Add New Relic layer if configured
            if NewRelicConfig::is_configured() {
                subscriber.with(NewRelicTracingLayer::new()).init();
                info!("📺 Logging to console with New Relic export enabled");
            } else {
                subscriber.init();
                info!("📺 Logging to console (stdout)");
            }
        }
        _ => {
            // File logging (default) - daily rotating files
            let log_dir = std::env::var("LOG_DIR").unwrap_or_else(|_| "./logs".to_string());
            let log_file_prefix =
                std::env::var("LOG_FILE_PREFIX").unwrap_or_else(|_| "silvana".to_string());

            // Create directory
            std::fs::create_dir_all(&log_dir)?;

            // Create file appender
            let file_appender = tracing_appender::rolling::daily(&log_dir, &log_file_prefix);
            let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

            let subscriber = tracing_subscriber::registry().with(env_filter).with(
                tracing_subscriber::fmt::layer()
                    .with_writer(non_blocking)
                    .with_ansi(false)
                    .with_target(true),
            );

            // Add New Relic layer if configured
            if NewRelicConfig::is_configured() {
                subscriber.with(NewRelicTracingLayer::new()).init();
                info!("📝 Logging to files with New Relic export enabled");
            } else {
                subscriber.init();
                info!("📝 Logging to daily rotating files in: {}/", log_dir);
            }

            // Keep the guard alive
            std::mem::forget(_guard);
        }
    }

    // Report New Relic configuration status
    if NewRelicConfig::is_configured() {
        match NewRelicConfig::from_env() {
            Ok(config) => {
                info!("🔗 New Relic: ✅ CONFIGURED");
                info!("📡 Endpoint: {}", config.endpoint);
                info!("🏷️  API Key: ***");
                info!("📤 Logs will be sent to New Relic (WARN level and above)");
            }
            Err(e) => {
                warn!("🔗 New Relic: ⚠️  CONFIGURATION ERROR - {}", e);
            }
        }
    } else {
        info!("🔗 New Relic: ❌ NOT CONFIGURED");
        info!("   Set NEW_RELIC_API_KEY and NEW_RELIC_OTLP_ENDPOINT to enable");
    }

    Ok(())
}

/// Test function to verify New Relic integration
pub async fn test_newrelic_integration() -> Result<()> {
    if !NewRelicConfig::is_configured() {
        info!("🧪 New Relic not configured, skipping integration test");
        return Ok(());
    }

    info!("🧪 Testing New Relic OpenTelemetry integration...");

    // Test sending logs directly
    send_log_to_newrelic("info", "New Relic integration test - info log").await?;
    send_log_to_newrelic("warn", "New Relic integration test - warning log").await?;
    send_log_to_newrelic("error", "New Relic integration test - error log").await?;

    // Test sending logs through tracing
    warn!("New Relic integration test - warning through tracing");
    error!("New Relic integration test - error through tracing");

    // Test sending metrics
    let meter = global::meter("silvana-coordinator");
    let counter = meter.f64_counter("test_counter").build();
    counter.add(1.0, &[]);

    // Send APM metrics
    send_apm_metrics().await;

    // Test sending a trace span
    let tracer = global::tracer("silvana-coordinator");
    let mut span = tracer.start("test_span");
    span.set_attribute(KeyValue::new("test.attribute", "test_value"));
    span.end();

    // Force flush to ensure test data is sent
    flush_telemetry().await;

    // Give it a moment to send
    tokio::time::sleep(Duration::from_secs(2)).await;

    info!("🧪 New Relic integration test completed");
    Ok(())
}
