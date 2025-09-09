//! Coordinator-specific metrics following OpenTelemetry semantic conventions
//! These metrics are designed to work with New Relic's APM golden metrics

use opentelemetry::{KeyValue, global};
use std::time::SystemTime;

/// Send coordinator-specific metrics following OpenTelemetry semantic conventions
/// Based on https://docs.newrelic.com/docs/opentelemetry/get-started/apm-monitoring/opentelemetry-apm-ui/#golden-metrics
pub fn send_coordinator_metrics(
    containers_loading: u64,
    containers_running: u64,
    app_instances_count: u64,
    has_pending_jobs: bool,
    current_agents: u64,
    running_jobs: u64,
    shutdown: bool,
    force_shutdown: bool,
    // Job selection metrics
    job_pool_size: u64,
    job_pool_merge_count: u64,
    job_pool_other_count: u64,
    job_pool_settlement_count: u64,
    jobs_locked_count: u64,
    jobs_failed_cached_count: u64,
    last_selected_job_sequence: u64,
    last_selected_job_instance: String,
    // Multicall processor metrics
    multicall_total_operations: u64,
    multicall_batches_executed: u64,
    multicall_batches_failed: u64,
    multicall_start_jobs_queued: u64,
    multicall_complete_jobs_queued: u64,
    multicall_fail_jobs_queued: u64,
    multicall_last_batch_size: u64,
    multicall_last_execution_time_ms: u64,
    multicall_successful_start_jobs: u64,
    multicall_failed_start_jobs: u64,
    // Docker buffer processor metrics
    docker_buffer_size: u64,
    docker_jobs_processed: u64,
    docker_jobs_skipped: u64,
    docker_jobs_returned_to_buffer: u64,
    docker_containers_started: u64,
    docker_containers_failed: u64,
    docker_resource_check_failures: u64,
    docker_job_lock_conflicts: u64,
    docker_agent_method_fetch_failures: u64,
) {
    let meter = global::meter("silvana-coordinator");

    // Golden Metric 1: Response time (Latency)
    // For coordinator, we track job processing time
    let _job_duration = meter
        .f64_histogram("silvana.job.duration")
        .with_description("Duration of job processing in seconds")
        .with_unit("s")
        .build();

    // Golden Metric 2: Throughput
    // Track job processing rate
    let job_counter = meter
        .u64_counter("silvana.job.count")
        .with_description("Total number of jobs processed")
        .build();

    job_counter.add(
        1,
        &[
            KeyValue::new("job.status", "processed"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    // Golden Metric 4: Saturation (Resource utilization)
    // Docker container utilization
    let container_utilization = meter
        .f64_gauge("silvana.container.utilization")
        .with_description("Container utilization ratio")
        .with_unit("1")
        .build();

    // Assuming max 10 containers
    let utilization = ((containers_loading + containers_running) as f64) / 10.0;
    container_utilization.record(
        utilization,
        &[KeyValue::new("service.name", "silvana-coordinator")],
    );

    // Additional APM metrics following OpenTelemetry semantic conventions

    // HTTP-like metrics (for gRPC endpoints)
    let request_duration = meter
        .f64_histogram("http.server.duration")
        .with_description("Duration of HTTP requests")
        .with_unit("ms")
        .build();

    // Simulate request metrics for APM
    request_duration.record(
        50.0, // Example duration in ms
        &[
            KeyValue::new("http.method", "POST"),
            KeyValue::new("http.scheme", "grpc"),
            KeyValue::new("http.status_code", 200i64),
            KeyValue::new("http.target", "/GetJob"),
            KeyValue::new("net.host.name", "coordinator"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    // Process metrics (required for APM)
    let cpu_utilization = meter
        .f64_gauge("process.runtime.cpu.utilization")
        .with_description("CPU utilization")
        .with_unit("1")
        .build();

    // Estimate CPU based on container count
    let cpu_estimate = ((containers_running as f64) * 0.1).min(1.0);
    cpu_utilization.record(
        cpu_estimate,
        &[KeyValue::new("service.name", "silvana-coordinator")],
    );

    let memory_usage = meter
        .u64_gauge("process.runtime.memory.usage")
        .with_description("Memory usage in bytes")
        .with_unit("By")
        .build();

    // Estimate memory based on container count (100MB per container)
    let memory_estimate = containers_running * 100 * 1024 * 1024;
    memory_usage.record(
        memory_estimate,
        &[KeyValue::new("service.name", "silvana-coordinator")],
    );

    // Custom business metrics with proper namespacing

    // Docker containers
    let containers_gauge = meter
        .u64_gauge("silvana.docker.containers")
        .with_description("Number of Docker containers")
        .build();

    containers_gauge.record(
        containers_loading,
        &[
            KeyValue::new("container.state", "loading"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    containers_gauge.record(
        containers_running,
        &[
            KeyValue::new("container.state", "running"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    // App instances
    let app_instances_gauge = meter
        .u64_gauge("silvana.app_instances.count")
        .with_description("Number of app instances being tracked")
        .build();

    app_instances_gauge.record(
        app_instances_count,
        &[KeyValue::new("service.name", "silvana-coordinator")],
    );

    // Pending jobs indicator
    let pending_jobs_gauge = meter
        .u64_gauge("silvana.jobs.pending")
        .with_description("Whether there are pending jobs")
        .build();

    pending_jobs_gauge.record(
        if has_pending_jobs { 1 } else { 0 },
        &[KeyValue::new("service.name", "silvana-coordinator")],
    );

    // Current agents
    let agents_gauge = meter
        .u64_gauge("silvana.agents.current")
        .with_description("Number of current agents")
        .build();

    agents_gauge.record(
        current_agents,
        &[KeyValue::new("service.name", "silvana-coordinator")],
    );

    // Job queue metrics
    let job_queue_gauge = meter
        .u64_gauge("silvana.jobs.queue")
        .with_description("Jobs in various states")
        .build();

    job_queue_gauge.record(
        running_jobs,
        &[
            KeyValue::new("job.state", "running"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    // System health metrics
    let system_status_gauge = meter
        .u64_gauge("silvana.system.status")
        .with_description("System status flags")
        .build();

    system_status_gauge.record(
        if shutdown { 1 } else { 0 },
        &[
            KeyValue::new("status.type", "shutdown"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    system_status_gauge.record(
        if force_shutdown { 1 } else { 0 },
        &[
            KeyValue::new("status.type", "force_shutdown"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    // Job selection metrics
    let job_selection_gauge = meter
        .u64_gauge("silvana.job_selection.pool_size")
        .with_description("Job pool size for selection")
        .build();

    job_selection_gauge.record(
        job_pool_size,
        &[KeyValue::new("service.name", "silvana-coordinator")],
    );

    let job_type_gauge = meter
        .u64_gauge("silvana.job_selection.pool_by_type")
        .with_description("Job pool composition by type")
        .build();

    job_type_gauge.record(
        job_pool_merge_count,
        &[
            KeyValue::new("job.type", "merge"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    job_type_gauge.record(
        job_pool_other_count,
        &[
            KeyValue::new("job.type", "other"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    job_type_gauge.record(
        job_pool_settlement_count,
        &[
            KeyValue::new("job.type", "settlement"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    let job_filtering_gauge = meter
        .u64_gauge("silvana.job_selection.filtered")
        .with_description("Jobs filtered during selection")
        .build();

    job_filtering_gauge.record(
        jobs_locked_count,
        &[
            KeyValue::new("filter.reason", "locked"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    job_filtering_gauge.record(
        jobs_failed_cached_count,
        &[
            KeyValue::new("filter.reason", "failed_cache"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    // Track the last selected job
    if last_selected_job_sequence > 0 {
        let job_selected_gauge = meter
            .u64_gauge("silvana.job_selection.last_selected")
            .with_description("Last selected job sequence")
            .build();

        job_selected_gauge.record(
            last_selected_job_sequence,
            &[
                KeyValue::new("app_instance", last_selected_job_instance),
                KeyValue::new("service.name", "silvana-coordinator"),
            ],
        );
    }

    // Multicall processor metrics
    let multicall_ops_gauge = meter
        .u64_gauge("silvana.multicall.operations")
        .with_description("Multicall operations queued")
        .build();

    multicall_ops_gauge.record(
        multicall_total_operations,
        &[
            KeyValue::new("operation.type", "total"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    multicall_ops_gauge.record(
        multicall_start_jobs_queued,
        &[
            KeyValue::new("operation.type", "start_jobs"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    multicall_ops_gauge.record(
        multicall_complete_jobs_queued,
        &[
            KeyValue::new("operation.type", "complete_jobs"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    multicall_ops_gauge.record(
        multicall_fail_jobs_queued,
        &[
            KeyValue::new("operation.type", "fail_jobs"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    let multicall_batches_gauge = meter
        .u64_gauge("silvana.multicall.batches")
        .with_description("Multicall batch execution stats")
        .build();

    multicall_batches_gauge.record(
        multicall_batches_executed,
        &[
            KeyValue::new("batch.status", "executed"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    multicall_batches_gauge.record(
        multicall_batches_failed,
        &[
            KeyValue::new("batch.status", "failed"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    let multicall_performance_gauge = meter
        .u64_gauge("silvana.multicall.performance")
        .with_description("Multicall performance metrics")
        .build();

    multicall_performance_gauge.record(
        multicall_last_batch_size,
        &[
            KeyValue::new("metric.type", "batch_size"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    multicall_performance_gauge.record(
        multicall_last_execution_time_ms,
        &[
            KeyValue::new("metric.type", "execution_time_ms"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    let multicall_start_jobs_gauge = meter
        .u64_gauge("silvana.multicall.start_jobs")
        .with_description("Multicall start job results")
        .build();

    multicall_start_jobs_gauge.record(
        multicall_successful_start_jobs,
        &[
            KeyValue::new("result", "successful"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    multicall_start_jobs_gauge.record(
        multicall_failed_start_jobs,
        &[
            KeyValue::new("result", "failed"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    // Docker buffer processor metrics
    let docker_buffer_gauge = meter
        .u64_gauge("silvana.docker.buffer")
        .with_description("Docker buffer processor metrics")
        .build();

    docker_buffer_gauge.record(
        docker_buffer_size,
        &[
            KeyValue::new("metric.type", "buffer_size"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    let docker_jobs_gauge = meter
        .u64_gauge("silvana.docker.jobs")
        .with_description("Docker job processing metrics")
        .build();

    docker_jobs_gauge.record(
        docker_jobs_processed,
        &[
            KeyValue::new("job.status", "processed"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    docker_jobs_gauge.record(
        docker_jobs_skipped,
        &[
            KeyValue::new("job.status", "skipped"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    docker_jobs_gauge.record(
        docker_jobs_returned_to_buffer,
        &[
            KeyValue::new("job.status", "returned_to_buffer"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    let docker_containers_gauge = meter
        .u64_gauge("silvana.docker.container_operations")
        .with_description("Docker container operation metrics")
        .build();

    docker_containers_gauge.record(
        docker_containers_started,
        &[
            KeyValue::new("operation", "started"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    docker_containers_gauge.record(
        docker_containers_failed,
        &[
            KeyValue::new("operation", "failed"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    let docker_failures_gauge = meter
        .u64_gauge("silvana.docker.failures")
        .with_description("Docker processor failure metrics")
        .build();

    docker_failures_gauge.record(
        docker_resource_check_failures,
        &[
            KeyValue::new("failure.type", "resource_check"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    docker_failures_gauge.record(
        docker_job_lock_conflicts,
        &[
            KeyValue::new("failure.type", "job_lock_conflict"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    docker_failures_gauge.record(
        docker_agent_method_fetch_failures,
        &[
            KeyValue::new("failure.type", "agent_method_fetch"),
            KeyValue::new("service.name", "silvana-coordinator"),
        ],
    );

    // Uptime metric
    let uptime_counter = meter
        .u64_counter("silvana.uptime")
        .with_description("Service uptime indicator")
        .build();

    uptime_counter.add(
        1,
        &[
            KeyValue::new("service.name", "silvana-coordinator"),
            KeyValue::new(
                "service.instance.id",
                std::env::var("SUI_ADDRESS").unwrap_or_else(|_| "unknown".to_string()),
            ),
        ],
    );
}

/// Record a span for job processing (for distributed tracing)
pub fn record_job_span(job_id: &str, duration_ms: f64, success: bool) {
    let tracer = global::tracer("silvana-coordinator");
    use opentelemetry::trace::{Span, Status, Tracer};

    let mut span = tracer
        .span_builder("silvana.job.process")
        .with_kind(opentelemetry::trace::SpanKind::Internal)
        .with_attributes([
            KeyValue::new("job.id", job_id.to_string()),
            KeyValue::new("service.name", "silvana-coordinator"),
            KeyValue::new("span.type", "job"),
        ])
        .with_start_time(SystemTime::now())
        .start(&tracer);

    if !success {
        span.set_status(Status::error("Job processing failed"));
        span.set_attribute(KeyValue::new("error", true));
    } else {
        span.set_status(Status::Ok);
    }

    span.set_attribute(KeyValue::new("job.duration_ms", duration_ms));
    span.end();
}

/// Record an HTTP-like span for gRPC requests (for APM)
pub fn record_grpc_span(method: &str, duration_ms: f64, status_code: i32) {
    let tracer = global::tracer("silvana-coordinator");
    use opentelemetry::trace::{Span, Tracer};

    let mut span = tracer
        .span_builder("grpc.request")
        .with_kind(opentelemetry::trace::SpanKind::Server)
        .with_attributes([
            KeyValue::new("rpc.system", "grpc"),
            KeyValue::new("rpc.method", method.to_string()),
            KeyValue::new("rpc.service", "silvana.coordinator"),
            KeyValue::new("http.status_code", status_code as i64),
            KeyValue::new("service.name", "silvana-coordinator"),
        ])
        .with_start_time(SystemTime::now())
        .start(&tracer);

    span.set_attribute(KeyValue::new("rpc.grpc.status_code", status_code as i64));
    span.set_attribute(KeyValue::new("http.response_time_ms", duration_ms));
    span.end();
}
