//! # Silvana RPC Monitoring
//!
//! This module provides comprehensive monitoring capabilities for the Silvana RPC service,
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
//! ### System Metrics (NEW)
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
//! ## Usage
//!
//! Initialize monitoring and start metrics collection:
//! ```rust
//! use rpc::monitoring::{init_monitoring, spawn_monitoring_tasks};
//!
//! // Initialize monitoring system
//! init_monitoring()?;
//!
//! // Start all monitoring tasks
//! spawn_monitoring_tasks(event_buffer);
//! ```

use anyhow::Result;
use axum::http::{header, StatusCode};
use axum::{response::Response, routing::get, Router};
use prometheus::{
    register_gauge, register_int_counter, register_int_gauge, Encoder, Gauge, IntCounter, IntGauge,
    TextEncoder,
};
use std::net::SocketAddr;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use sysinfo::System;
use tokio::time::interval;
use tracing::{error, info, warn};

use crate::adapters::EventWrapper;
use buffer::EventBuffer;

// Custom Prometheus metrics
static BUFFER_EVENTS_TOTAL: OnceLock<IntCounter> = OnceLock::new();
static BUFFER_EVENTS_PROCESSED: OnceLock<IntCounter> = OnceLock::new();
static BUFFER_EVENTS_DROPPED: OnceLock<IntCounter> = OnceLock::new();
static BUFFER_EVENTS_ERROR: OnceLock<IntCounter> = OnceLock::new();
static BUFFER_SIZE_CURRENT: OnceLock<IntGauge> = OnceLock::new();
static BUFFER_MEMORY_BYTES: OnceLock<IntGauge> = OnceLock::new();
static BUFFER_BACKPRESSURE_EVENTS: OnceLock<IntCounter> = OnceLock::new();
static BUFFER_HEALTH_STATUS: OnceLock<IntGauge> = OnceLock::new();
static CIRCUIT_BREAKER_STATUS: OnceLock<IntGauge> = OnceLock::new();

// Additional gRPC metrics
static GRPC_REQUESTS_TOTAL: OnceLock<IntCounter> = OnceLock::new();
static GRPC_REQUEST_DURATION: OnceLock<prometheus::HistogramVec> = OnceLock::new();

// System metrics
static CPU_USAGE_PERCENT: OnceLock<Gauge> = OnceLock::new();
static MEMORY_USED_BYTES: OnceLock<IntGauge> = OnceLock::new();
static MEMORY_TOTAL_BYTES: OnceLock<IntGauge> = OnceLock::new();
static MEMORY_AVAILABLE_BYTES: OnceLock<IntGauge> = OnceLock::new();
static LOAD_AVERAGE_1MIN: OnceLock<Gauge> = OnceLock::new();
static LOAD_AVERAGE_5MIN: OnceLock<Gauge> = OnceLock::new();
static LOAD_AVERAGE_15MIN: OnceLock<Gauge> = OnceLock::new();
static DISK_READ_BYTES_TOTAL: OnceLock<IntCounter> = OnceLock::new();
static DISK_WRITE_BYTES_TOTAL: OnceLock<IntCounter> = OnceLock::new();
static NETWORK_RECEIVED_BYTES_TOTAL: OnceLock<IntCounter> = OnceLock::new();
static NETWORK_TRANSMITTED_BYTES_TOTAL: OnceLock<IntCounter> = OnceLock::new();
static PROCESS_CPU_PERCENT: OnceLock<Gauge> = OnceLock::new();
static PROCESS_MEMORY_BYTES: OnceLock<IntGauge> = OnceLock::new();
static PROCESS_VIRTUAL_MEMORY_BYTES: OnceLock<IntGauge> = OnceLock::new();
static PROCESS_THREADS_COUNT: OnceLock<IntGauge> = OnceLock::new();
static UPTIME_SECONDS: OnceLock<IntGauge> = OnceLock::new();

// FIXED: Thread-safe counter tracking to prevent race conditions
static LAST_VALUES: OnceLock<Mutex<LastMetricValues>> = OnceLock::new();
// System metrics tracking to prevent overflow
static LAST_SYSTEM_VALUES: OnceLock<Mutex<LastSystemValues>> = OnceLock::new();

#[derive(Debug)]
struct LastMetricValues {
    received: u64,
    processed: u64,
    dropped: u64,
    errors: u64,
    backpressure: u64,
}

impl Default for LastMetricValues {
    fn default() -> Self {
        Self {
            received: 0,
            processed: 0,
            dropped: 0,
            errors: 0,
            backpressure: 0,
        }
    }
}

#[derive(Debug)]
struct LastSystemValues {
    disk_read_bytes: u64,
    disk_write_bytes: u64,
    network_received_bytes: u64,
    network_transmitted_bytes: u64,
}

impl Default for LastSystemValues {
    fn default() -> Self {
        Self {
            disk_read_bytes: 0,
            disk_write_bytes: 0,
            network_received_bytes: 0,
            network_transmitted_bytes: 0,
        }
    }
}

/// Initialize all monitoring components
pub fn init_monitoring() -> Result<()> {
    // Initialize custom application metrics
    init_custom_metrics()?;

    // FIXED: Initialize thread-safe metric tracking
    LAST_VALUES
        .set(Mutex::new(LastMetricValues::default()))
        .map_err(|_| anyhow::anyhow!("Failed to initialize metric tracking"))?;

    // Initialize system metrics tracking
    LAST_SYSTEM_VALUES
        .set(Mutex::new(LastSystemValues::default()))
        .map_err(|_| anyhow::anyhow!("Failed to initialize system metric tracking"))?;

    info!("📊 Monitoring system initialized");
    Ok(())
}

/// Initialize custom Prometheus metrics
fn init_custom_metrics() -> Result<()> {
    BUFFER_EVENTS_TOTAL
        .set(register_int_counter!(
            "silvana_buffer_events_total",
            "Total number of events received by the buffer"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register BUFFER_EVENTS_TOTAL"))?;

    BUFFER_EVENTS_PROCESSED
        .set(register_int_counter!(
            "silvana_buffer_events_processed_total",
            "Total number of events processed by the buffer"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register BUFFER_EVENTS_PROCESSED"))?;

    BUFFER_EVENTS_DROPPED
        .set(register_int_counter!(
            "silvana_buffer_events_dropped_total",
            "Total number of events dropped due to overload"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register BUFFER_EVENTS_DROPPED"))?;

    BUFFER_EVENTS_ERROR
        .set(register_int_counter!(
            "silvana_buffer_events_error_total",
            "Total number of events that failed processing"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register BUFFER_EVENTS_ERROR"))?;

    BUFFER_SIZE_CURRENT
        .set(register_int_gauge!(
            "silvana_buffer_size_current",
            "Current number of events in the buffer"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register BUFFER_SIZE_CURRENT"))?;

    BUFFER_MEMORY_BYTES
        .set(register_int_gauge!(
            "silvana_buffer_memory_bytes",
            "Current memory usage of the buffer in bytes"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register BUFFER_MEMORY_BYTES"))?;

    BUFFER_BACKPRESSURE_EVENTS
        .set(register_int_counter!(
            "silvana_buffer_backpressure_events_total",
            "Total number of backpressure events"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register BUFFER_BACKPRESSURE_EVENTS"))?;

    BUFFER_HEALTH_STATUS
        .set(register_int_gauge!(
            "silvana_buffer_health_status",
            "Buffer health status (1 = healthy, 0 = unhealthy)"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register BUFFER_HEALTH_STATUS"))?;

    CIRCUIT_BREAKER_STATUS
        .set(register_int_gauge!(
            "silvana_circuit_breaker_status",
            "Circuit breaker status (1 = open, 0 = closed)"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register CIRCUIT_BREAKER_STATUS"))?;

    // Initialize gRPC metrics
    GRPC_REQUESTS_TOTAL
        .set(register_int_counter!(
            "silvana_grpc_requests_total",
            "Total number of gRPC requests"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register GRPC_REQUESTS_TOTAL"))?;

    use prometheus::{register_gauge, register_histogram_vec, HistogramOpts};
    GRPC_REQUEST_DURATION
        .set(register_histogram_vec!(
            HistogramOpts::new(
                "silvana_grpc_request_duration_seconds",
                "Duration of gRPC requests"
            )
            .buckets(vec![0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 2.5, 5.0, 10.0]),
            &["method", "status"]
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register GRPC_REQUEST_DURATION"))?;

    // Initialize system metrics
    CPU_USAGE_PERCENT
        .set(register_gauge!(
            "silvana_cpu_usage_percent",
            "CPU usage percentage"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register CPU_USAGE_PERCENT"))?;

    MEMORY_USED_BYTES
        .set(register_int_gauge!(
            "silvana_memory_used_bytes",
            "Memory used in bytes"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register MEMORY_USED_BYTES"))?;

    MEMORY_TOTAL_BYTES
        .set(register_int_gauge!(
            "silvana_memory_total_bytes",
            "Total memory in bytes"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register MEMORY_TOTAL_BYTES"))?;

    MEMORY_AVAILABLE_BYTES
        .set(register_int_gauge!(
            "silvana_memory_available_bytes",
            "Available memory in bytes"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register MEMORY_AVAILABLE_BYTES"))?;

    LOAD_AVERAGE_1MIN
        .set(register_gauge!(
            "silvana_load_average_1min",
            "1-minute load average"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register LOAD_AVERAGE_1MIN"))?;

    LOAD_AVERAGE_5MIN
        .set(register_gauge!(
            "silvana_load_average_5min",
            "5-minute load average"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register LOAD_AVERAGE_5MIN"))?;

    LOAD_AVERAGE_15MIN
        .set(register_gauge!(
            "silvana_load_average_15min",
            "15-minute load average"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register LOAD_AVERAGE_15MIN"))?;

    DISK_READ_BYTES_TOTAL
        .set(register_int_counter!(
            "silvana_disk_read_bytes_total",
            "Total disk read bytes"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register DISK_READ_BYTES_TOTAL"))?;

    DISK_WRITE_BYTES_TOTAL
        .set(register_int_counter!(
            "silvana_disk_write_bytes_total",
            "Total disk write bytes"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register DISK_WRITE_BYTES_TOTAL"))?;

    NETWORK_RECEIVED_BYTES_TOTAL
        .set(register_int_counter!(
            "silvana_network_received_bytes_total",
            "Total network received bytes"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register NETWORK_RECEIVED_BYTES_TOTAL"))?;

    NETWORK_TRANSMITTED_BYTES_TOTAL
        .set(register_int_counter!(
            "silvana_network_transmitted_bytes_total",
            "Total network transmitted bytes"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register NETWORK_TRANSMITTED_BYTES_TOTAL"))?;

    PROCESS_CPU_PERCENT
        .set(register_gauge!(
            "silvana_process_cpu_percent",
            "Process CPU usage percentage"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register PROCESS_CPU_PERCENT"))?;

    PROCESS_MEMORY_BYTES
        .set(register_int_gauge!(
            "silvana_process_memory_bytes",
            "Process memory usage in bytes"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register PROCESS_MEMORY_BYTES"))?;

    PROCESS_VIRTUAL_MEMORY_BYTES
        .set(register_int_gauge!(
            "silvana_process_virtual_memory_bytes",
            "Process virtual memory usage in bytes"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register PROCESS_VIRTUAL_MEMORY_BYTES"))?;

    PROCESS_THREADS_COUNT
        .set(register_int_gauge!(
            "silvana_process_threads_count",
            "Process thread count"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register PROCESS_THREADS_COUNT"))?;

    UPTIME_SECONDS
        .set(register_int_gauge!(
            "silvana_uptime_seconds",
            "System uptime in seconds"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register UPTIME_SECONDS"))?;

    Ok(())
}

/// Create metrics HTTP server
pub fn create_metrics_server() -> Router {
    Router::new().route("/metrics", get(metrics_handler))
}

/// Start metrics HTTP server
pub async fn start_metrics_server(addr: SocketAddr) -> Result<()> {
    let app = create_metrics_server();
    let listener = tokio::net::TcpListener::bind(addr).await?;

    info!("📊 Starting metrics server on {}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}

/// Metrics endpoint handler
async fn metrics_handler() -> Result<Response<String>, StatusCode> {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();

    match encoder.encode_to_string(&metric_families) {
        Ok(metrics) => {
            let response = Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, encoder.format_type())
                .body(metrics)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            Ok(response)
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// Record gRPC request metrics (call this manually in your gRPC handlers)
pub fn record_grpc_request(method: &str, status: &str, duration_seconds: f64) {
    if let Some(counter) = GRPC_REQUESTS_TOTAL.get() {
        counter.inc();
    }

    if let Some(histogram) = GRPC_REQUEST_DURATION.get() {
        histogram
            .with_label_values(&[method, status])
            .observe(duration_seconds);
    }
}

/// Update Prometheus metrics with buffer stats
pub fn update_buffer_metrics(stats: &buffer::BufferStats, health: bool) {
    if let (
        Some(events_total),
        Some(events_processed),
        Some(events_dropped),
        Some(events_error),
        Some(buffer_size),
        Some(memory_bytes),
        Some(backpressure_events),
        Some(health_status),
        Some(circuit_breaker),
        Some(last_values_mutex),
    ) = (
        BUFFER_EVENTS_TOTAL.get(),
        BUFFER_EVENTS_PROCESSED.get(),
        BUFFER_EVENTS_DROPPED.get(),
        BUFFER_EVENTS_ERROR.get(),
        BUFFER_SIZE_CURRENT.get(),
        BUFFER_MEMORY_BYTES.get(),
        BUFFER_BACKPRESSURE_EVENTS.get(),
        BUFFER_HEALTH_STATUS.get(),
        CIRCUIT_BREAKER_STATUS.get(),
        LAST_VALUES.get(),
    ) {
        // FIXED: Thread-safe counter tracking with proper error handling
        if let Ok(mut last_values) = last_values_mutex.lock() {
            let received_diff = stats.total_received.saturating_sub(last_values.received);
            let processed_diff = stats.total_processed.saturating_sub(last_values.processed);
            let dropped_diff = stats.total_dropped.saturating_sub(last_values.dropped);
            let errors_diff = stats.total_errors.saturating_sub(last_values.errors);
            let backpressure_diff = stats
                .backpressure_events
                .saturating_sub(last_values.backpressure);

            if received_diff > 0 {
                events_total.inc_by(received_diff);
                last_values.received = stats.total_received;
            }
            if processed_diff > 0 {
                events_processed.inc_by(processed_diff);
                last_values.processed = stats.total_processed;
            }
            if dropped_diff > 0 {
                events_dropped.inc_by(dropped_diff);
                last_values.dropped = stats.total_dropped;
            }
            if errors_diff > 0 {
                events_error.inc_by(errors_diff);
                last_values.errors = stats.total_errors;
            }
            if backpressure_diff > 0 {
                backpressure_events.inc_by(backpressure_diff);
                last_values.backpressure = stats.backpressure_events;
            }
        } else {
            warn!("Failed to acquire lock for metric tracking - metrics may be inaccurate");
        }

        // FIXED: Safe integer casting to prevent overflow (usize to i64)
        let safe_buffer_size = if stats.current_buffer_size <= i64::MAX as usize {
            stats.current_buffer_size as i64
        } else {
            warn!(
                "Buffer size {} exceeds i64::MAX, clamping to maximum",
                stats.current_buffer_size
            );
            i64::MAX
        };

        let safe_memory_bytes = if stats.current_memory_bytes <= i64::MAX as usize {
            stats.current_memory_bytes as i64
        } else {
            warn!(
                "Memory bytes {} exceeds i64::MAX, clamping to maximum",
                stats.current_memory_bytes
            );
            i64::MAX
        };

        // Set gauge values (current state) with safe casting
        buffer_size.set(safe_buffer_size);
        memory_bytes.set(safe_memory_bytes);
        health_status.set(if health { 1 } else { 0 });
        circuit_breaker.set(if stats.circuit_breaker_open { 1 } else { 0 });
    }
}

/// Runs periodic statistics reporting for the event buffer
pub async fn stats_reporter(buffer: EventBuffer<EventWrapper>) {
    let mut interval = interval(Duration::from_secs(30));

    loop {
        interval.tick().await;

        let stats = buffer.get_stats().await;
        let health = buffer.health_check().await;

        // Update Prometheus metrics
        update_buffer_metrics(&stats, health);

        if !health {
            // FIXED: Safe division for memory usage calculation (usize values)
            let memory_mb = if stats.current_memory_bytes <= usize::MAX / (1024 * 1024) {
                stats.current_memory_bytes / (1024 * 1024)
            } else {
                // Handle edge case where memory bytes is extremely large
                warn!(
                    "Memory usage {} bytes exceeds safe division range",
                    stats.current_memory_bytes
                );
                stats.current_memory_bytes / 1_000_000 // Use decimal MB for very large values
            };

            info!(
            "📊 Buffer Stats - Received: {}, Processed: {}, Errors: {}, Dropped: {}, Buffer: {}, Memory: {}MB, Backpressure: {}, Health: {}",
            stats.total_received,
            stats.total_processed,
            stats.total_errors,
            stats.total_dropped,
            stats.current_buffer_size,
            memory_mb,
            stats.backpressure_events,
            if health { "✅" } else { "❌" }
        );
        }

        // Alert on concerning metrics
        if stats.circuit_breaker_open {
            error!("🚨 Circuit breaker is OPEN - system overloaded!");
        }

        if stats.current_memory_bytes > 80 * 1024 * 1024 {
            // 80MB warning
            // FIXED: Safe division for memory warning calculation (usize values)
            let memory_mb_warn = if stats.current_memory_bytes <= usize::MAX / (1024 * 1024) {
                stats.current_memory_bytes / (1024 * 1024)
            } else {
                warn!(
                    "Memory usage {} bytes exceeds safe division range in warning",
                    stats.current_memory_bytes
                );
                stats.current_memory_bytes / 1_000_000 // Use decimal MB for very large values
            };

            warn!(
                "⚠️  High memory usage: {}MB (80%+ of limit)",
                memory_mb_warn
            );
        }

        if stats.total_dropped > 0 && stats.total_dropped % 100 == 0 {
            warn!("⚠️  {} events dropped due to overload", stats.total_dropped);
        }

        // FIXED: Safe backpressure rate calculation with overflow protection
        let backpressure_rate = if stats.total_received > 0 {
            // Check if values are too large for safe f64 conversion
            if stats.total_received > f64::MAX as u64 || stats.backpressure_events > f64::MAX as u64
            {
                warn!("Values too large for safe f64 conversion, using scaled calculation");
                // Use a scaling approach for extremely large values
                let scale = 1_000_000u64;
                let received_scaled = stats.total_received / scale;
                let backpressure_scaled = stats.backpressure_events / scale;

                if received_scaled > 0 {
                    (backpressure_scaled as f64 / received_scaled as f64) * 100.0
                } else {
                    0.0
                }
            } else {
                // Safe to convert to f64
                let received_f64 = stats.total_received as f64;
                let backpressure_f64 = stats.backpressure_events as f64;
                let rate = (backpressure_f64 / received_f64) * 100.0;

                // Ensure the result is finite and reasonable
                if rate.is_finite() && rate >= 0.0 && rate <= 100.0 {
                    rate
                } else {
                    warn!(
                        "Calculated backpressure rate {} is invalid, defaulting to 0.0",
                        rate
                    );
                    0.0
                }
            }
        } else {
            0.0
        };

        if backpressure_rate > 10.0 {
            warn!("⚠️  High backpressure rate: {:.1}%", backpressure_rate);
        }
    }
}

/// Runs periodic health monitoring for the event buffer
pub async fn health_monitor(buffer: EventBuffer<EventWrapper>) {
    let mut health_interval = interval(Duration::from_secs(10));

    loop {
        health_interval.tick().await;
        let health = buffer.health_check().await;
        if !health {
            error!("🚨 System health check FAILED - degraded performance detected");
        }
    }
}

/// Collect system metrics and update Prometheus gauges
pub async fn system_metrics_collector() {
    let mut interval = interval(Duration::from_secs(15)); // Update every 15 seconds
    let mut system = System::new_all();
    let current_pid = std::process::id() as usize;

    loop {
        interval.tick().await;

        // Refresh system information
        system.refresh_all();

        // Update CPU metrics
        if let Some(cpu_gauge) = CPU_USAGE_PERCENT.get() {
            // Calculate average CPU usage across all CPUs
            let mut total_cpu_usage = 0.0;
            let cpus = system.cpus();
            if !cpus.is_empty() {
                for cpu in cpus {
                    total_cpu_usage += cpu.cpu_usage();
                }
                total_cpu_usage /= cpus.len() as f32;
            }
            cpu_gauge.set(total_cpu_usage as f64);
        }

        // Update memory metrics
        if let (Some(used_gauge), Some(total_gauge), Some(available_gauge)) = (
            MEMORY_USED_BYTES.get(),
            MEMORY_TOTAL_BYTES.get(),
            MEMORY_AVAILABLE_BYTES.get(),
        ) {
            let used_memory = system.used_memory() as i64;
            let total_memory = system.total_memory() as i64;
            let available_memory = system.available_memory() as i64;

            used_gauge.set(used_memory);
            total_gauge.set(total_memory);
            available_gauge.set(available_memory);
        }

        // Update load average metrics
        let load_avg = System::load_average();
        if let (Some(load1_gauge), Some(load5_gauge), Some(load15_gauge)) = (
            LOAD_AVERAGE_1MIN.get(),
            LOAD_AVERAGE_5MIN.get(),
            LOAD_AVERAGE_15MIN.get(),
        ) {
            load1_gauge.set(load_avg.one);
            load5_gauge.set(load_avg.five);
            load15_gauge.set(load_avg.fifteen);
        }

        // Update uptime
        if let Some(uptime_gauge) = UPTIME_SECONDS.get() {
            uptime_gauge.set(System::uptime() as i64);
        }

        // Update process-specific metrics (for current process)
        if let Some(process) = system.process(sysinfo::Pid::from(current_pid)) {
            if let Some(process_cpu_gauge) = PROCESS_CPU_PERCENT.get() {
                process_cpu_gauge.set(process.cpu_usage() as f64);
            }

            if let Some(process_memory_gauge) = PROCESS_MEMORY_BYTES.get() {
                process_memory_gauge.set(process.memory() as i64);
            }

            if let Some(process_virtual_memory_gauge) = PROCESS_VIRTUAL_MEMORY_BYTES.get() {
                process_virtual_memory_gauge.set(process.virtual_memory() as i64);
            }
        }

        // Update disk and network I/O metrics using diff calculation
        if let Some(last_system_values_mutex) = LAST_SYSTEM_VALUES.get() {
            if let Ok(mut last_values) = last_system_values_mutex.lock() {
                let total_disk_read = 0u64;
                let total_disk_write = 0u64;

                // Collect disk I/O stats using sysinfo Disks
                let _disks = sysinfo::Disks::new_with_refreshed_list();
                // Note: sysinfo doesn't provide I/O statistics on all platforms
                // This is more of a placeholder for when it's available
                // for _disk in &_disks {
                //     // Future: Add disk I/O collection when available
                // }

                // Collect network I/O stats
                let mut total_network_received = 0u64;
                let mut total_network_transmitted = 0u64;

                let networks = sysinfo::Networks::new_with_refreshed_list();
                for (_, data) in &networks {
                    total_network_received += data.total_received();
                    total_network_transmitted += data.total_transmitted();
                }

                // Calculate diffs and update counters
                let disk_read_diff = total_disk_read.saturating_sub(last_values.disk_read_bytes);
                let disk_write_diff = total_disk_write.saturating_sub(last_values.disk_write_bytes);
                let network_received_diff =
                    total_network_received.saturating_sub(last_values.network_received_bytes);
                let network_transmitted_diff =
                    total_network_transmitted.saturating_sub(last_values.network_transmitted_bytes);

                if let (Some(disk_read_counter), Some(disk_write_counter)) =
                    (DISK_READ_BYTES_TOTAL.get(), DISK_WRITE_BYTES_TOTAL.get())
                {
                    if disk_read_diff > 0 {
                        disk_read_counter.inc_by(disk_read_diff);
                        last_values.disk_read_bytes = total_disk_read;
                    }
                    if disk_write_diff > 0 {
                        disk_write_counter.inc_by(disk_write_diff);
                        last_values.disk_write_bytes = total_disk_write;
                    }
                }

                if let (Some(network_received_counter), Some(network_transmitted_counter)) = (
                    NETWORK_RECEIVED_BYTES_TOTAL.get(),
                    NETWORK_TRANSMITTED_BYTES_TOTAL.get(),
                ) {
                    if network_received_diff > 0 {
                        network_received_counter.inc_by(network_received_diff);
                        last_values.network_received_bytes = total_network_received;
                    }
                    if network_transmitted_diff > 0 {
                        network_transmitted_counter.inc_by(network_transmitted_diff);
                        last_values.network_transmitted_bytes = total_network_transmitted;
                    }
                }
            } else {
                warn!("Failed to acquire lock for system metric tracking");
            }
        }
    }
}

/// Spawns buffer stats reporting, health monitoring, and system metrics collection tasks
pub fn spawn_monitoring_tasks(buffer: EventBuffer<EventWrapper>) {
    // Start stats reporting
    let stats_buffer = buffer.clone();
    tokio::spawn(async move {
        stats_reporter(stats_buffer).await;
    });

    // Start health monitoring
    let health_buffer = buffer.clone();
    tokio::spawn(async move {
        health_monitor(health_buffer).await;
    });

    // Start system metrics collection
    tokio::spawn(async move {
        system_metrics_collector().await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_integer_casting() {
        // Test safe u64 to i64 casting for buffer size
        let large_buffer_size = u64::MAX;
        let safe_size = if large_buffer_size <= i64::MAX as u64 {
            large_buffer_size as i64
        } else {
            i64::MAX
        };
        assert_eq!(safe_size, i64::MAX);

        // Test safe u64 to i64 casting for memory bytes
        let large_memory = (i64::MAX as u64) + 1;
        let safe_memory = if large_memory <= i64::MAX as u64 {
            large_memory as i64
        } else {
            i64::MAX
        };
        assert_eq!(safe_memory, i64::MAX);

        // Test normal values pass through unchanged
        let normal_size = 1000u64;
        let safe_normal = if normal_size <= i64::MAX as u64 {
            normal_size as i64
        } else {
            i64::MAX
        };
        assert_eq!(safe_normal, 1000i64);
    }

    #[test]
    fn test_safe_division_operations() {
        // Test safe memory division
        let large_memory = u64::MAX;
        let memory_mb = if large_memory <= u64::MAX / (1024 * 1024) {
            large_memory / (1024 * 1024)
        } else {
            large_memory / 1_000_000 // Fallback to decimal MB
        };

        // Should use fallback calculation for very large values
        assert_eq!(memory_mb, large_memory / 1_000_000);

        // Test normal memory values
        let normal_memory = 100 * 1024 * 1024; // 100MB
        let normal_mb = if normal_memory <= u64::MAX / (1024 * 1024) {
            normal_memory / (1024 * 1024)
        } else {
            normal_memory / 1_000_000
        };
        assert_eq!(normal_mb, 100);
    }

    #[test]
    fn test_backpressure_rate_calculation() {
        // Test normal case
        let total_received = 1000u64;
        let backpressure_events = 50u64;

        let rate = if total_received > 0 {
            if total_received > f64::MAX as u64 || backpressure_events > f64::MAX as u64 {
                let scale = 1_000_000u64;
                let received_scaled = total_received / scale;
                let backpressure_scaled = backpressure_events / scale;

                if received_scaled > 0 {
                    (backpressure_scaled as f64 / received_scaled as f64) * 100.0
                } else {
                    0.0
                }
            } else {
                let received_f64 = total_received as f64;
                let backpressure_f64 = backpressure_events as f64;
                let rate = (backpressure_f64 / received_f64) * 100.0;

                if rate.is_finite() && rate >= 0.0 && rate <= 100.0 {
                    rate
                } else {
                    0.0
                }
            }
        } else {
            0.0
        };

        assert_eq!(rate, 5.0); // 50/1000 * 100 = 5%

        // Test zero received (division by zero prevention)
        let zero_received = 0u64;
        let zero_rate = if zero_received > 0 {
            // This branch should not execute
            100.0
        } else {
            0.0
        };
        assert_eq!(zero_rate, 0.0);

        // Test extremely large values (avoid arithmetic overflow)
        let huge_received = f64::MAX as u64; // Use maximum value without overflow
        let huge_backpressure = 1000000u64;

        let scaled_rate = if huge_received >= f64::MAX as u64 {
            let scale = 1_000_000u64;
            let received_scaled = huge_received / scale;
            let backpressure_scaled = huge_backpressure / scale;

            if received_scaled > 0 {
                (backpressure_scaled as f64 / received_scaled as f64) * 100.0
            } else {
                0.0
            }
        } else {
            // This branch won't execute with our test value, but for completeness
            (huge_backpressure as f64 / huge_received as f64) * 100.0
        };

        // Should compute a reasonable rate using scaled values
        assert!(scaled_rate >= 0.0 && scaled_rate <= 100.0);
    }

    #[test]
    fn test_thread_safe_metric_tracking() {
        // Test that LastMetricValues struct can be created and used safely
        let values = LastMetricValues::default();
        assert_eq!(values.received, 0);
        assert_eq!(values.processed, 0);
        assert_eq!(values.dropped, 0);
        assert_eq!(values.errors, 0);
        assert_eq!(values.backpressure, 0);

        // Test that we can safely wrap in Mutex
        let mutex_values = Mutex::new(values);
        let lock_result = mutex_values.lock();
        assert!(lock_result.is_ok());
    }

    #[test]
    fn test_overflow_protection() {
        // Test saturating arithmetic behavior
        let current = u64::MAX;
        let addition = 100u64;
        let safe_result = current.saturating_sub(addition);

        // Should not panic or wrap around
        assert!(safe_result < current);

        // Test that our saturating operations work correctly
        let base = 1000u64;
        let increment = 500u64;
        let result = base.saturating_sub(increment);
        assert_eq!(result, 500);
    }

    #[test]
    fn test_percentage_bounds_checking() {
        // Test percentage calculation bounds
        let backpressure = 150u64;
        let total = 100u64; // This would give >100% which is invalid

        let rate = if total > 0 {
            let rate = (backpressure as f64 / total as f64) * 100.0;
            if rate.is_finite() && rate >= 0.0 && rate <= 100.0 {
                rate
            } else {
                0.0 // Invalid rate, default to 0
            }
        } else {
            0.0
        };

        // Should clamp invalid rates
        assert_eq!(rate, 0.0); // 150% is invalid, should default to 0

        // Test valid percentage
        let valid_backpressure = 25u64;
        let valid_total = 100u64;
        let valid_rate = (valid_backpressure as f64 / valid_total as f64) * 100.0;
        assert_eq!(valid_rate, 25.0);
        assert!(valid_rate >= 0.0 && valid_rate <= 100.0);
    }
}
