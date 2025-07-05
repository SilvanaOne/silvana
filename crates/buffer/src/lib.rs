//! Generic Event Buffer with Pluggable Backends
//!
//! This crate provides a high-performance event buffering system that can be used
//! by different components (RPC, Coordinator) with different backend implementations.
//!
//! # Architecture
//! - RPC: Events → Buffer → TiDB + NATS
//! - Coordinator: Events → Buffer → RPC gRPC client

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use tokio::sync::{OwnedSemaphorePermit, RwLock, Semaphore, mpsc};
use tokio::time::{Duration, Instant, interval, sleep, timeout};
use tracing::{debug, error, info, warn};

// Note: Backend implementations have been moved to their respective domain crates:
// - TiDB backend: tidb crate
// - NATS publisher: nats crate

// Configuration constants
const DEFAULT_BATCH_SIZE: usize = 100;
const DEFAULT_FLUSH_INTERVAL: Duration = Duration::from_millis(500);
const DEFAULT_CHANNEL_CAPACITY: usize = 250000;
const MAX_MEMORY_BYTES: usize = 1000 * 1024 * 1024; // 1GB
const BASE_ADD_EVENT_TIMEOUT: Duration = Duration::from_millis(100);
const ERROR_THRESHOLD: usize = 100;
const MAX_RETRIES: usize = 10;
const INITIAL_RETRY_DELAY: Duration = Duration::from_millis(100);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(30);
const MAX_BATCH_SIZE: usize = 10000;
const FAST_PATH_THRESHOLD: usize = 8;

/// Generic trait for event types that can be buffered
pub trait BufferableEvent: Send + Sync + Clone + 'static {
    /// Estimate the memory size of this event
    fn estimate_size(&self) -> usize;

    /// Get a string identifier for this event type (for logging/debugging)
    fn event_type_name(&self) -> &'static str;
}

/// Backend trait for processing batches of events
#[async_trait]
pub trait EventBackend<T: BufferableEvent>: Send + Sync {
    /// Process a batch of events
    /// Returns the number of successfully processed events
    async fn process_batch(&self, events: &[T]) -> Result<usize>;

    /// Optional: Get backend name for logging
    fn backend_name(&self) -> &'static str {
        "Unknown"
    }
}

/// Generic publisher trait for secondary event distribution
#[async_trait]
pub trait EventPublisher<T: BufferableEvent>: Send + Sync {
    /// Publish events to external system (e.g., NATS, Kafka, etc.)
    /// Returns (successful_count, failed_count)
    async fn publish_batch(&self, events: &[T]) -> Result<(usize, usize)>;
}

// Wrapper for events with their semaphore permits
type EventWithPermit<T> = (T, OwnedSemaphorePermit);

#[derive(Clone)]
pub struct EventBuffer<T: BufferableEvent> {
    sender: mpsc::Sender<EventWithPermit<T>>,
    stats: Arc<BufferStatsAtomic>,
    memory_usage: Arc<AtomicUsize>,
    circuit_breaker: Arc<CircuitBreaker>,
    backpressure_semaphore: Arc<Semaphore>,
    add_event_timeout: Duration,
    total_permits: usize,
}

/// Atomic counters for lock-free stats updates
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

/// Snapshot for reading stats
#[derive(Debug, Default, Clone)]
pub struct BufferStats {
    pub total_received: u64,
    pub total_processed: u64,
    pub total_errors: u64,
    pub total_dropped: u64,
    pub total_retries: u64,
    pub current_buffer_size: usize,
    pub current_memory_bytes: usize,
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

struct BatchProcessor<T: BufferableEvent> {
    receiver: mpsc::Receiver<EventWithPermit<T>>,
    buffer: Vec<T>,
    backend: Arc<dyn EventBackend<T>>,
    event_publisher: Option<Arc<dyn EventPublisher<T>>>,
    stats: Arc<BufferStatsAtomic>,
    memory_usage: Arc<AtomicUsize>,
    circuit_breaker: Arc<CircuitBreaker>,
    batch_size: usize,
    flush_interval: Duration,
}

impl<T: BufferableEvent> EventBuffer<T> {
    pub fn new(backend: Arc<dyn EventBackend<T>>) -> Self {
        Self::with_config(
            backend,
            None,
            DEFAULT_BATCH_SIZE,
            DEFAULT_FLUSH_INTERVAL,
            DEFAULT_CHANNEL_CAPACITY,
        )
    }

    pub fn with_publisher(
        backend: Arc<dyn EventBackend<T>>,
        event_publisher: Arc<dyn EventPublisher<T>>,
    ) -> Self {
        Self::with_config(
            backend,
            Some(event_publisher),
            DEFAULT_BATCH_SIZE,
            DEFAULT_FLUSH_INTERVAL,
            DEFAULT_CHANNEL_CAPACITY,
        )
    }

    pub fn with_config(
        backend: Arc<dyn EventBackend<T>>,
        event_publisher: Option<Arc<dyn EventPublisher<T>>>,
        batch_size: usize,
        flush_interval: Duration,
        channel_capacity: usize,
    ) -> Self {
        let (sender, receiver) = mpsc::channel(channel_capacity);
        let stats = Arc::new(BufferStatsAtomic::default());
        let memory_usage = Arc::new(AtomicUsize::new(0));
        let circuit_breaker = Arc::new(CircuitBreaker::new(
            ERROR_THRESHOLD,
            Duration::from_secs(60),
        ));
        let backpressure_semaphore = Arc::new(Semaphore::new(channel_capacity));

        let add_event_timeout = std::cmp::max(BASE_ADD_EVENT_TIMEOUT, flush_interval / 10);

        // Clone Arc references for the spawned task
        let stats_clone = Arc::clone(&stats);
        let memory_usage_clone = Arc::clone(&memory_usage);
        let circuit_breaker_clone = Arc::clone(&circuit_breaker);

        // Spawn the processor task
        tokio::spawn(async move {
            let processor = BatchProcessor::new(
                receiver,
                backend,
                event_publisher,
                stats_clone,
                memory_usage_clone,
                circuit_breaker_clone,
                batch_size,
                flush_interval,
            );

            processor.run().await;
        });

        Self {
            sender,
            stats,
            memory_usage,
            circuit_breaker,
            backpressure_semaphore,
            add_event_timeout,
            total_permits: channel_capacity,
        }
    }

    pub async fn add_event(&self, event: T) -> Result<()> {
        // Check circuit breaker
        if self.circuit_breaker.is_open().await {
            self.stats.total_dropped.fetch_add(1, Ordering::Relaxed);
            return Err(anyhow!("Circuit breaker is open - system overloaded"));
        }

        // Estimate event size
        let event_size = event.estimate_size();

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
        let available_permits = self.backpressure_semaphore.available_permits();
        let total_permits = self.total_permits;

        let permit = if available_permits > total_permits / FAST_PATH_THRESHOLD {
            // Fast path: plenty of capacity
            match self.backpressure_semaphore.clone().try_acquire_owned() {
                Ok(permit) => permit,
                Err(_) => {
                    self.stats
                        .backpressure_events
                        .fetch_add(1, Ordering::Relaxed);
                    self.stats.total_dropped.fetch_add(1, Ordering::Relaxed);
                    return Err(anyhow!("Backpressure active - buffer at capacity"));
                }
            }
        } else if available_permits > 0 {
            // Near capacity: try fast path first, then fall back to blocking
            match self.backpressure_semaphore.clone().try_acquire_owned() {
                Ok(permit) => permit,
                Err(_) => {
                    match timeout(
                        self.add_event_timeout,
                        self.backpressure_semaphore.clone().acquire_owned(),
                    )
                    .await
                    {
                        Ok(Ok(permit)) => permit,
                        Ok(Err(_)) => {
                            self.stats
                                .backpressure_events
                                .fetch_add(1, Ordering::Relaxed);
                            self.stats.total_dropped.fetch_add(1, Ordering::Relaxed);
                            return Err(anyhow!("Backpressure semaphore closed"));
                        }
                        Err(_) => {
                            self.stats
                                .backpressure_events
                                .fetch_add(1, Ordering::Relaxed);
                            self.stats.total_dropped.fetch_add(1, Ordering::Relaxed);
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
            return Err(anyhow!("Backpressure active - buffer at capacity"));
        };

        // Try to send with timeout
        let wrapped_event = (event, permit);
        match timeout(self.add_event_timeout, self.sender.send(wrapped_event)).await {
            Ok(Ok(())) => {
                self.stats.total_received.fetch_add(1, Ordering::Relaxed);
                self.stats
                    .current_buffer_size
                    .fetch_add(1, Ordering::Relaxed);
                self.memory_usage.fetch_add(event_size, Ordering::Relaxed);
                self.circuit_breaker.record_success().await;
                Ok(())
            }
            Ok(Err(_)) => {
                self.stats.total_dropped.fetch_add(1, Ordering::Relaxed);
                Err(anyhow!("Event buffer channel closed"))
            }
            Err(_) => {
                self.stats.total_dropped.fetch_add(1, Ordering::Relaxed);
                self.stats
                    .backpressure_events
                    .fetch_add(1, Ordering::Relaxed);
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
        let basic_health =
            !stats.circuit_breaker_open && stats.current_memory_bytes < MAX_MEMORY_BYTES;

        let backpressure_healthy = if stats.total_received == 0 {
            true
        } else {
            let backpressure_ratio = stats.backpressure_events as f64 / stats.total_received as f64;
            backpressure_ratio < 0.1
        };

        basic_health && backpressure_healthy
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
        let count = self.error_count.fetch_add(1, Ordering::Relaxed) + 1;
        *self.last_error_time.write().await = Some(Instant::now());

        if count >= self.threshold {
            warn!("Circuit breaker opened - too many errors: {}", count);
            self.is_open.store(true, Ordering::Relaxed);
        }
    }

    async fn record_success(&self) {
        if self.error_count.load(Ordering::Relaxed) > 0 {
            self.error_count.store(0, Ordering::Relaxed);
        }
    }

    async fn record_backend_success(&self) {
        if self.is_open.load(Ordering::Relaxed) {
            info!("Circuit breaker reset after successful backend operation");
            self.is_open.store(false, Ordering::Relaxed);
        }
        self.error_count.store(0, Ordering::Relaxed);
    }
}

impl<T: BufferableEvent> BatchProcessor<T> {
    fn new(
        receiver: mpsc::Receiver<EventWithPermit<T>>,
        backend: Arc<dyn EventBackend<T>>,
        event_publisher: Option<Arc<dyn EventPublisher<T>>>,
        stats: Arc<BufferStatsAtomic>,
        memory_usage: Arc<AtomicUsize>,
        circuit_breaker: Arc<CircuitBreaker>,
        batch_size: usize,
        flush_interval: Duration,
    ) -> Self {
        Self {
            receiver,
            buffer: Vec::with_capacity(batch_size),
            backend,
            event_publisher,
            stats,
            memory_usage,
            circuit_breaker,
            batch_size,
            flush_interval,
        }
    }

    async fn run(mut self) {
        let mut flush_timer = interval(self.flush_interval);
        let mut permits_held: Vec<OwnedSemaphorePermit> = Vec::new();

        info!(
            "Started batch processor with backend={}, batch_size={}, flush_interval={:?}",
            self.backend.backend_name(),
            self.batch_size,
            self.flush_interval
        );

        if self.event_publisher.is_some() {
            info!("Event publishing enabled");
        } else {
            info!("Event publishing disabled");
        }

        loop {
            tokio::select! {
                event_result = self.receiver.recv() => {
                    match event_result {
                        Some((event, permit)) => {
                            self.buffer.push(event);
                            permits_held.push(permit);

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

                _ = flush_timer.tick() => {
                    if !self.buffer.is_empty() {
                        debug!("Periodic flush triggered with {} events", self.buffer.len());
                        self.drain_and_flush(&mut permits_held).await;
                    }
                }
            }
        }
    }

    async fn drain_and_flush(&mut self, permits_held: &mut Vec<OwnedSemaphorePermit>) {
        //let initial_count = self.buffer.len();
        //let mut total_drained = 0;
        let mut batch_number = 1;

        loop {
            let mut drained_this_batch = 0;
            let remaining_capacity = MAX_BATCH_SIZE.saturating_sub(self.buffer.len());

            if remaining_capacity == 0 {
                debug!(
                    "Batch {} reached max size limit ({}), flushing immediately",
                    batch_number, MAX_BATCH_SIZE
                );
                self.flush_buffer(permits_held).await;
                batch_number += 1;
                continue;
            }

            for _ in 0..remaining_capacity {
                match self.receiver.try_recv() {
                    Ok((event, permit)) => {
                        self.buffer.push(event);
                        permits_held.push(permit);
                        drained_this_batch += 1;
                        //total_drained += 1;
                    }
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                        warn!("Event channel disconnected during drain");
                        break;
                    }
                }
            }

            if !self.buffer.is_empty()
                && (drained_this_batch == 0 || self.buffer.len() >= MAX_BATCH_SIZE)
            {
                self.flush_buffer(permits_held).await;
                batch_number += 1;

                if drained_this_batch == 0 {
                    break;
                }
            } else if drained_this_batch == 0 {
                break;
            }
        }
    }

    async fn flush_buffer(&mut self, permits_held: &mut Vec<OwnedSemaphorePermit>) {
        if self.buffer.is_empty() {
            return;
        }

        let events_to_process = std::mem::take(&mut self.buffer);
        let event_count = events_to_process.len();

        debug!(
            "Flushing batch of {} events to backend: {}",
            event_count,
            self.backend.backend_name()
        );

        let memory_to_release = events_to_process
            .iter()
            .map(|event| event.estimate_size())
            .sum::<usize>();

        // Process backend and event publishing in parallel (if event publisher is enabled)
        let start_time = Instant::now();
        let (backend_result, publish_result) =
            if let Some(ref event_publisher) = self.event_publisher {
                let backend_start = Instant::now();
                let publish_start = Instant::now();

                let backend_future = async {
                    let result = self.process_with_retry(&events_to_process).await;
                    let backend_duration = backend_start.elapsed();
                    debug!(
                        "⏱️ Backend processing completed in {:?} for {} events ({})",
                        backend_duration,
                        event_count,
                        self.backend.backend_name()
                    );
                    (result, backend_duration)
                };

                let publish_future = async {
                    let result = event_publisher.publish_batch(&events_to_process).await;
                    let publish_duration = publish_start.elapsed();
                    debug!(
                        "⏱️ Event publishing completed in {:?} for {} events",
                        publish_duration, event_count
                    );
                    (result, publish_duration)
                };

                let ((backend_result, backend_duration), (publish_result, publish_duration)) =
                    tokio::join!(backend_future, publish_future);
                let total_duration = start_time.elapsed();

                debug!(
                    "⏱️ Parallel processing summary: Backend={:?}, Publish={:?}, Total={:?}",
                    backend_duration, publish_duration, total_duration,
                );

                (backend_result, publish_result)
            } else {
                let backend_start = Instant::now();
                let backend_result = self.process_with_retry(&events_to_process).await;
                let backend_duration = backend_start.elapsed();
                let total_duration = start_time.elapsed();

                debug!(
                    "⏱️ Backend-only processing completed in {:?} for {} events ({})",
                    backend_duration,
                    event_count,
                    self.backend.backend_name()
                );
                debug!("⏱️ Total processing time: {:?}", total_duration,);

                (backend_result, Ok((0, 0))) // No event publishing
            };

        // Handle backend result
        match backend_result {
            Ok(processed_count) => {
                debug!(
                    "Successfully processed {} events via backend: {}",
                    processed_count,
                    self.backend.backend_name()
                );

                if processed_count < event_count {
                    let dropped_count = event_count - processed_count;
                    warn!(
                        "Partial processing: {} of {} events processed, {} events dropped",
                        processed_count, event_count, dropped_count
                    );
                    self.stats
                        .total_dropped
                        .fetch_add(dropped_count as u64, Ordering::Relaxed);
                }

                self.stats
                    .total_processed
                    .fetch_add(processed_count as u64, Ordering::Relaxed);
                let old_size = self.stats.current_buffer_size.load(Ordering::Relaxed);
                if old_size >= event_count {
                    self.stats
                        .current_buffer_size
                        .fetch_sub(event_count, Ordering::Relaxed);
                } else {
                    self.stats.current_buffer_size.store(0, Ordering::Relaxed);
                }

                *self.stats.last_flush_time.write().await = Some(Instant::now());
                self.circuit_breaker.record_backend_success().await;
            }
            Err(e) => {
                error!(
                    "Failed to process events via backend {} after {} retries: {}",
                    self.backend.backend_name(),
                    MAX_RETRIES,
                    e
                );
                self.circuit_breaker.record_error().await;
                self.stats
                    .total_errors
                    .fetch_add(event_count as u64, Ordering::Relaxed);
                self.stats
                    .total_dropped
                    .fetch_add(event_count as u64, Ordering::Relaxed);

                let old_size = self.stats.current_buffer_size.load(Ordering::Relaxed);
                if old_size >= event_count {
                    self.stats
                        .current_buffer_size
                        .fetch_sub(event_count, Ordering::Relaxed);
                } else {
                    self.stats.current_buffer_size.store(0, Ordering::Relaxed);
                }
            }
        }

        // Handle event publishing result (if enabled)
        if self.event_publisher.is_some() {
            match publish_result {
                Ok((successful, failed)) => {
                    if successful > 0 {
                        debug!(
                            "📤 Successfully published {}/{} events",
                            successful,
                            successful + failed
                        );
                    }
                    if failed > 0 {
                        error!(
                            "⚠️ Failed to publish {}/{} events",
                            failed,
                            successful + failed
                        );
                    }
                }
                Err(e) => {
                    error!("⚠️ Event publishing encountered an error: {}", e);
                }
            }
        }

        // Release memory accounting
        let old_memory = self.memory_usage.load(Ordering::Relaxed);
        if old_memory >= memory_to_release {
            self.memory_usage
                .fetch_sub(memory_to_release, Ordering::Relaxed);
        } else {
            self.memory_usage.store(0, Ordering::Relaxed);
        }

        permits_held.clear();
    }

    async fn process_with_retry(&self, events: &[T]) -> Result<usize> {
        let mut last_error = None;

        for attempt in 1..=MAX_RETRIES {
            match self.backend.process_batch(events).await {
                Ok(processed_count) => {
                    if attempt > 1 {
                        info!(
                            "Backend processing succeeded on attempt {} after retries",
                            attempt
                        );
                    }
                    return Ok(processed_count);
                }
                Err(e) => {
                    last_error = Some(e);

                    if attempt < MAX_RETRIES {
                        let base_delay = INITIAL_RETRY_DELAY.as_millis() as u64;
                        let exponential_delay = base_delay * (2_u64.pow(attempt as u32 - 1));
                        let delay_with_jitter =
                            exponential_delay + (fastrand::u64(0..base_delay) / 2);
                        let delay = Duration::from_millis(
                            delay_with_jitter.min(MAX_RETRY_DELAY.as_millis() as u64),
                        );

                        warn!(
                            "Backend processing failed on attempt {} of {}: {}. Retrying in {:?}",
                            attempt,
                            MAX_RETRIES,
                            last_error.as_ref().unwrap(),
                            delay
                        );
                        self.stats.total_retries.fetch_add(1, Ordering::Relaxed);
                        sleep(delay).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow!("All retry attempts exhausted")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug)]
    struct TestEvent {
        data: String,
    }

    impl BufferableEvent for TestEvent {
        fn estimate_size(&self) -> usize {
            self.data.len() + std::mem::size_of::<Self>()
        }

        fn event_type_name(&self) -> &'static str {
            "TestEvent"
        }
    }

    struct TestBackend;

    #[async_trait]
    impl EventBackend<TestEvent> for TestBackend {
        async fn process_batch(&self, events: &[TestEvent]) -> Result<usize> {
            Ok(events.len())
        }

        fn backend_name(&self) -> &'static str {
            "TestBackend"
        }
    }

    #[tokio::test]
    async fn test_buffer_creation() {
        let backend = Arc::new(TestBackend);
        let buffer = EventBuffer::new(backend);

        let stats = buffer.get_stats().await;
        assert_eq!(stats.total_received, 0);
        assert_eq!(stats.total_processed, 0);
    }

    #[tokio::test]
    async fn test_event_processing() {
        let backend = Arc::new(TestBackend);
        let buffer = EventBuffer::new(backend);

        let event = TestEvent {
            data: "test data".to_string(),
        };

        let result = buffer.add_event(event).await;
        assert!(result.is_ok());

        let stats = buffer.get_stats().await;
        assert_eq!(stats.total_received, 1);
    }
}
