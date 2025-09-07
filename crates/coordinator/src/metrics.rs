use monitoring::coordinator_metrics;
use monitoring::newrelic::NewRelicConfig;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::time::{Duration, interval};
use tracing::{debug, info};

use crate::constants::METRICS_REPORTING_INTERVAL_SECS;
use crate::jobs::JobsTracker;
use crate::state::SharedState;

/// Global metrics for coordinator
pub struct CoordinatorMetrics {
    pub docker_containers_loading: Arc<AtomicUsize>,
    pub docker_containers_running: Arc<AtomicUsize>,
    pub app_instances_tracked: Arc<AtomicUsize>,
    pub has_pending_jobs: Arc<AtomicBool>,
    pub shutdown_flag: Arc<AtomicBool>,
    pub force_shutdown_flag: Arc<AtomicBool>,
    // Job selection metrics
    pub job_pool_size: Arc<AtomicUsize>,
    pub job_pool_merge_count: Arc<AtomicUsize>,
    pub job_pool_other_count: Arc<AtomicUsize>,
    pub job_pool_settlement_count: Arc<AtomicUsize>,
    pub jobs_locked_count: Arc<AtomicUsize>,
    pub jobs_failed_cached_count: Arc<AtomicUsize>,
    pub last_selected_job_sequence: Arc<AtomicUsize>,
    pub last_selected_job_instance: Arc<parking_lot::RwLock<String>>,
}

impl CoordinatorMetrics {
    pub fn new() -> Self {
        Self {
            docker_containers_loading: Arc::new(AtomicUsize::new(0)),
            docker_containers_running: Arc::new(AtomicUsize::new(0)),
            app_instances_tracked: Arc::new(AtomicUsize::new(0)),
            has_pending_jobs: Arc::new(AtomicBool::new(false)),
            shutdown_flag: Arc::new(AtomicBool::new(false)),
            force_shutdown_flag: Arc::new(AtomicBool::new(false)),
            job_pool_size: Arc::new(AtomicUsize::new(0)),
            job_pool_merge_count: Arc::new(AtomicUsize::new(0)),
            job_pool_other_count: Arc::new(AtomicUsize::new(0)),
            job_pool_settlement_count: Arc::new(AtomicUsize::new(0)),
            jobs_locked_count: Arc::new(AtomicUsize::new(0)),
            jobs_failed_cached_count: Arc::new(AtomicUsize::new(0)),
            last_selected_job_sequence: Arc::new(AtomicUsize::new(0)),
            last_selected_job_instance: Arc::new(parking_lot::RwLock::new(String::new())),
        }
    }

    pub fn set_docker_containers(&self, loading: usize, running: usize) {
        self.docker_containers_loading
            .store(loading, Ordering::Relaxed);
        self.docker_containers_running
            .store(running, Ordering::Relaxed);
    }

    pub fn set_app_instances(&self, count: usize) {
        self.app_instances_tracked.store(count, Ordering::Relaxed);
    }

    pub fn set_has_pending_jobs(&self, has_pending: bool) {
        self.has_pending_jobs.store(has_pending, Ordering::Relaxed);
    }

    pub fn set_shutdown_flags(&self, shutdown: bool, force_shutdown: bool) {
        self.shutdown_flag.store(shutdown, Ordering::Relaxed);
        self.force_shutdown_flag
            .store(force_shutdown, Ordering::Relaxed);
    }

    pub fn set_job_selection_metrics(
        &self,
        pool_size: usize,
        merge_count: usize,
        other_count: usize,
        settlement_count: usize,
        locked_count: usize,
        failed_cached_count: usize,
        selected_job_sequence: u64,
        selected_job_instance: String,
    ) {
        self.job_pool_size.store(pool_size, Ordering::Relaxed);
        self.job_pool_merge_count
            .store(merge_count, Ordering::Relaxed);
        self.job_pool_other_count
            .store(other_count, Ordering::Relaxed);
        self.job_pool_settlement_count
            .store(settlement_count, Ordering::Relaxed);
        self.jobs_locked_count
            .store(locked_count, Ordering::Relaxed);
        self.jobs_failed_cached_count
            .store(failed_cached_count, Ordering::Relaxed);
        self.last_selected_job_sequence
            .store(selected_job_sequence as usize, Ordering::Relaxed);
        *self.last_selected_job_instance.write() = selected_job_instance;
    }
}

/// Collect and log coordinator metrics
async fn collect_coordinator_metrics(
    state: SharedState,
    metrics: Arc<CoordinatorMetrics>,
    jobs_tracker: JobsTracker,
) {
    // Docker container metrics
    let containers_loading = metrics.docker_containers_loading.load(Ordering::Relaxed);
    let containers_running = metrics.docker_containers_running.load(Ordering::Relaxed);
    let containers_total = containers_loading + containers_running;

    // App instances tracked
    let app_instances_with_jobs = jobs_tracker.get_app_instances_with_jobs().await;
    let app_instances_count = app_instances_with_jobs.len();
    metrics.set_app_instances(app_instances_count);

    // Pending jobs status - check if we have any pending jobs
    let has_pending = !app_instances_with_jobs.is_empty();
    metrics.set_has_pending_jobs(has_pending);

    // Shutdown flags
    let shutdown = state.is_shutting_down();
    let force_shutdown = state.is_force_shutting_down();
    metrics.set_shutdown_flags(shutdown, force_shutdown);

    // Current agents count
    let current_agents = state.get_current_agents_count().await;

    // Agent job database stats
    let running_jobs = state.get_running_jobs().await;

    // Job selection metrics
    let job_pool_size = metrics.job_pool_size.load(Ordering::Relaxed);
    let job_pool_merge_count = metrics.job_pool_merge_count.load(Ordering::Relaxed);
    let job_pool_other_count = metrics.job_pool_other_count.load(Ordering::Relaxed);
    let job_pool_settlement_count = metrics.job_pool_settlement_count.load(Ordering::Relaxed);
    let jobs_locked_count = metrics.jobs_locked_count.load(Ordering::Relaxed);
    let jobs_failed_cached_count = metrics.jobs_failed_cached_count.load(Ordering::Relaxed);
    let last_selected_job_sequence = metrics.last_selected_job_sequence.load(Ordering::Relaxed);
    let last_selected_job_instance = metrics.last_selected_job_instance.read().clone();

    // Coordinator info
    let coordinator_id = state.get_coordinator_id();
    let chain = state.get_chain();

    // Send OpenTelemetry metrics to New Relic (if configured)
    coordinator_metrics::send_coordinator_metrics(
        containers_loading as u64,
        containers_running as u64,
        app_instances_count as u64,
        has_pending,
        current_agents as u64,
        running_jobs as u64,
        shutdown,
        force_shutdown,
        job_pool_size as u64,
        job_pool_merge_count as u64,
        job_pool_other_count as u64,
        job_pool_settlement_count as u64,
        jobs_locked_count as u64,
        jobs_failed_cached_count as u64,
        last_selected_job_sequence as u64,
        last_selected_job_instance,
    );

    // Log metrics at debug level (these will be sent to New Relic if warn/error occur)
    debug!(
        "📊 Coordinator metrics: containers_loading={}, containers_running={}, containers_total={}, \
        app_instances_tracked={}, has_pending_jobs={}, current_agents={}, \
        agent_jobs_running={}, \
        job_pool_size={}, job_pool_merge={}, job_pool_other={}, job_pool_settlement={}, \
        jobs_locked={}, jobs_failed_cached={}, last_job_seq={}, \
        shutdown={}, force_shutdown={}, coordinator_id={}, chain={}",
        containers_loading,
        containers_running,
        containers_total,
        app_instances_count,
        has_pending,
        current_agents,
        running_jobs,
        job_pool_size,
        job_pool_merge_count,
        job_pool_other_count,
        job_pool_settlement_count,
        jobs_locked_count,
        jobs_failed_cached_count,
        last_selected_job_sequence,
        shutdown,
        force_shutdown,
        coordinator_id,
        chain
    );

    // Log important changes at info level (these become alerts in New Relic)
    if containers_total > 5 {
        info!(
            "⚡ High container count: {} containers running/loading",
            containers_total
        );
    }

    if app_instances_count > 10 {
        info!(
            "📈 Tracking {} app instances with pending jobs",
            app_instances_count
        );
    }

    // Alert on system issues
    if shutdown {
        info!("🛑 Coordinator shutdown initiated");
    }

    if force_shutdown {
        info!("🚨 Coordinator force shutdown initiated");
    }
}

/// Start periodic metrics collection
pub async fn start_metrics_reporter(
    state: SharedState,
    metrics: Arc<CoordinatorMetrics>,
    jobs_tracker: JobsTracker,
) {
    if !NewRelicConfig::is_configured() {
        info!("New Relic not configured, metrics reporter will only log locally");
    } else {
        info!(
            "📊 Starting metrics reporter (collects every {} seconds)",
            METRICS_REPORTING_INTERVAL_SECS
        );
    }

    let mut ticker = interval(Duration::from_secs(METRICS_REPORTING_INTERVAL_SECS));

    loop {
        ticker.tick().await;

        // Clone for async move
        let state_clone = state.clone();
        let metrics_clone = metrics.clone();
        let jobs_tracker_clone = jobs_tracker.clone();

        // Collect metrics
        collect_coordinator_metrics(state_clone, metrics_clone, jobs_tracker_clone).await;
    }
}
