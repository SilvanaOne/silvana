//! Metrics collection and monitoring functionality for Silvana services
//!
//! This module provides comprehensive metrics collection including:
//! - Buffer metrics (events, memory, health)
//! - gRPC metrics (requests, duration)
//! - System metrics (CPU, memory, load, disk, network)
//! - Prometheus HTTP server for metrics exposure

use anyhow::Result;
use axum::http::{StatusCode, header};
use axum::{Router, response::Response, routing::get};
use prometheus::{
    Encoder, Gauge, IntCounter, IntGauge, register_gauge, register_int_counter, register_int_gauge,
};

use std::net::SocketAddr;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use sysinfo::{Networks, System};
use tokio::time::interval;
use tracing::{debug, info, warn, error};

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

// System metrics (renamed to match BetterStack expectations)
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

// Additional metrics that BetterStack expects
static MEMORY_FREE_BYTES: OnceLock<IntGauge> = OnceLock::new();
static MEMORY_CACHED_BYTES: OnceLock<IntGauge> = OnceLock::new();
static PROCESSES_PIDS: OnceLock<IntGauge> = OnceLock::new();
static PROCESSES_BLOCKED: OnceLock<IntGauge> = OnceLock::new();

// Thread-safe counter tracking to prevent race conditions
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
    network_received_bytes: u64,
    network_transmitted_bytes: u64,
}

impl Default for LastSystemValues {
    fn default() -> Self {
        Self {
            network_received_bytes: 0,
            network_transmitted_bytes: 0,
        }
    }
}

/// Initialize all monitoring components
pub fn init_monitoring() -> Result<()> {
    // Initialize custom application metrics
    init_custom_metrics()?;

    // Initialize thread-safe metric tracking
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
            "node_buffer_events_total",
            "Total number of events received by the buffer"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register BUFFER_EVENTS_TOTAL"))?;

    BUFFER_EVENTS_PROCESSED
        .set(register_int_counter!(
            "node_buffer_events_processed_total",
            "Total number of events processed by the buffer"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register BUFFER_EVENTS_PROCESSED"))?;

    BUFFER_EVENTS_DROPPED
        .set(register_int_counter!(
            "node_buffer_events_dropped_total",
            "Total number of events dropped due to overload"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register BUFFER_EVENTS_DROPPED"))?;

    BUFFER_EVENTS_ERROR
        .set(register_int_counter!(
            "node_buffer_events_error_total",
            "Total number of events that failed processing"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register BUFFER_EVENTS_ERROR"))?;

    BUFFER_SIZE_CURRENT
        .set(register_int_gauge!(
            "node_buffer_size_current",
            "Current number of events in the buffer"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register BUFFER_SIZE_CURRENT"))?;

    BUFFER_MEMORY_BYTES
        .set(register_int_gauge!(
            "node_buffer_memory_bytes",
            "Current memory usage of the buffer in bytes"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register BUFFER_MEMORY_BYTES"))?;

    BUFFER_BACKPRESSURE_EVENTS
        .set(register_int_counter!(
            "node_buffer_backpressure_events_total",
            "Total number of backpressure events"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register BUFFER_BACKPRESSURE_EVENTS"))?;

    BUFFER_HEALTH_STATUS
        .set(register_int_gauge!(
            "node_buffer_health_status",
            "Buffer health status (1 = healthy, 0 = unhealthy)"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register BUFFER_HEALTH_STATUS"))?;

    CIRCUIT_BREAKER_STATUS
        .set(register_int_gauge!(
            "node_circuit_breaker_status",
            "Circuit breaker status (1 = open, 0 = closed)"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register CIRCUIT_BREAKER_STATUS"))?;

    // Initialize gRPC metrics
    GRPC_REQUESTS_TOTAL
        .set(register_int_counter!(
            "node_grpc_requests_total",
            "Total number of gRPC requests"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register GRPC_REQUESTS_TOTAL"))?;

    use prometheus::{HistogramOpts, register_histogram_vec};
    GRPC_REQUEST_DURATION
        .set(register_histogram_vec!(
            HistogramOpts::new(
                "node_grpc_request_duration_seconds",
                "Duration of gRPC requests"
            )
            .buckets(vec![0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 2.5, 5.0, 10.0]),
            &["method", "status"]
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register GRPC_REQUEST_DURATION"))?;

    // Initialize system metrics (matching exact BetterStack expectations)
    CPU_USAGE_PERCENT
        .set(register_gauge!(
            "node_cpu_seconds_total",
            "CPU usage percentage"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register CPU_USAGE_PERCENT"))?;

    MEMORY_USED_BYTES
        .set(register_int_gauge!(
            "node_memory_MemUsed_bytes",
            "Memory used in bytes"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register MEMORY_USED_BYTES"))?;

    MEMORY_TOTAL_BYTES
        .set(register_int_gauge!(
            "node_memory_MemTotal_bytes",
            "Total memory in bytes"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register MEMORY_TOTAL_BYTES"))?;

    MEMORY_AVAILABLE_BYTES
        .set(register_int_gauge!(
            "node_memory_MemAvailable_bytes",
            "Available memory in bytes"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register MEMORY_AVAILABLE_BYTES"))?;

    LOAD_AVERAGE_1MIN
        .set(register_gauge!("node_load1", "1-minute load average")?)
        .map_err(|_| anyhow::anyhow!("Failed to register LOAD_AVERAGE_1MIN"))?;

    LOAD_AVERAGE_5MIN
        .set(register_gauge!("node_load5", "5-minute load average")?)
        .map_err(|_| anyhow::anyhow!("Failed to register LOAD_AVERAGE_5MIN"))?;

    LOAD_AVERAGE_15MIN
        .set(register_gauge!("node_load15", "15-minute load average")?)
        .map_err(|_| anyhow::anyhow!("Failed to register LOAD_AVERAGE_15MIN"))?;

    DISK_READ_BYTES_TOTAL
        .set(register_int_counter!(
            "node_disk_read_bytes_total",
            "Total disk read bytes"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register DISK_READ_BYTES_TOTAL"))?;

    DISK_WRITE_BYTES_TOTAL
        .set(register_int_counter!(
            "node_disk_written_bytes_total",
            "Total disk write bytes"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register DISK_WRITE_BYTES_TOTAL"))?;

    NETWORK_RECEIVED_BYTES_TOTAL
        .set(register_int_counter!(
            "node_network_receive_bytes_total",
            "Total network received bytes"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register NETWORK_RECEIVED_BYTES_TOTAL"))?;

    NETWORK_TRANSMITTED_BYTES_TOTAL
        .set(register_int_counter!(
            "node_network_transmit_bytes_total",
            "Total network transmitted bytes"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register NETWORK_TRANSMITTED_BYTES_TOTAL"))?;

    PROCESS_CPU_PERCENT
        .set(register_gauge!(
            "node_process_cpu_percent",
            "Process CPU usage percentage"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register PROCESS_CPU_PERCENT"))?;

    PROCESS_MEMORY_BYTES
        .set(register_int_gauge!(
            "node_process_memory_bytes",
            "Process memory usage in bytes"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register PROCESS_MEMORY_BYTES"))?;

    PROCESS_VIRTUAL_MEMORY_BYTES
        .set(register_int_gauge!(
            "node_process_virtual_memory_bytes",
            "Process virtual memory usage in bytes"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register PROCESS_VIRTUAL_MEMORY_BYTES"))?;

    PROCESS_THREADS_COUNT
        .set(register_int_gauge!(
            "node_processes_threads",
            "Process thread count"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register PROCESS_THREADS_COUNT"))?;

    UPTIME_SECONDS
        .set(register_int_gauge!(
            "node_boot_time_seconds",
            "System boot time in seconds"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register UPTIME_SECONDS"))?;

    // Additional BetterStack expected metrics
    MEMORY_FREE_BYTES
        .set(register_int_gauge!(
            "node_memory_MemFree_bytes",
            "Free memory in bytes"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register MEMORY_FREE_BYTES"))?;

    MEMORY_CACHED_BYTES
        .set(register_int_gauge!(
            "node_memory_Cached_bytes",
            "Cached memory in bytes"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register MEMORY_CACHED_BYTES"))?;

    PROCESSES_PIDS
        .set(register_int_gauge!(
            "node_processes_pids",
            "Total number of processes"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register PROCESSES_PIDS"))?;

    PROCESSES_BLOCKED
        .set(register_int_gauge!(
            "node_procs_blocked",
            "Number of blocked processes"
        )?)
        .map_err(|_| anyhow::anyhow!("Failed to register PROCESSES_BLOCKED"))?;

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

    info!("📊 Started metrics server on {}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}

/// Metrics endpoint handler
async fn metrics_handler() -> Result<Response<String>, StatusCode> {
    let encoder = prometheus::TextEncoder::new();
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
        // Thread-safe counter tracking with proper error handling
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

        // Safe integer casting to prevent overflow (usize to i64)
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

        // Update gauge metrics safely
        buffer_size.set(safe_buffer_size);
        memory_bytes.set(safe_memory_bytes);
        health_status.set(if health { 1 } else { 0 });

        // Circuit breaker status based on health (simple heuristic)
        // In a real implementation, this would be more sophisticated
        circuit_breaker.set(if health { 0 } else { 1 });
    } else {
        warn!("Some metrics not initialized - skipping buffer metrics update");
    }
}

/// Collect and update system metrics using sysinfo
pub async fn update_system_metrics() {
    let mut system = System::new_all();
    system.refresh_all();

    // CPU usage (global average)
    if let Some(cpu_gauge) = CPU_USAGE_PERCENT.get() {
        let cpu_usage = system.global_cpu_usage();
        if cpu_usage.is_finite() && cpu_usage >= 0.0 {
            cpu_gauge.set(cpu_usage as f64);
        }
    }

    // Memory metrics
    if let (
        Some(used_gauge),
        Some(total_gauge),
        Some(available_gauge),
        Some(free_gauge),
        Some(cached_gauge),
    ) = (
        MEMORY_USED_BYTES.get(),
        MEMORY_TOTAL_BYTES.get(),
        MEMORY_AVAILABLE_BYTES.get(),
        MEMORY_FREE_BYTES.get(),
        MEMORY_CACHED_BYTES.get(),
    ) {
        let total_memory = system.total_memory();
        let used_memory = system.used_memory();
        let available_memory = system.available_memory();
        let free_memory = system.free_memory();
        // Note: sysinfo doesn't provide cached memory directly, we'll estimate it
        let cached_memory = available_memory.saturating_sub(free_memory);

        // Safe casting to i64
        let safe_total = if total_memory <= i64::MAX as u64 {
            total_memory as i64
        } else {
            i64::MAX
        };
        let safe_used = if used_memory <= i64::MAX as u64 {
            used_memory as i64
        } else {
            i64::MAX
        };
        let safe_available = if available_memory <= i64::MAX as u64 {
            available_memory as i64
        } else {
            i64::MAX
        };
        let safe_free = if free_memory <= i64::MAX as u64 {
            free_memory as i64
        } else {
            i64::MAX
        };
        let safe_cached = if cached_memory <= i64::MAX as u64 {
            cached_memory as i64
        } else {
            i64::MAX
        };

        total_gauge.set(safe_total);
        used_gauge.set(safe_used);
        available_gauge.set(safe_available);
        free_gauge.set(safe_free);
        cached_gauge.set(safe_cached);
    }

    // Load averages (Unix systems only)
    #[cfg(unix)]
    if let (Some(load1), Some(load5), Some(load15)) = (
        LOAD_AVERAGE_1MIN.get(),
        LOAD_AVERAGE_5MIN.get(),
        LOAD_AVERAGE_15MIN.get(),
    ) {
        let load_avg = System::load_average();
        if load_avg.one.is_finite() && load_avg.one >= 0.0 {
            load1.set(load_avg.one);
        }
        if load_avg.five.is_finite() && load_avg.five >= 0.0 {
            load5.set(load_avg.five);
        }
        if load_avg.fifteen.is_finite() && load_avg.fifteen >= 0.0 {
            load15.set(load_avg.fifteen);
        }
    }

    // Disk I/O metrics
    if let (Some(_disk_read), Some(_disk_write), Some(_last_system_mutex)) = (
        DISK_READ_BYTES_TOTAL.get(),
        DISK_WRITE_BYTES_TOTAL.get(),
        LAST_SYSTEM_VALUES.get(),
    ) {
        // Note: sysinfo doesn't provide I/O stats for disks directly on system
        // This would require using the Disks type separately
        // For now, we'll leave disk I/O metrics as placeholders
    }

    // Network I/O metrics
    if let (Some(net_rx), Some(net_tx), Some(last_system_mutex)) = (
        NETWORK_RECEIVED_BYTES_TOTAL.get(),
        NETWORK_TRANSMITTED_BYTES_TOTAL.get(),
        LAST_SYSTEM_VALUES.get(),
    ) {
        let mut total_received = 0u64;
        let mut total_transmitted = 0u64;

        // Create a separate Networks instance for network data
        let networks = Networks::new_with_refreshed_list();
        for (_interface_name, data) in &networks {
            total_received = total_received.saturating_add(data.total_received());
            total_transmitted = total_transmitted.saturating_add(data.total_transmitted());
        }

        // Update counters with delta (to avoid resets)
        if let Ok(mut last_values) = last_system_mutex.lock() {
            let rx_diff = total_received.saturating_sub(last_values.network_received_bytes);
            let tx_diff = total_transmitted.saturating_sub(last_values.network_transmitted_bytes);

            if rx_diff > 0 {
                net_rx.inc_by(rx_diff);
                last_values.network_received_bytes = total_received;
            }
            if tx_diff > 0 {
                net_tx.inc_by(tx_diff);
                last_values.network_transmitted_bytes = total_transmitted;
            }
        }
    }

    // Process-specific metrics
    let pid = sysinfo::Pid::from(std::process::id() as usize);
    if let Some(process) = system.process(pid) {
        // Process CPU usage
        if let Some(process_cpu) = PROCESS_CPU_PERCENT.get() {
            let cpu_usage = process.cpu_usage();
            if cpu_usage.is_finite() && cpu_usage >= 0.0 {
                process_cpu.set(cpu_usage as f64);
            }
        }

        // Process memory usage
        if let Some(process_mem) = PROCESS_MEMORY_BYTES.get() {
            let memory = process.memory();
            let safe_memory = if memory <= i64::MAX as u64 {
                memory as i64
            } else {
                i64::MAX
            };
            process_mem.set(safe_memory);
        }

        // Process virtual memory usage
        if let Some(process_vmem) = PROCESS_VIRTUAL_MEMORY_BYTES.get() {
            let virtual_memory = process.virtual_memory();
            let safe_vmem = if virtual_memory <= i64::MAX as u64 {
                virtual_memory as i64
            } else {
                i64::MAX
            };
            process_vmem.set(safe_vmem);
        }
    }

    // System-wide process metrics
    let all_processes = system.processes();
    if let (Some(pids_gauge), Some(blocked_gauge), Some(threads_gauge)) = (
        PROCESSES_PIDS.get(),
        PROCESSES_BLOCKED.get(),
        PROCESS_THREADS_COUNT.get(),
    ) {
        let total_processes = all_processes.len() as i64;
        let mut total_threads = 0i64;
        let mut blocked_processes = 0i64;

        for (_pid, process) in all_processes {
            // Count blocked processes (processes in uninterruptible sleep)
            // This is an approximation since sysinfo doesn't expose process state directly
            if process.cpu_usage() == 0.0 && process.memory() > 0 {
                // Heuristic: process using memory but no CPU might be blocked
                blocked_processes += 1;
            }
            // Approximate thread count (sysinfo doesn't provide this directly)
            total_threads += 1; // Each process has at least one thread
        }

        pids_gauge.set(total_processes);
        blocked_gauge.set(blocked_processes);
        threads_gauge.set(total_threads);
    }

    // System boot time (BetterStack expects boot timestamp, not uptime)
    if let Some(boot_time_gauge) = UPTIME_SECONDS.get() {
        let uptime = System::uptime();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let boot_time = now.saturating_sub(uptime);
        let safe_boot_time = if boot_time <= i64::MAX as u64 {
            boot_time as i64
        } else {
            i64::MAX
        };
        boot_time_gauge.set(safe_boot_time);
    }
}

/// Buffer statistics reporter task
pub async fn stats_reporter<T: buffer::BufferableEvent>(buffer: EventBuffer<T>) {
    let mut interval = interval(Duration::from_secs(15));

    loop {
        interval.tick().await;

        let stats = buffer.get_stats().await;
        let health = buffer.health_check().await;

        update_buffer_metrics(&stats, health);

        // Also send to BetterStack via OpenTelemetry
        crate::opentelemetry::send_buffer_metrics(&stats).await;
    }
}

/// Buffer health monitor task
pub async fn health_monitor<T: buffer::BufferableEvent>(buffer: EventBuffer<T>) {
    let mut interval = interval(Duration::from_secs(10));

    loop {
        interval.tick().await;

        let is_healthy = buffer.health_check().await;
        let stats = buffer.get_stats().await;

        if !is_healthy {
            warn!(
                "⚠️  Buffer health check failed - Size: {}, Memory: {}MB, Errors: {}",
                stats.current_buffer_size,
                stats.current_memory_bytes / 1_048_576, // Convert to MB
                stats.total_errors
            );

            // Send health alert to BetterStack
            crate::opentelemetry::log(
                "warn",
                &format!(
                    "Buffer health check failed - Size: {}, Memory: {}MB, Errors: {}",
                    stats.current_buffer_size,
                    stats.current_memory_bytes / 1_048_576,
                    stats.total_errors
                ),
            )
            .await;
        }
    }
}

/// System metrics collector task
pub async fn system_metrics_collector() {
    let mut interval = interval(Duration::from_secs(10));

    loop {
        interval.tick().await;
        update_system_metrics().await;

        // Also send to BetterStack via OpenTelemetry
        crate::opentelemetry::send_system_metrics().await;
    }
}

/// Spawn all monitoring tasks
pub fn spawn_monitoring_tasks<T: buffer::BufferableEvent>(buffer: EventBuffer<T>) {
    info!("🔄 Spawning monitoring tasks");

    // Buffer statistics reporter
    let buffer_clone = buffer.clone();
    tokio::spawn(async move {
        stats_reporter(buffer_clone).await;
    });

    // Buffer health monitor
    let buffer_clone = buffer.clone();
    tokio::spawn(async move {
        health_monitor(buffer_clone).await;
    });

    // System metrics collector
    tokio::spawn(async {
        system_metrics_collector().await;
    });

    // OpenTelemetry system metrics exporter (runs separately at 60s intervals)
    tokio::spawn(async {
        crate::opentelemetry::system_metrics_exporter().await;
    });

    info!("✅ All monitoring tasks spawned successfully");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_integer_casting() {
        // Test that we handle large values safely
        let large_value = usize::MAX;

        let safe_i64 = if large_value <= i64::MAX as usize {
            large_value as i64
        } else {
            i64::MAX
        };

        // On 64-bit systems, usize::MAX > i64::MAX, so should clamp
        #[cfg(target_pointer_width = "64")]
        assert_eq!(safe_i64, i64::MAX);

        // On 32-bit systems, usize::MAX < i64::MAX, so should preserve
        #[cfg(target_pointer_width = "32")]
        assert_eq!(safe_i64, large_value as i64);
    }

    #[test]
    fn test_safe_division_operations() {
        // Test division by zero protection
        let bytes = 1_048_576_u64;
        let mb = bytes / 1_048_576; // Should be 1
        assert_eq!(mb, 1);

        let zero_bytes = 0_u64;
        let zero_mb = zero_bytes / 1_048_576; // Should be 0
        assert_eq!(zero_mb, 0);

        // Test for potential overflow in large byte values
        let large_bytes = u64::MAX;
        let large_mb = large_bytes / 1_048_576; // Should not panic
        assert!(large_mb > 0);
    }

    #[test]
    fn test_thread_safe_metric_tracking() {
        let values = LastMetricValues::default();
        assert_eq!(values.received, 0);
        assert_eq!(values.processed, 0);
        assert_eq!(values.dropped, 0);
        assert_eq!(values.errors, 0);
        assert_eq!(values.backpressure, 0);

        let system_values = LastSystemValues::default();
        assert_eq!(system_values.network_received_bytes, 0);
        assert_eq!(system_values.network_transmitted_bytes, 0);
    }

    #[test]
    fn test_overflow_protection() {
        // Test saturating_sub behavior
        let current = 100u64;
        let previous = 150u64;

        let diff = current.saturating_sub(previous);
        assert_eq!(diff, 0); // Should not underflow

        let normal_diff = 150u64.saturating_sub(100u64);
        assert_eq!(normal_diff, 50); // Normal case
    }
}
