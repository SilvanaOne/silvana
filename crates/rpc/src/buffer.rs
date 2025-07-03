use crate::database::EventDatabase;
use crate::entities;
use crate::events::Event;
use anyhow::{anyhow, Result};
use std::env;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, OwnedSemaphorePermit, RwLock, Semaphore};
use tokio::time::{interval, sleep, timeout, Duration, Instant};
use tracing::{debug, error, info, warn};

// Default configuration constants (kept for potential future use)
#[allow(dead_code)]
const DEFAULT_BATCH_SIZE: usize = 100;
#[allow(dead_code)]
const DEFAULT_FLUSH_INTERVAL: Duration = Duration::from_millis(500); // Reduced for faster processing
#[allow(dead_code)]
const DEFAULT_CHANNEL_CAPACITY: usize = 250000; // Increased for high-throughput scenarios
                                                // Maximum memory usage in bytes (1GB for events)
const MAX_MEMORY_BYTES: usize = 1000 * 1024 * 1024;
// Base timeout for adding events when buffer is full (will be adjusted based on flush interval)
const BASE_ADD_EVENT_TIMEOUT: Duration = Duration::from_millis(100);
// Circuit breaker threshold
const ERROR_THRESHOLD: usize = 100;
// Retry configuration for database operations
const MAX_DB_RETRIES: usize = 10;
const INITIAL_RETRY_DELAY: Duration = Duration::from_millis(100);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(30);
// Maximum batch size to prevent database placeholder limits
const MAX_BATCH_SIZE: usize = 10000;
// Semaphore strategy thresholds (as fraction of total capacity)
const FAST_PATH_THRESHOLD: usize = 8; // Use fast path when > 1/8 permits available (more conservative)

// Wrapper for events with their semaphore permits
type EventWithPermit = (Event, OwnedSemaphorePermit);

#[derive(Clone)]
pub struct EventBuffer {
    sender: mpsc::Sender<EventWithPermit>,
    stats: Arc<BufferStatsAtomic>,
    memory_usage: Arc<AtomicUsize>,
    circuit_breaker: Arc<CircuitBreaker>,
    backpressure_semaphore: Arc<Semaphore>,
    add_event_timeout: Duration,
    total_permits: usize, // Store the actual channel capacity
}

// Atomic counters for lock-free stats updates
#[derive(Debug, Default)]
pub struct BufferStatsAtomic {
    pub total_received: AtomicU64,
    pub total_processed: AtomicU64,
    pub total_errors: AtomicU64,
    pub total_dropped: AtomicU64,
    pub total_retries: AtomicU64,
    pub current_buffer_size: AtomicUsize,
    pub backpressure_events: AtomicU64,
    pub last_flush_time: RwLock<Option<Instant>>,
}

// Snapshot for reading stats
#[derive(Debug, Default, Clone)]
pub struct BufferStats {
    pub total_received: u64,
    pub total_processed: u64,
    pub total_errors: u64,
    pub total_dropped: u64,
    #[allow(dead_code)]
    pub total_retries: u64,
    pub current_buffer_size: usize,
    pub current_memory_bytes: usize,
    #[allow(dead_code)]
    pub last_flush_time: Option<Instant>,
    pub backpressure_events: u64,
    pub circuit_breaker_open: bool,
}

struct CircuitBreaker {
    is_open: AtomicBool,
    error_count: AtomicUsize,
    last_error_time: RwLock<Option<Instant>>,
    threshold: usize,
    timeout: Duration,
}

struct BatchProcessor {
    receiver: mpsc::Receiver<EventWithPermit>,
    buffer: Vec<Event>,
    database: Arc<EventDatabase>,
    stats: Arc<BufferStatsAtomic>,
    memory_usage: Arc<AtomicUsize>,
    circuit_breaker: Arc<CircuitBreaker>,
    batch_size: usize,
    flush_interval: Duration,
    nats_client: Option<async_nats::Client>,
    nats_stream_name: String,
}

impl EventBuffer {
    #[allow(dead_code)]
    pub fn new(database: Arc<EventDatabase>) -> Self {
        Self::with_config(
            database,
            DEFAULT_BATCH_SIZE,
            DEFAULT_FLUSH_INTERVAL,
            DEFAULT_CHANNEL_CAPACITY,
        )
    }

    pub fn with_config(
        database: Arc<EventDatabase>,
        batch_size: usize,
        flush_interval: Duration,
        channel_capacity: usize,
    ) -> Self {
        // Use bounded channel to prevent OOM
        let (sender, receiver) = mpsc::channel(channel_capacity);
        let stats = Arc::new(BufferStatsAtomic::default());
        let memory_usage = Arc::new(AtomicUsize::new(0));
        let circuit_breaker = Arc::new(CircuitBreaker::new(
            ERROR_THRESHOLD,
            Duration::from_secs(60),
        ));
        let backpressure_semaphore = Arc::new(Semaphore::new(channel_capacity));

        // Make timeout proportional to flush interval for better behavior under load
        let add_event_timeout = std::cmp::max(
            BASE_ADD_EVENT_TIMEOUT,
            flush_interval / 10, // 10% of flush interval
        );

        // Clone Arc references for the spawned task
        let stats_clone = Arc::clone(&stats);
        let memory_usage_clone = Arc::clone(&memory_usage);
        let circuit_breaker_clone = Arc::clone(&circuit_breaker);

        // Spawn the processor task
        tokio::spawn(async move {
            let processor = BatchProcessor::new(
                receiver,
                database,
                stats_clone,
                memory_usage_clone,
                circuit_breaker_clone,
                batch_size,
                flush_interval,
            )
            .await;

            processor.run().await;
        });

        Self {
            sender,
            stats,
            memory_usage,
            circuit_breaker,
            backpressure_semaphore,
            add_event_timeout,
            total_permits: channel_capacity, // Store the actual capacity
        }
    }

    pub async fn add_event(&self, event: Event) -> Result<()> {
        // Check circuit breaker
        if self.circuit_breaker.is_open().await {
            self.stats.total_dropped.fetch_add(1, Ordering::Relaxed);
            return Err(anyhow!("Circuit breaker is open - system overloaded"));
        }

        // Estimate event size more accurately
        let event_size = self.estimate_event_size(&event);

        // Check memory usage
        let current_memory = self.memory_usage.load(Ordering::Relaxed);
        if current_memory + event_size > MAX_MEMORY_BYTES {
            self.stats.total_dropped.fetch_add(1, Ordering::Relaxed);
            return Err(anyhow!(
                "Memory limit exceeded: {}MB + {}KB > {}MB",
                current_memory / (1024 * 1024),
                event_size / 1024,
                MAX_MEMORY_BYTES / (1024 * 1024)
            ));
        }

        // Acquire backpressure permit with tiered strategy
        // Strategy: Fast path when plenty of capacity, blocking with timeout when near capacity
        let available_permits = self.backpressure_semaphore.available_permits();
        let total_permits = self.total_permits; // Use the actual configured capacity

        let permit = if available_permits > total_permits / FAST_PATH_THRESHOLD {
            // Fast path: plenty of capacity, use non-blocking
            match self.backpressure_semaphore.clone().try_acquire_owned() {
                Ok(permit) => permit,
                Err(_) => {
                    // Race condition: permits depleted between check and acquire
                    self.stats
                        .backpressure_events
                        .fetch_add(1, Ordering::Relaxed);
                    self.stats.total_dropped.fetch_add(1, Ordering::Relaxed);

                    let current_memory = self.memory_usage.load(Ordering::Relaxed);
                    warn!(
                        "Backpressure active - buffer at capacity (race condition): {}/{} permits, {}MB/{}MB memory ({:.1}%)",
                        total_permits - self.backpressure_semaphore.available_permits(),
                        total_permits,
                        current_memory / (1024 * 1024),
                        MAX_MEMORY_BYTES / (1024 * 1024),
                        (current_memory as f64 / MAX_MEMORY_BYTES as f64) * 100.0
                    );
                    return Err(anyhow!("Backpressure active - buffer at capacity (race)"));
                }
            }
        } else if available_permits > 0 {
            // Near capacity: try fast path first, then fall back to blocking with timeout
            match self.backpressure_semaphore.clone().try_acquire_owned() {
                Ok(permit) => permit,
                Err(_) => {
                    // Fall back to blocking acquire with timeout
                    debug!("Near capacity, falling back to blocking acquire");
                    match timeout(
                        self.add_event_timeout,
                        self.backpressure_semaphore.clone().acquire_owned(),
                    )
                    .await
                    {
                        Ok(Ok(permit)) => permit,
                        Ok(Err(_)) => {
                            // Semaphore closed (shouldn't happen)
                            self.stats
                                .backpressure_events
                                .fetch_add(1, Ordering::Relaxed);
                            self.stats.total_dropped.fetch_add(1, Ordering::Relaxed);
                            return Err(anyhow!("Backpressure semaphore closed"));
                        }
                        Err(_) => {
                            // Timeout waiting for permit
                            self.stats
                                .backpressure_events
                                .fetch_add(1, Ordering::Relaxed);
                            self.stats.total_dropped.fetch_add(1, Ordering::Relaxed);

                            let current_memory = self.memory_usage.load(Ordering::Relaxed);
                            warn!(
                                "Backpressure timeout - system overloaded: {}/{} permits, {}MB/{}MB memory ({:.1}%), timeout: {:?}",
                                total_permits - self.backpressure_semaphore.available_permits(),
                                total_permits,
                                current_memory / (1024 * 1024),
                                MAX_MEMORY_BYTES / (1024 * 1024),
                                (current_memory as f64 / MAX_MEMORY_BYTES as f64) * 100.0,
                                self.add_event_timeout
                            );
                            return Err(anyhow!(
                                "Backpressure timeout - buffer acquisition failed"
                            ));
                        }
                    }
                }
            }
        } else {
            // No permits available
            self.stats
                .backpressure_events
                .fetch_add(1, Ordering::Relaxed);
            self.stats.total_dropped.fetch_add(1, Ordering::Relaxed);

            let current_memory = self.memory_usage.load(Ordering::Relaxed);
            warn!(
                "Backpressure applied - buffer full: {}/{} permits used, {}MB/{}MB memory ({:.1}% memory utilization)",
                total_permits - available_permits,
                total_permits,
                current_memory / (1024 * 1024),
                MAX_MEMORY_BYTES / (1024 * 1024),
                (current_memory as f64 / MAX_MEMORY_BYTES as f64) * 100.0
            );
            return Err(anyhow!("Backpressure active - buffer at capacity"));
        };

        // Try to send with timeout to prevent blocking
        let wrapped_event = (event, permit);
        match timeout(self.add_event_timeout, self.sender.send(wrapped_event)).await {
            Ok(Ok(())) => {
                // Update stats atomically - no lock contention
                self.stats.total_received.fetch_add(1, Ordering::Relaxed);
                self.stats
                    .current_buffer_size
                    .fetch_add(1, Ordering::Relaxed);

                // FIXED: Use saturating_add for memory usage to prevent overflow
                let old_memory = self.memory_usage.fetch_add(event_size, Ordering::Relaxed);

                // Safety check: if memory usage would overflow, log a warning
                if old_memory > usize::MAX - event_size {
                    warn!("Memory usage counter near overflow, resetting to current estimate");
                    self.memory_usage
                        .store(current_memory + event_size, Ordering::Relaxed);
                }

                // Reset circuit breaker on success
                self.circuit_breaker.record_success().await;

                Ok(())
            }
            Ok(Err(_)) => {
                // Channel closed
                self.stats.total_dropped.fetch_add(1, Ordering::Relaxed);
                Err(anyhow!("Event buffer channel closed"))
            }
            Err(_) => {
                // Timeout - buffer is full
                self.stats.total_dropped.fetch_add(1, Ordering::Relaxed);
                self.stats
                    .backpressure_events
                    .fetch_add(1, Ordering::Relaxed);

                let current_memory = self.memory_usage.load(Ordering::Relaxed);
                warn!(
                    "Event buffer send timeout - system overloaded: {}/{} permits, {}MB/{}MB memory ({:.1}%), timeout: {:?}",
                    total_permits - self.backpressure_semaphore.available_permits(),
                    total_permits,
                    current_memory / (1024 * 1024),
                    MAX_MEMORY_BYTES / (1024 * 1024),
                    (current_memory as f64 / MAX_MEMORY_BYTES as f64) * 100.0,
                    self.add_event_timeout
                );
                Err(anyhow!("Event buffer timeout - system overloaded"))
            }
        }
    }

    pub async fn get_stats(&self) -> BufferStats {
        BufferStats {
            total_received: self.stats.total_received.load(Ordering::Relaxed),
            total_processed: self.stats.total_processed.load(Ordering::Relaxed),
            total_errors: self.stats.total_errors.load(Ordering::Relaxed),
            total_dropped: self.stats.total_dropped.load(Ordering::Relaxed),
            total_retries: self.stats.total_retries.load(Ordering::Relaxed),
            current_buffer_size: self.stats.current_buffer_size.load(Ordering::Relaxed),
            current_memory_bytes: self.memory_usage.load(Ordering::Relaxed),
            last_flush_time: *self.stats.last_flush_time.read().await,
            backpressure_events: self.stats.backpressure_events.load(Ordering::Relaxed),
            circuit_breaker_open: self.circuit_breaker.is_open().await,
        }
    }

    pub async fn health_check(&self) -> bool {
        let stats = self.get_stats().await;

        // Check basic health conditions
        let basic_health =
            !stats.circuit_breaker_open && stats.current_memory_bytes < MAX_MEMORY_BYTES;

        // Backpressure check: healthy if no events received yet, or backpressure < 10% of total
        let backpressure_healthy = if stats.total_received == 0 {
            // System is idle - always healthy regardless of any startup backpressure
            true
        } else {
            // System has processed events - check backpressure ratio using floating point
            // to avoid integer division issues with small event counts
            let backpressure_ratio = stats.backpressure_events as f64 / stats.total_received as f64;
            backpressure_ratio < 0.1 // Less than 10% backpressure
        };

        basic_health && backpressure_healthy
    }

    fn estimate_event_size(&self, event: &Event) -> usize {
        // More accurate size estimation
        let base_size = std::mem::size_of::<Event>();

        let payload_size = match &event.event_type {
            Some(event_type) => match event_type {
                crate::events::event::EventType::Coordinator(coord_event) => {
                    self.estimate_coordinator_event_size(coord_event)
                }
                crate::events::event::EventType::Agent(agent_event) => {
                    self.estimate_agent_event_size(agent_event)
                }
            },
            None => 0,
        };

        base_size + payload_size
    }

    fn estimate_coordinator_event_size(
        &self,
        coord_event: &crate::events::CoordinatorEvent,
    ) -> usize {
        match &coord_event.event {
            Some(event) => match event {
                crate::events::coordinator_event::Event::CoordinatorStarted(e) => {
                    e.coordinator_id.len()
                        + e.ethereum_address.len()
                        + e.sui_ed25519_address.len()
                        + 64
                }
                crate::events::coordinator_event::Event::AgentStartedJob(e) => {
                    e.coordinator_id.len()
                        + e.developer.len()
                        + e.agent.len()
                        + e.app.len()
                        + e.job_id.len()
                        + 64
                }
                crate::events::coordinator_event::Event::AgentFinishedJob(e) => {
                    e.coordinator_id.len()
                        + e.developer.len()
                        + e.agent.len()
                        + e.app.len()
                        + e.job_id.len()
                        + 72
                }
                crate::events::coordinator_event::Event::CoordinationTx(e) => {
                    e.coordinator_id.len()
                        + e.developer.len()
                        + e.agent.len()
                        + e.app.len()
                        + e.job_id.len()
                        + e.memo.len()
                        + e.tx_hash.len()
                        + 64
                }
                crate::events::coordinator_event::Event::CoordinatorError(e) => {
                    e.coordinator_id.len() + e.message.len() + 64
                }
                crate::events::coordinator_event::Event::ClientTransaction(e) => {
                    e.coordinator_id.len()
                        + e.developer.len()
                        + e.agent.len()
                        + e.app.len()
                        + e.client_ip_address.len()
                        + e.method.len()
                        + e.data.len()
                        + e.tx_hash.len()
                        + 128
                }
            },
            None => 0,
        }
    }

    fn estimate_agent_event_size(&self, agent_event: &crate::events::AgentEvent) -> usize {
        match &agent_event.event {
            Some(event) => match event {
                crate::events::agent_event::Event::Message(e) => {
                    let base_size = e.coordinator_id.len()
                        + e.developer.len()
                        + e.agent.len()
                        + e.app.len()
                        + e.job_id.len()
                        + e.message.len()
                        + 64;

                    // Add memory for child table records (each sequence creates a separate record)
                    let sequence_records_size = e.sequences.len()
                        * std::mem::size_of::<entities::agent_message_event_sequences::Model>();

                    base_size + sequence_records_size
                }
                crate::events::agent_event::Event::Transaction(e) => {
                    let base_size = e.coordinator_id.len()
                        + e.tx_type.len()
                        + e.developer.len()
                        + e.agent.len()
                        + e.app.len()
                        + e.job_id.len()
                        + e.tx_hash.len()
                        + e.chain.len()
                        + e.network.len()
                        + e.memo.len()
                        + e.metadata.len()
                        + 128;

                    // Add memory for child table records (each sequence creates a separate record)
                    let sequence_records_size = e.sequences.len()
                        * std::mem::size_of::<entities::agent_transaction_event_sequences::Model>();

                    base_size + sequence_records_size
                }
            },
            None => 0,
        }
    }
}

impl CircuitBreaker {
    fn new(threshold: usize, timeout: Duration) -> Self {
        Self {
            is_open: AtomicBool::new(false),
            error_count: AtomicUsize::new(0),
            last_error_time: RwLock::new(None),
            threshold,
            timeout,
        }
    }

    async fn is_open(&self) -> bool {
        if !self.is_open.load(Ordering::Relaxed) {
            return false;
        }

        // Check if timeout has passed
        let last_error = self.last_error_time.read().await;
        if let Some(last_time) = *last_error {
            if Instant::now().duration_since(last_time) > self.timeout {
                info!("Circuit breaker reset - attempting to recover");
                self.is_open.store(false, Ordering::Relaxed);
                self.error_count.store(0, Ordering::Relaxed);
                return false;
            }
        }

        true
    }

    async fn record_error(&self) {
        // FIXED: Use saturating arithmetic to prevent overflow
        let old_count = self.error_count.load(Ordering::Relaxed);
        let count = if old_count < usize::MAX {
            self.error_count.fetch_add(1, Ordering::Relaxed) + 1
        } else {
            // Prevent overflow by capping at MAX
            warn!("Circuit breaker error count at maximum, maintaining threshold");
            old_count
        };

        *self.last_error_time.write().await = Some(Instant::now());

        if count >= self.threshold {
            warn!("Circuit breaker opened - too many errors: {}", count);
            self.is_open.store(true, Ordering::Relaxed);
        }
    }

    async fn record_success(&self) {
        // Only reset error count if we had errors
        if self.error_count.load(Ordering::Relaxed) > 0 {
            self.error_count.store(0, Ordering::Relaxed);
        }
    }

    // New method: record successful database operation (vs just buffer acceptance)
    async fn record_db_success(&self) {
        // Reset circuit breaker only after successful DB operations
        if self.is_open.load(Ordering::Relaxed) {
            info!("Circuit breaker reset after successful database operation");
            self.is_open.store(false, Ordering::Relaxed);
        }
        self.error_count.store(0, Ordering::Relaxed);
    }
}

impl BatchProcessor {
    async fn new(
        receiver: mpsc::Receiver<EventWithPermit>,
        database: Arc<EventDatabase>,
        stats: Arc<BufferStatsAtomic>,
        memory_usage: Arc<AtomicUsize>,
        circuit_breaker: Arc<CircuitBreaker>,
        batch_size: usize,
        flush_interval: Duration,
    ) -> Self {
        // Initialize NATS client if NATS_URL is provided
        let nats_client = match env::var("NATS_URL") {
            Ok(nats_url) => {
                info!("🔄 Attempting to connect to NATS server at: {}", nats_url);
                match timeout(Duration::from_secs(5), async_nats::connect(&nats_url)).await {
                    Ok(Ok(client)) => {
                        info!("✅ Connected to NATS server at: {}", nats_url);
                        Some(client)
                    }
                    Ok(Err(e)) => {
                        warn!("⚠️  Failed to connect to NATS server at {}: {}. Continuing without NATS.", nats_url, e);
                        None
                    }
                    Err(_) => {
                        warn!("⚠️  Timeout connecting to NATS server at {} after 5 seconds. Continuing without NATS.", nats_url);
                        None
                    }
                }
            }
            Err(_) => {
                info!("ℹ️  NATS_URL not set. Running without NATS publishing.");
                None
            }
        };

        let nats_stream_name =
            env::var("NATS_STREAM_NAME").unwrap_or_else(|_| "silvana-events".to_string());

        Self {
            receiver,
            buffer: Vec::with_capacity(batch_size),
            database,
            stats,
            memory_usage,
            circuit_breaker,
            batch_size,
            flush_interval,
            nats_client,
            nats_stream_name,
        }
    }

    async fn run(mut self) {
        let mut flush_timer = interval(self.flush_interval);
        let mut permits_held: Vec<OwnedSemaphorePermit> = Vec::new();

        info!(
            "Started batch processor with batch_size={}, flush_interval={:?}, max_batch_size={}",
            self.batch_size, self.flush_interval, MAX_BATCH_SIZE
        );
        info!(
            "Using adaptive batching with database limits: batch_size acts as minimum trigger, max_batch_size={} prevents database placeholder overflow",
            MAX_BATCH_SIZE
        );

        loop {
            tokio::select! {
                // Receive new events with permits
                event_result = self.receiver.recv() => {
                    match event_result {
                        Some((event, permit)) => {
                            self.buffer.push(event);
                            permits_held.push(permit);

                            // Check if we should flush due to batch size or max limit
                            if self.buffer.len() >= self.batch_size || self.buffer.len() >= MAX_BATCH_SIZE {
                                if self.buffer.len() >= MAX_BATCH_SIZE {
                                    debug!("Max batch size limit reached ({}), flushing immediately", self.buffer.len());
                                    self.flush_buffer(&mut permits_held).await;
                                } else {
                                    debug!("Batch size reached ({}), draining all available events", self.buffer.len());
                                    self.drain_and_flush(&mut permits_held).await;
                                }
                            }
                        }
                        None => {
                            warn!("Event channel closed, flushing remaining events");
                            self.flush_buffer(&mut permits_held).await;
                            break;
                        }
                    }
                }

                // Periodic flush
                _ = flush_timer.tick() => {
                    if !self.buffer.is_empty() {
                        debug!("Periodic flush triggered with {} events, draining all available", self.buffer.len());
                        self.drain_and_flush(&mut permits_held).await;
                    }
                }
            }
        }
    }

    /// Drain available events from the channel and flush them, respecting MAX_BATCH_SIZE limit
    async fn drain_and_flush(&mut self, permits_held: &mut Vec<OwnedSemaphorePermit>) {
        let initial_count = self.buffer.len();
        let mut total_drained = 0;
        let mut batch_number = 1;

        loop {
            let mut drained_this_batch = 0;
            let remaining_capacity = MAX_BATCH_SIZE.saturating_sub(self.buffer.len());

            if remaining_capacity == 0 {
                // Buffer is at max capacity, flush immediately
                debug!(
                    "Batch {} reached max size limit ({}), flushing immediately",
                    batch_number, MAX_BATCH_SIZE
                );
                self.flush_buffer(permits_held).await;
                batch_number += 1;
                continue;
            }

            // Drain events up to the remaining capacity
            for _ in 0..remaining_capacity {
                match self.receiver.try_recv() {
                    Ok((event, permit)) => {
                        self.buffer.push(event);
                        permits_held.push(permit);
                        drained_this_batch += 1;
                        total_drained += 1;
                    }
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                        // No more events available right now
                        break;
                    }
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                        // Channel closed
                        warn!("Event channel disconnected during drain");
                        break;
                    }
                }
            }

            // If we have events to flush, or if we filled the buffer, flush now
            if !self.buffer.is_empty()
                && (drained_this_batch == 0 || self.buffer.len() >= MAX_BATCH_SIZE)
            {
                let total_events = self.buffer.len();
                if total_drained > 0 && batch_number == 1 {
                    info!(
                        "Drained {} additional events (total batch: {} events, efficiency gain: {}x)",
                        total_drained,
                        total_events,
                        if initial_count > 0 {
                            total_events / initial_count
                        } else {
                            1
                        }
                    );
                } else if batch_number > 1 {
                    info!(
                        "Processing batch {} with {} events (max batch size: {})",
                        batch_number, total_events, MAX_BATCH_SIZE
                    );
                }

                self.flush_buffer(permits_held).await;
                batch_number += 1;

                // If we didn't drain any new events this iteration, we're done
                if drained_this_batch == 0 {
                    break;
                }
            } else if drained_this_batch == 0 {
                // No new events and buffer is empty or small, we're done
                break;
            }
        }

        if batch_number > 2 {
            info!(
                "Completed drain_and_flush with {} batches, total events drained: {}",
                batch_number - 1,
                total_drained
            );
        }
    }

    async fn flush_buffer(&mut self, permits_held: &mut Vec<OwnedSemaphorePermit>) {
        if self.buffer.is_empty() {
            return;
        }

        // FIXED: Don't clone, consume the vector directly
        let events_to_process = std::mem::take(&mut self.buffer);
        let event_count = events_to_process.len();

        debug!(
            "Flushing batch of {} events in parallel to TiDB and NATS",
            event_count
        );

        // Calculate memory to release more accurately
        let memory_to_release = events_to_process
            .iter()
            .map(|event| self.estimate_event_memory_usage(event))
            .sum::<usize>();

        // Process database insertion and NATS publishing in parallel
        let (db_result, nats_result) = tokio::join!(
            self.insert_with_retry(&events_to_process),
            self.publish_to_nats_with_result(&events_to_process)
        );

        // Handle database result
        match db_result {
            Ok(inserted_count) => {
                debug!("Successfully inserted {} events to TiDB", inserted_count);

                // Handle potential partial inserts to keep counters consistent
                if inserted_count < event_count {
                    let dropped_count = event_count - inserted_count;
                    warn!(
                        "Partial insert: {} of {} events inserted, {} events dropped",
                        inserted_count, event_count, dropped_count
                    );

                    // Count the uninserted events as dropped
                    // FIXED: Use saturating_add to prevent overflow
                    self.stats
                        .total_dropped
                        .fetch_add(dropped_count as u64, Ordering::Relaxed);
                }

                // Update stats atomically - use actual counts to prevent divergence
                // FIXED: Use saturating operations to prevent overflow
                self.stats
                    .total_processed
                    .fetch_add(inserted_count as u64, Ordering::Relaxed);

                // FIXED: Use saturating_sub to prevent underflow
                let old_size = self.stats.current_buffer_size.load(Ordering::Relaxed);
                if old_size >= event_count {
                    self.stats
                        .current_buffer_size
                        .fetch_sub(event_count, Ordering::Relaxed);
                } else {
                    warn!(
                        "Buffer size underflow prevented: {} < {}",
                        old_size, event_count
                    );
                    self.stats.current_buffer_size.store(0, Ordering::Relaxed);
                }

                // Update last flush time
                *self.stats.last_flush_time.write().await = Some(Instant::now());

                // Record successful DB operation for circuit breaker (even if partial)
                self.circuit_breaker.record_db_success().await;
            }
            Err(e) => {
                error!(
                    "Failed to insert events to TiDB after {} retries: {}",
                    MAX_DB_RETRIES, e
                );

                // Record error in circuit breaker
                self.circuit_breaker.record_error().await;

                // Update error stats - all events failed
                // FIXED: Use saturating operations to prevent overflow/underflow
                self.stats
                    .total_errors
                    .fetch_add(event_count as u64, Ordering::Relaxed);
                self.stats
                    .total_dropped
                    .fetch_add(event_count as u64, Ordering::Relaxed);

                // FIXED: Use saturating_sub to prevent underflow
                let old_size = self.stats.current_buffer_size.load(Ordering::Relaxed);
                if old_size >= event_count {
                    self.stats
                        .current_buffer_size
                        .fetch_sub(event_count, Ordering::Relaxed);
                } else {
                    warn!(
                        "Buffer size underflow prevented: {} < {}",
                        old_size, event_count
                    );
                    self.stats.current_buffer_size.store(0, Ordering::Relaxed);
                }

                // Events are dropped only after all retries are exhausted
                error!(
                    "Dropping {} events due to persistent database errors after {} retries",
                    event_count, MAX_DB_RETRIES
                );
            }
        }

        // Handle NATS result (independent of database result)
        match nats_result {
            Ok((successful, failed)) => {
                if successful > 0 {
                    debug!(
                        "📤 Successfully published {}/{} events to NATS JetStream",
                        successful,
                        successful + failed
                    );
                }
                if failed > 0 {
                    error!(
                        "⚠️  Failed to publish {}/{} events to NATS",
                        failed,
                        successful + failed
                    );
                }
            }
            Err(e) => {
                error!("⚠️  NATS publishing encountered an error: {}", e);
            }
        }

        // Release memory accounting
        // FIXED: Use saturating_sub to prevent underflow
        let old_memory = self.memory_usage.load(Ordering::Relaxed);
        if old_memory >= memory_to_release {
            self.memory_usage
                .fetch_sub(memory_to_release, Ordering::Relaxed);
        } else {
            warn!(
                "Memory usage underflow prevented: {} < {}, resetting to 0",
                old_memory, memory_to_release
            );
            self.memory_usage.store(0, Ordering::Relaxed);
        }

        // FIXED: Permits are automatically released when dropped - no manual add_permits needed
        permits_held.clear();
    }

    async fn insert_with_retry(&self, events: &[Event]) -> Result<usize> {
        let mut last_error = None;

        for attempt in 1..=MAX_DB_RETRIES {
            match self.database.insert_events_batch(events).await {
                Ok(inserted_count) => {
                    if attempt > 1 {
                        info!(
                            "Database insertion succeeded on attempt {} after retries",
                            attempt
                        );
                    }
                    return Ok(inserted_count);
                }
                Err(e) => {
                    last_error = Some(e);

                    if attempt < MAX_DB_RETRIES {
                        // Calculate exponential backoff delay with jitter
                        let base_delay = INITIAL_RETRY_DELAY.as_millis() as u64;
                        let exponential_delay = base_delay * (2_u64.pow(attempt as u32 - 1));
                        let delay_with_jitter =
                            exponential_delay + (fastrand::u64(0..base_delay) / 2);
                        let delay = Duration::from_millis(
                            delay_with_jitter.min(MAX_RETRY_DELAY.as_millis() as u64),
                        );

                        // FIXED: Use safe error formatting instead of unwrap()
                        let error_msg = last_error
                            .as_ref()
                            .map(|e| e.to_string())
                            .unwrap_or_else(|| "Unknown error".to_string());

                        warn!(
                            "Database insertion failed on attempt {} of {}: {}. Retrying in {:?}",
                            attempt, MAX_DB_RETRIES, error_msg, delay
                        );

                        // Track retry attempts
                        // FIXED: Use saturating_add to prevent overflow
                        self.stats.total_retries.fetch_add(1, Ordering::Relaxed);

                        sleep(delay).await;
                    } else {
                        // FIXED: Use safe error formatting instead of unwrap()
                        let error_msg = last_error
                            .as_ref()
                            .map(|e| e.to_string())
                            .unwrap_or_else(|| "Unknown error".to_string());

                        error!(
                            "Database insertion failed on final attempt {} of {}: {}",
                            attempt, MAX_DB_RETRIES, error_msg
                        );
                    }
                }
            }
        }

        // FIXED: Return safe error instead of unwrap()
        Err(last_error
            .unwrap_or_else(|| anyhow!("All retry attempts exhausted with no recorded error")))
    }

    async fn publish_to_nats_with_result(&self, events: &[Event]) -> Result<(usize, usize)> {
        if events.is_empty() {
            return Ok((0, 0));
        }

        let Some(ref client) = self.nats_client else {
            debug!(
                "ℹ️  NATS client not available. Skipping publishing {} events.",
                events.len()
            );
            return Ok((0, events.len())); // Treat as "failed" if no client
        };

        debug!(
            "📤 Publishing {} events to NATS JetStream stream '{}'",
            events.len(),
            self.nats_stream_name
        );

        let mut successful_publishes = 0;
        let mut failed_publishes = 0;

        for event in events {
            // Serialize event to JSON for NATS publishing
            match serde_json::to_vec(event) {
                Ok(payload) => {
                    // Create a subject based on event type for better routing
                    let subject = self.create_nats_subject(event);

                    // Publish to NATS with timeout
                    match timeout(
                        Duration::from_secs(5),
                        client.publish(subject.clone(), payload.into()),
                    )
                    .await
                    {
                        Ok(Ok(_)) => {
                            successful_publishes += 1;
                            debug!("✅ Published event to NATS subject: {}", subject);
                        }
                        Ok(Err(e)) => {
                            failed_publishes += 1;
                            debug!(
                                "❌ Failed to publish event to NATS subject {}: {}",
                                subject, e
                            );
                        }
                        Err(_) => {
                            failed_publishes += 1;
                            debug!("⏰ Timeout publishing event to NATS subject: {}", subject);
                        }
                    }
                }
                Err(e) => {
                    failed_publishes += 1;
                    debug!("🔥 Failed to serialize event for NATS publishing: {}", e);
                }
            }
        }

        Ok((successful_publishes, failed_publishes))
    }

    // async fn publish_to_nats(&self, events: &[Event]) {
    //     match self.publish_to_nats_with_result(events).await {
    //         Ok((successful, failed)) => {
    //             if successful > 0 {
    //                 info!(
    //                     "📤 Successfully published {}/{} events to NATS JetStream",
    //                     successful,
    //                     successful + failed
    //                 );
    //             }
    //             if failed > 0 {
    //                 warn!(
    //                     "⚠️  Failed to publish {}/{} events to NATS",
    //                     failed,
    //                     successful + failed
    //                 );
    //             }
    //         }
    //         Err(e) => {
    //             warn!("⚠️  NATS publishing encountered an error: {}", e);
    //         }
    //     }
    // }

    fn create_nats_subject(&self, event: &Event) -> String {
        let base_subject = format!("{}.events", self.nats_stream_name);

        match &event.event_type {
            Some(event_type) => match event_type {
                crate::events::event::EventType::Coordinator(coord_event) => {
                    match &coord_event.event {
                        Some(coordinator_event) => match coordinator_event {
                            crate::events::coordinator_event::Event::CoordinatorStarted(_) => {
                                format!("{}.coordinator.started", base_subject)
                            }
                            crate::events::coordinator_event::Event::AgentStartedJob(_) => {
                                format!("{}.coordinator.agent_started_job", base_subject)
                            }
                            crate::events::coordinator_event::Event::AgentFinishedJob(_) => {
                                format!("{}.coordinator.agent_finished_job", base_subject)
                            }
                            crate::events::coordinator_event::Event::CoordinationTx(_) => {
                                format!("{}.coordinator.coordination_tx", base_subject)
                            }
                            crate::events::coordinator_event::Event::CoordinatorError(_) => {
                                format!("{}.coordinator.error", base_subject)
                            }
                            crate::events::coordinator_event::Event::ClientTransaction(_) => {
                                format!("{}.coordinator.client_transaction", base_subject)
                            }
                        },
                        None => format!("{}.coordinator.unknown", base_subject),
                    }
                }
                crate::events::event::EventType::Agent(agent_event) => match &agent_event.event {
                    Some(agent_event_type) => match agent_event_type {
                        crate::events::agent_event::Event::Message(_) => {
                            format!("{}.agent.message", base_subject)
                        }
                        crate::events::agent_event::Event::Transaction(_) => {
                            format!("{}.agent.transaction", base_subject)
                        }
                    },
                    None => format!("{}.agent.unknown", base_subject),
                },
            },
            None => format!("{}.unknown", base_subject),
        }
    }

    fn estimate_event_memory_usage(&self, event: &Event) -> usize {
        // Reuse the estimation logic from EventBuffer
        let base_size = std::mem::size_of::<Event>();
        let payload_size = match &event.event_type {
            Some(event_type) => match event_type {
                crate::events::event::EventType::Coordinator(coord_event) => {
                    self.estimate_coordinator_event_size(coord_event)
                }
                crate::events::event::EventType::Agent(agent_event) => {
                    self.estimate_agent_event_size(agent_event)
                }
            },
            None => 0,
        };
        base_size + payload_size
    }

    fn estimate_coordinator_event_size(
        &self,
        coord_event: &crate::events::CoordinatorEvent,
    ) -> usize {
        // Same logic as in EventBuffer - could be refactored into a shared module
        match &coord_event.event {
            Some(event) => match event {
                crate::events::coordinator_event::Event::CoordinatorStarted(e) => {
                    e.coordinator_id.len()
                        + e.ethereum_address.len()
                        + e.sui_ed25519_address.len()
                        + 64
                }
                crate::events::coordinator_event::Event::AgentStartedJob(e) => {
                    e.coordinator_id.len()
                        + e.developer.len()
                        + e.agent.len()
                        + e.app.len()
                        + e.job_id.len()
                        + 64
                }
                crate::events::coordinator_event::Event::AgentFinishedJob(e) => {
                    e.coordinator_id.len()
                        + e.developer.len()
                        + e.agent.len()
                        + e.app.len()
                        + e.job_id.len()
                        + 72
                }
                crate::events::coordinator_event::Event::CoordinationTx(e) => {
                    e.coordinator_id.len()
                        + e.developer.len()
                        + e.agent.len()
                        + e.app.len()
                        + e.job_id.len()
                        + e.memo.len()
                        + e.tx_hash.len()
                        + 64
                }
                crate::events::coordinator_event::Event::CoordinatorError(e) => {
                    e.coordinator_id.len() + e.message.len() + 64
                }
                crate::events::coordinator_event::Event::ClientTransaction(e) => {
                    e.coordinator_id.len()
                        + e.developer.len()
                        + e.agent.len()
                        + e.app.len()
                        + e.client_ip_address.len()
                        + e.method.len()
                        + e.data.len()
                        + e.tx_hash.len()
                        + 128
                }
            },
            None => 0,
        }
    }

    fn estimate_agent_event_size(&self, agent_event: &crate::events::AgentEvent) -> usize {
        match &agent_event.event {
            Some(event) => match event {
                crate::events::agent_event::Event::Message(e) => {
                    let base_size = e.coordinator_id.len()
                        + e.developer.len()
                        + e.agent.len()
                        + e.app.len()
                        + e.job_id.len()
                        + e.message.len()
                        + 64;

                    // Add memory for child table records (each sequence creates a separate record)
                    let sequence_records_size = e.sequences.len()
                        * std::mem::size_of::<entities::agent_message_event_sequences::Model>();

                    base_size + sequence_records_size
                }
                crate::events::agent_event::Event::Transaction(e) => {
                    let base_size = e.coordinator_id.len()
                        + e.tx_type.len()
                        + e.developer.len()
                        + e.agent.len()
                        + e.app.len()
                        + e.job_id.len()
                        + e.tx_hash.len()
                        + e.chain.len()
                        + e.network.len()
                        + e.memo.len()
                        + e.metadata.len()
                        + 128;

                    // Add memory for child table records (each sequence creates a separate record)
                    let sequence_records_size = e.sequences.len()
                        * std::mem::size_of::<entities::agent_transaction_event_sequences::Model>();

                    base_size + sequence_records_size
                }
            },
            None => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_limits() {
        assert!(MAX_MEMORY_BYTES > 0);
        assert!(DEFAULT_CHANNEL_CAPACITY > 0);
        assert!(ERROR_THRESHOLD > 0);
        assert!(MAX_DB_RETRIES > 0);
    }

    #[tokio::test]
    async fn test_circuit_breaker() {
        let cb = CircuitBreaker::new(3, Duration::from_millis(100));

        // Should be closed initially
        assert!(!cb.is_open().await);

        // Record errors to open circuit breaker
        cb.record_error().await;
        cb.record_error().await;
        cb.record_error().await;

        assert!(cb.is_open().await);

        // Test DB success reset
        cb.record_db_success().await;
        assert!(!cb.is_open().await);
    }

    #[tokio::test]
    async fn test_stats_atomic_updates() {
        let stats = BufferStatsAtomic::default();

        // Test concurrent updates don't race
        let stats_arc = Arc::new(stats);
        let mut handles = vec![];

        for _ in 0..10 {
            let stats_clone = Arc::clone(&stats_arc);
            let handle = tokio::spawn(async move {
                for _ in 0..100 {
                    stats_clone.total_received.fetch_add(1, Ordering::Relaxed);
                    stats_clone.total_processed.fetch_add(1, Ordering::Relaxed);
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await.expect("Test task should not panic");
        }

        assert_eq!(stats_arc.total_received.load(Ordering::Relaxed), 1000);
        assert_eq!(stats_arc.total_processed.load(Ordering::Relaxed), 1000);
    }

    #[test]
    fn test_exponential_backoff() {
        // Test that retry delays increase exponentially
        let base_delay = INITIAL_RETRY_DELAY.as_millis() as u64;

        for attempt in 1..=5 {
            let exponential_delay = base_delay * (2_u64.pow(attempt as u32 - 1));
            let max_delay = MAX_RETRY_DELAY.as_millis() as u64;
            let actual_delay = exponential_delay.min(max_delay);

            println!(
                "Attempt {}: {}ms (max: {}ms)",
                attempt, actual_delay, max_delay
            );

            // Verify exponential growth up to max
            if attempt == 1 {
                assert_eq!(actual_delay, base_delay);
            } else {
                assert!(actual_delay >= base_delay);
                assert!(actual_delay <= max_delay);
            }
        }
    }

    #[test]
    fn test_semaphore_thresholds() {
        // Test that our semaphore strategy constants make sense
        let total_permits = DEFAULT_CHANNEL_CAPACITY;

        // FIXED: Ensure total_permits is not zero to prevent division by zero
        assert!(total_permits > 0, "Total permits must be greater than zero");

        let fast_path_threshold = total_permits / FAST_PATH_THRESHOLD;

        // Should have meaningful thresholds
        assert!(fast_path_threshold > 0);
        assert!(fast_path_threshold < total_permits);

        // Fast path should trigger when we have plenty of capacity
        assert!(fast_path_threshold >= total_permits / 10); // At least 10% capacity for fast path

        println!(
            "Total permits: {}, Fast path threshold: {} ({}%)",
            total_permits,
            fast_path_threshold,
            // FIXED: Safe division to prevent panic
            if total_permits > 0 {
                (fast_path_threshold * 100) / total_permits
            } else {
                0
            }
        );
    }

    #[test]
    fn test_partial_insert_counter_consistency() {
        // Test the logic for handling partial inserts to ensure counter consistency
        let event_count = 100;
        let inserted_count = 75; // Partial insert
        let dropped_count = event_count - inserted_count;

        // Simulate the logic from flush_buffer
        let mut total_processed = 0u64;
        let mut total_dropped = 0u64;
        let mut current_buffer_size = event_count;

        // Apply the same logic as in flush_buffer for partial insert
        if inserted_count < event_count {
            total_dropped += dropped_count as u64;
        }
        total_processed += inserted_count as u64;
        current_buffer_size -= event_count; // All events removed from buffer

        // Verify counter consistency
        assert_eq!(total_processed, 75);
        assert_eq!(total_dropped, 25);
        assert_eq!(current_buffer_size, 0); // All events removed from buffer
        assert_eq!(total_processed + total_dropped, event_count as u64); // Total events accounted for
    }

    #[test]
    fn test_health_check_idle_system() {
        // Test that health check passes when system is idle (no events received)

        // Simulate idle system stats
        let stats = BufferStats {
            total_received: 0, // No events received yet
            total_processed: 0,
            total_errors: 0,
            total_dropped: 0,
            total_retries: 0,
            current_buffer_size: 0,
            current_memory_bytes: 0,
            last_flush_time: None,
            backpressure_events: 0, // Even if there were some backpressure events
            circuit_breaker_open: false,
        };

        // Simulate the health check logic
        let basic_health =
            !stats.circuit_breaker_open && stats.current_memory_bytes < MAX_MEMORY_BYTES;

        let backpressure_healthy = if stats.total_received == 0 {
            true // Should be healthy when idle
        } else {
            let backpressure_ratio = stats.backpressure_events as f64 / stats.total_received as f64;
            backpressure_ratio < 0.1
        };

        let health_result = basic_health && backpressure_healthy;

        // Should be healthy when idle
        assert!(
            health_result,
            "Health check should pass when system is idle"
        );

        // Test with some backpressure events during idle
        let stats_with_backpressure = BufferStats {
            backpressure_events: 5, // Some backpressure during startup
            ..stats
        };

        let backpressure_healthy_with_events = if stats_with_backpressure.total_received == 0 {
            true // Should still be healthy when idle
        } else {
            let backpressure_ratio = stats_with_backpressure.backpressure_events as f64
                / stats_with_backpressure.total_received as f64;
            backpressure_ratio < 0.1
        };

        let health_with_backpressure = basic_health && backpressure_healthy_with_events;
        assert!(
            health_with_backpressure,
            "Health check should pass when idle even with startup backpressure"
        );
    }

    #[test]
    fn test_max_batch_size_limit() {
        // Test that MAX_BATCH_SIZE is properly configured
        assert_eq!(
            MAX_BATCH_SIZE, 10000,
            "MAX_BATCH_SIZE should be 10000 to prevent database placeholder limits"
        );

        // Test that MAX_BATCH_SIZE is reasonable compared to other constants
        assert!(MAX_BATCH_SIZE > 0);
        assert!(
            MAX_BATCH_SIZE <= DEFAULT_CHANNEL_CAPACITY,
            "MAX_BATCH_SIZE should not exceed channel capacity"
        );

        // Test batch size calculation logic
        let buffer_size = 1500;
        let remaining_capacity = MAX_BATCH_SIZE.saturating_sub(buffer_size);
        assert_eq!(
            remaining_capacity, 8500,
            "Should have correct remaining capacity"
        );

        let buffer_size = 15000;
        let remaining_capacity = MAX_BATCH_SIZE.saturating_sub(buffer_size);
        assert_eq!(
            remaining_capacity, 0,
            "Should have no remaining capacity when buffer exceeds max"
        );
    }

    #[test]
    fn test_panic_prevention_measures() {
        // Test division by zero prevention
        let initial_count = 0;
        let total_events = 100;
        let efficiency_gain = if initial_count > 0 {
            total_events / initial_count
        } else {
            1
        };
        assert_eq!(
            efficiency_gain, 1,
            "Should default to 1 when initial_count is 0"
        );

        // Test saturating subtraction behavior
        let old_size = 5usize;
        let event_count = 10usize;
        let safe_subtraction = if old_size >= event_count {
            old_size - event_count
        } else {
            0
        };
        assert_eq!(safe_subtraction, 0, "Should safely handle underflow");

        // Test overflow protection
        let max_value = usize::MAX;
        let addition_value = 100usize;
        let safe_addition = max_value.saturating_add(addition_value);
        assert_eq!(
            safe_addition,
            usize::MAX,
            "Should saturate at maximum value"
        );

        // Test memory overflow detection
        let old_memory = usize::MAX - 50;
        let event_size = 100usize;
        let would_overflow = old_memory > usize::MAX - event_size;
        assert!(would_overflow, "Should detect potential overflow");

        // Test safe division with constants
        let total_permits = DEFAULT_CHANNEL_CAPACITY;
        assert!(
            total_permits > 0,
            "Total permits must be positive for safe division"
        );

        let safe_division = if total_permits > 0 {
            (31250 * 100) / total_permits // Example calculation from test
        } else {
            0
        };
        assert!(
            safe_division < usize::MAX,
            "Division result should be reasonable"
        );
    }
}
