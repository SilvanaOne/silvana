//! # Silvana Monitoring System
//!
//! This crate provides comprehensive monitoring capabilities for the Silvana project,
//! including both application-specific metrics and system-level metrics suitable for
//! BetterStack dashboards and other monitoring solutions.
//!
//! ## Available Metrics
//!
//! ### Buffer Metrics
//! - `silvana_buffer_events_total` - Total events received
//! - `silvana_buffer_events_processed_total` - Total events processed
//! - `silvana_buffer_events_dropped_total` - Total events dropped
//! - `silvana_buffer_events_error_total` - Total processing errors
//! - `silvana_buffer_size_current` - Current buffer size
//! - `silvana_buffer_memory_bytes` - Current memory usage
//! - `silvana_buffer_backpressure_events_total` - Total backpressure events
//! - `silvana_buffer_health_status` - Buffer health (1=healthy, 0=unhealthy)
//! - `silvana_circuit_breaker_status` - Circuit breaker status (1=open, 0=closed)
//!
//! ### gRPC Metrics
//! - `silvana_grpc_requests_total` - Total gRPC requests
//! - `silvana_grpc_request_duration_seconds` - Request duration histogram
//!
//! ### System Metrics
//! - `silvana_cpu_usage_percent` - CPU usage percentage
//! - `silvana_memory_used_bytes` - Memory used in bytes
//! - `silvana_memory_total_bytes` - Total memory in bytes
//! - `silvana_memory_available_bytes` - Available memory in bytes
//! - `silvana_load_average_1min` - 1-minute load average
//! - `silvana_load_average_5min` - 5-minute load average
//! - `silvana_load_average_15min` - 15-minute load average
//! - `silvana_disk_read_bytes_total` - Total disk read bytes
//! - `silvana_disk_write_bytes_total` - Total disk write bytes
//! - `silvana_network_received_bytes_total` - Total network received bytes
//! - `silvana_network_transmitted_bytes_total` - Total network transmitted bytes
//! - `silvana_process_cpu_percent` - Process CPU usage percentage
//! - `silvana_process_memory_bytes` - Process memory usage in bytes
//! - `silvana_process_virtual_memory_bytes` - Process virtual memory usage in bytes
//! - `silvana_process_threads_count` - Process thread count
//! - `silvana_uptime_seconds` - System uptime in seconds
//!
//! ## BetterStack Integration
//!
//! These metrics are designed to work seamlessly with BetterStack dashboards:
//! - CPU and memory metrics for resource monitoring
//! - Load averages for system health
//! - Network and disk I/O for performance analysis
//! - Process-specific metrics for application monitoring
//! - Buffer and gRPC metrics for service-specific insights
//!
//! ## OpenTelemetry Integration
//!
//! The monitoring crate now includes OpenTelemetry support for sending logs, metrics,
//! and traces to BetterStack when the appropriate environment variables are configured:
//! - `OPENTELEMETRY_INGESTING_HOST` - BetterStack ingesting host
//! - `OPENTELEMETRY_SOURCE_ID` - Source identifier for logs/metrics/traces
//! - `OPENTELEMETRY_SOURCE_TOKEN` - Authentication token for BetterStack
//!
//! ## Usage
//!
//! Initialize monitoring and logging:
//! ```rust,no_run
//! use monitoring::{init_monitoring, init_logging, spawn_monitoring_tasks};
//! use monitoring::opentelemetry::init_opentelemetry;
//! use anyhow::Result;
//! use buffer::EventBuffer;
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Initialize logging with daily rotating files
//!     init_logging().await?;
//!
//!     // Initialize monitoring system
//!     init_monitoring()?;
//!
//!     // Initialize OpenTelemetry (optional, based on environment variables)
//!     init_opentelemetry().await?;
//!
//!     // Create an event buffer (this is just an example)
//!     // let event_buffer = EventBuffer::new(/* your backend */);
//!
//!     // Start all monitoring tasks
//!     // spawn_monitoring_tasks(event_buffer);
//!
//!     Ok(())
//! }
//! ```

// Module declarations
pub mod coordinator_metrics;
pub mod logging;
pub mod metrics;
pub mod newrelic;
pub mod opentelemetry;

// Re-export the BufferableEvent trait for convenience
pub use buffer::BufferableEvent;

// Re-export core monitoring functionality
pub use metrics::{
    create_metrics_server, init_monitoring, record_grpc_request, spawn_monitoring_tasks,
    start_metrics_server, update_buffer_metrics, update_system_metrics,
};

// Re-export logging functionality
pub use logging::init_logging;

// Re-export OpenTelemetry functionality
pub use opentelemetry::{
    OpenTelemetryConfig, init_opentelemetry, send_buffer_metrics, send_log, send_metric,
    send_system_metrics, system_metrics_exporter, test_integration,
};

// Re-export New Relic functionality
pub use newrelic::{
    NewRelicConfig, init_newrelic, init_logging_with_newrelic, 
    send_log_to_newrelic, test_newrelic_integration,
};
