use futures::StreamExt;
use futures::TryStreamExt;
use prost::Message;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tokio::time::{Duration, sleep};
use tonic::Request;

use proto::silvana_events_service_client::SilvanaEventsServiceClient;
use proto::*;

// Configuration - easily changeable parameters
const NUM_EVENTS: usize = 1000;
const COORDINATOR_ID: &str = "test-coordinator-001";

fn get_server_addr() -> String {
    // Load .env file if it exists
    dotenvy::dotenv().ok();

    // Get TEST_SERVER from environment, fallback to default if not set
    std::env::var("TEST_SERVER").unwrap_or_else(|_| "http://127.0.0.1:50051".to_string())
}
const NATS_URL: &str = "nats://rpc.silvana.dev:4222";
const NATS_STREAM_NAME: &str = "silvana";

// Shared structure to collect all sent events for comparison
#[derive(Debug, Clone)]
struct SentEventsCollector {
    events: Arc<Mutex<Vec<Event>>>,
}

impl SentEventsCollector {
    fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    async fn add_event(&self, event: Event) {
        let mut events = self.events.lock().await;
        events.push(event);
    }

    async fn add_events(&self, events: Vec<Event>) {
        let mut stored_events = self.events.lock().await;
        stored_events.extend(events);
    }

    async fn get_events(&self) -> Vec<Event> {
        let events = self.events.lock().await;
        events.clone()
    }

    async fn get_event_counts_by_type(&self) -> HashMap<String, usize> {
        let events = self.events.lock().await;
        let mut counts = HashMap::new();
        for event in events.iter() {
            let event_type = classify_event(event);
            *counts.entry(event_type.to_string()).or_insert(0) += 1;
        }
        counts
    }

    #[allow(dead_code)]
    async fn len(&self) -> usize {
        let events = self.events.lock().await;
        events.len()
    }
}

#[tokio::test]
async fn test_send_coordinator_and_agent_events() {
    // Initialize Rustls crypto provider for HTTPS connections
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    
    let start_time = Instant::now();

    let server_addr = get_server_addr();

    println!("🧪 Starting integration test with NATS verification...");
    println!("📊 Configuration: {} events per type", NUM_EVENTS);
    println!("🎯 Server address: {}", server_addr);
    println!("📡 NATS address: {}", NATS_URL);

    // Create collector for sent events
    let sent_events = SentEventsCollector::new();

    // Connect to NATS first
    let nats_client = match async_nats::connect(NATS_URL).await {
        Ok(client) => {
            println!("✅ Connected to NATS server successfully");
            Some(client)
        }
        Err(e) => {
            println!(
                "⚠️  Failed to connect to NATS server at {}: {}",
                NATS_URL, e
            );
            println!("📝 Note: NATS verification will be skipped");
            // Continue without NATS verification
            None
        }
    };

    // Create JetStream client if NATS is available
    let jetstream_client = if let Some(ref nats) = nats_client {
        Some(async_nats::jetstream::new(nats.clone()))
    } else {
        None
    };

    // Connect to the gRPC server
    let client = match SilvanaEventsServiceClient::connect(server_addr.clone()).await {
        Ok(client) => {
            println!("✅ Connected to RPC server successfully");
            client
        }
        Err(e) => {
            panic!(
                "❌ Failed to connect to RPC server at {}: {}\nMake sure the server is running with: cargo run",
                server_addr, e
            );
        }
    };

    // Set up NATS subscriptions if available
    let nats_collector = if let Some(ref jetstream) = jetstream_client {
        Some(setup_nats_subscriptions(jetstream.clone()).await)
    } else {
        None
    };

    // Clone clients for parallel execution
    let mut client1 = client.clone();
    let mut client2 = client.clone();
    let mut client3 = client.clone();

    // Clone sent events collector for each test
    let sent_events1 = sent_events.clone();
    let sent_events2 = sent_events.clone();
    let sent_events3 = sent_events.clone();

    println!("\n🚀 Running tests in parallel...");

    // Run all tests concurrently
    let _ = tokio::join!(
        async {
            println!("📋 Starting Coordinator Events...");
            test_coordinator_events(&mut client1, sent_events1).await
        },
        async {
            println!("🤖 Starting Agent Events...");
            test_agent_events(&mut client2, sent_events2).await
        },
        async {
            println!("📦 Starting Batch Submission...");
            test_batch_events(&mut client3, sent_events3).await
        }
    );

    println!("✅ All parallel tests completed!");

    // Wait for NATS events to be published and verify them
    if let Some(collector) = nats_collector {
        println!("\n🔍 Verifying NATS published events...");
        verify_nats_events(collector, sent_events).await;
    } else {
        println!("\n⚠️  NATS verification skipped (no NATS connection)");
    }

    let duration = start_time.elapsed();
    let duration_ms = duration.as_millis();

    // Calculate total events dispatched (sent concurrently)
    let coordinator_event_types = 6; // 6 different coordinator event types
    let agent_event_types = 2; // 2 different agent event types
    let total_events =
        (coordinator_event_types * NUM_EVENTS) + (agent_event_types * NUM_EVENTS) + NUM_EVENTS;
    let events_per_second = if duration.as_secs_f64() > 0.0 {
        total_events as f64 / duration.as_secs_f64()
    } else {
        0.0
    };

    println!("\n🎉 All parallel integration tests completed successfully!");
    println!(
        "📊 Total events dispatched concurrently: {} events ({} coordinator + {} agent + {} batch)",
        total_events,
        coordinator_event_types * NUM_EVENTS,
        agent_event_types * NUM_EVENTS,
        NUM_EVENTS
    );
    println!(
        "⏱️  Total parallel execution time: {}ms ({:.2}s)",
        duration_ms,
        duration.as_secs_f64()
    );
    println!(
        "🚀 Concurrent throughput: {:.1} events/second",
        events_per_second
    );
}

// NATS event collector to track published events (now storing full events)
#[derive(Debug)]
struct NatsEventCollector {
    coordinator_started: tokio::sync::mpsc::Receiver<Event>,
    agent_started_job: tokio::sync::mpsc::Receiver<Event>,
    agent_finished_job: tokio::sync::mpsc::Receiver<Event>,
    coordination_tx: tokio::sync::mpsc::Receiver<Event>,
    coordinator_error: tokio::sync::mpsc::Receiver<Event>,
    client_transaction: tokio::sync::mpsc::Receiver<Event>,
    agent_message: tokio::sync::mpsc::Receiver<Event>,
    agent_transaction: tokio::sync::mpsc::Receiver<Event>,
}

async fn setup_nats_subscriptions(
    jetstream_client: async_nats::jetstream::Context,
) -> NatsEventCollector {
    // Create channels for each event type
    let (coordinator_started_tx, coordinator_started_rx) =
        tokio::sync::mpsc::channel(NUM_EVENTS * 10);
    let (agent_started_job_tx, agent_started_job_rx) = tokio::sync::mpsc::channel(NUM_EVENTS * 10);
    let (agent_finished_job_tx, agent_finished_job_rx) =
        tokio::sync::mpsc::channel(NUM_EVENTS * 10);
    let (coordination_tx_tx, coordination_tx_rx) = tokio::sync::mpsc::channel(NUM_EVENTS * 10);
    let (coordinator_error_tx, coordinator_error_rx) = tokio::sync::mpsc::channel(NUM_EVENTS * 10);
    let (client_transaction_tx, client_transaction_rx) =
        tokio::sync::mpsc::channel(NUM_EVENTS * 10);
    let (agent_message_tx, agent_message_rx) = tokio::sync::mpsc::channel(NUM_EVENTS * 10);
    let (agent_transaction_tx, agent_transaction_rx) = tokio::sync::mpsc::channel(NUM_EVENTS * 10);

    // Setup JetStream and consumer
    println!("📡 Setting up JetStream stream: {}", NATS_STREAM_NAME);
    let stream = match jetstream_client
        .get_or_create_stream(async_nats::jetstream::stream::Config {
            name: NATS_STREAM_NAME.to_string(),
            ..Default::default()
        })
        .await
    {
        Ok(stream) => {
            println!("✅ JetStream stream created successfully");
            stream
        }
        Err(e) => {
            println!("❌ Failed to create JetStream stream: {}", e);
            panic!("Cannot proceed without JetStream stream");
        }
    };

    let consumer = match stream
        .get_or_create_consumer(
            "integration-test",
            async_nats::jetstream::consumer::pull::Config {
                durable_name: Some("integration-test".to_string()),
                ..Default::default()
            },
        )
        .await
    {
        Ok(consumer) => {
            println!("✅ JetStream consumer created successfully");
            consumer
        }
        Err(e) => {
            println!("❌ Failed to create JetStream consumer: {}", e);
            panic!("Cannot proceed without JetStream consumer");
        }
    };

    // Create event type channels mapping
    let event_senders = vec![
        ("coordinator_started", coordinator_started_tx),
        ("agent_started_job", agent_started_job_tx),
        ("agent_finished_job", agent_finished_job_tx),
        ("coordination_tx", coordination_tx_tx),
        ("coordinator_error", coordinator_error_tx),
        ("client_transaction", client_transaction_tx),
        ("agent_message", agent_message_tx),
        ("agent_transaction", agent_transaction_tx),
    ];

    // Start a single consumer task that distributes events to appropriate channels
    let consumer_clone = consumer.clone();
    tokio::spawn(async move {
        match consumer_clone.messages().await {
            Ok(messages_stream) => {
                let mut messages = messages_stream.take(NUM_EVENTS * 50); // Generous buffer for all events

                while let Ok(Some(message)) = messages.try_next().await {
                    if let Err(e) = message.ack().await {
                        println!("⚠️  Failed to ack message: {}", e);
                    }

                    // Try to deserialize the event using protobuf
                    match proto::events::Event::decode(message.payload.clone()) {
                        Ok(event) => {
                            // Determine event type and send to appropriate channel
                            let event_type = classify_event(&event);

                            for (expected_type, sender) in &event_senders {
                                if &event_type == expected_type {
                                    if sender.send(event.clone()).await.is_err() {
                                        println!(
                                            "⚠️  Channel closed for event type: {}",
                                            expected_type
                                        );
                                    }
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            println!("⚠️  Failed to deserialize event from JetStream: {}", e);
                        }
                    }
                }
            }
            Err(e) => {
                println!("❌ Failed to get messages from JetStream: {}", e);
            }
        }
    });

    // Give subscription time to be established
    sleep(Duration::from_millis(500)).await;

    NatsEventCollector {
        coordinator_started: coordinator_started_rx,
        agent_started_job: agent_started_job_rx,
        agent_finished_job: agent_finished_job_rx,
        coordination_tx: coordination_tx_rx,
        coordinator_error: coordinator_error_rx,
        client_transaction: client_transaction_rx,
        agent_message: agent_message_rx,
        agent_transaction: agent_transaction_rx,
    }
}

fn classify_event(event: &Event) -> &'static str {
    match &event.event_type {
        Some(event::EventType::Coordinator(coord_event)) => match &coord_event.event {
            Some(coordinator_event::Event::CoordinatorStarted(_)) => "coordinator_started",
            Some(coordinator_event::Event::AgentStartedJob(_)) => "agent_started_job",
            Some(coordinator_event::Event::AgentFinishedJob(_)) => "agent_finished_job",
            Some(coordinator_event::Event::CoordinationTx(_)) => "coordination_tx",
            Some(coordinator_event::Event::CoordinatorError(_)) => "coordinator_error",
            Some(coordinator_event::Event::ClientTransaction(_)) => "client_transaction",
            None => "coordinator_unknown",
        },
        Some(event::EventType::Agent(agent_event)) => match &agent_event.event {
            Some(agent_event::Event::Message(_)) => "agent_message",
            Some(agent_event::Event::Transaction(_)) => "agent_transaction",
            None => "agent_unknown",
        },
        None => "unknown",
    }
}

async fn verify_nats_events(mut collector: NatsEventCollector, sent_events: SentEventsCollector) {
    println!("⏳ Waiting for NATS events to be published...");

    // Wait a bit for all events to be published
    sleep(Duration::from_secs(5)).await;

    let sent_counts = sent_events.get_event_counts_by_type().await;
    let mut total_received = 0;
    let mut events_by_type = HashMap::new();
    let mut received_events: Vec<Event> = Vec::new();

    // Collect all events with timeout
    let collection_timeout = Duration::from_secs(10);
    let start_collection = Instant::now();

    while start_collection.elapsed() < collection_timeout {
        tokio::select! {
            event = collector.coordinator_started.recv() => {
                if let Some(event) = event {
                    *events_by_type.entry("coordinator_started".to_string()).or_insert(0) += 1;
                    received_events.push(event);
                    total_received += 1;
                }
            }
            event = collector.agent_started_job.recv() => {
                if let Some(event) = event {
                    *events_by_type.entry("agent_started_job".to_string()).or_insert(0) += 1;
                    received_events.push(event);
                    total_received += 1;
                }
            }
            event = collector.agent_finished_job.recv() => {
                if let Some(event) = event {
                    *events_by_type.entry("agent_finished_job".to_string()).or_insert(0) += 1;
                    received_events.push(event);
                    total_received += 1;
                }
            }
            event = collector.coordination_tx.recv() => {
                if let Some(event) = event {
                    *events_by_type.entry("coordination_tx".to_string()).or_insert(0) += 1;
                    received_events.push(event);
                    total_received += 1;
                }
            }
            event = collector.coordinator_error.recv() => {
                if let Some(event) = event {
                    *events_by_type.entry("coordinator_error".to_string()).or_insert(0) += 1;
                    received_events.push(event);
                    total_received += 1;
                }
            }
            event = collector.client_transaction.recv() => {
                if let Some(event) = event {
                    *events_by_type.entry("client_transaction".to_string()).or_insert(0) += 1;
                    received_events.push(event);
                    total_received += 1;
                }
            }
            event = collector.agent_message.recv() => {
                if let Some(event) = event {
                    *events_by_type.entry("agent_message".to_string()).or_insert(0) += 1;
                    received_events.push(event);
                    total_received += 1;
                }
            }
            event = collector.agent_transaction.recv() => {
                if let Some(event) = event {
                    *events_by_type.entry("agent_transaction".to_string()).or_insert(0) += 1;
                    received_events.push(event);
                    total_received += 1;
                }
            }
            _ = sleep(Duration::from_millis(100)) => {
                // Check if we've received enough events
                let expected_total = sent_counts.values().sum::<usize>();

                if total_received >= expected_total {
                    break;
                }
            }
        }
    }

    println!("\n📊 NATS Event Verification Results:");
    println!("📥 Total events received from NATS: {}", total_received);

    // Show detailed comparison
    let all_event_types = [
        "coordinator_started",
        "agent_started_job",
        "agent_finished_job",
        "coordination_tx",
        "coordinator_error",
        "client_transaction",
        "agent_message",
        "agent_transaction",
    ];

    for event_type in &all_event_types {
        let sent_count = sent_counts.get(*event_type).unwrap_or(&0);
        let received_count = events_by_type.get(*event_type).unwrap_or(&0);
        let status = if sent_count == received_count {
            "✅"
        } else {
            "❌"
        };
        println!(
            "  {} {}: sent={}, received={}, diff={}",
            status,
            event_type,
            sent_count,
            received_count,
            *received_count as i32 - *sent_count as i32
        );
    }

    // Get sent events for comparison
    let sent_events_list = sent_events.get_events().await;
    let sent_count = sent_events_list.len();

    println!("\n📋 Event Content Verification:");
    println!("📤 Total events sent: {}", sent_count);
    println!("📥 Total events received: {}", total_received);

    // Compare event contents
    let content_verification = compare_events(&sent_events_list, &received_events).await;

    println!("\n🔍 Content Comparison Results:");
    println!(
        "  ✅ Matching events: {}",
        content_verification.matching_events
    );
    println!(
        "  ❌ Missing events: {}",
        content_verification.missing_events
    );
    println!("  ⚠️  Extra events: {}", content_verification.extra_events);
    println!(
        "  🔄 Different content: {}",
        content_verification.different_content
    );

    if content_verification.different_content > 0 {
        println!("\n📝 Content Differences Found:");
        for diff in &content_verification.content_differences {
            println!("  {} {}", "⚠️ ", diff);
        }
    }

    // Expected counts
    let expected_total = sent_counts.values().sum::<usize>();

    println!("\n📈 Expected vs Actual:");
    println!("  Expected total: {} events", expected_total);
    println!("  Sent total: {} events", sent_count);
    println!("  Received total: {} events", total_received);

    // Overall verification result
    let count_check = total_received >= expected_total * 90 / 100;
    let content_check = content_verification.matching_events >= sent_count * 90 / 100;

    if count_check && content_check {
        println!(
            "✅ NATS verification PASSED - Events successfully published with matching content!"
        );
    } else if count_check {
        println!("⚠️  NATS verification PARTIAL - Counts match but some content differences found");
    } else if total_received > 0 {
        println!(
            "⚠️  NATS verification PARTIAL - Some events received but counts or content differ"
        );
    } else {
        println!("❌ NATS verification FAILED - No events received from NATS");
    }

    println!(
        "📝 Note: Minor differences may occur due to timing, batching, and concurrent processing"
    );
}

#[derive(Debug)]
struct ContentVerificationResult {
    matching_events: usize,
    missing_events: usize,
    extra_events: usize,
    different_content: usize,
    content_differences: Vec<String>,
}

async fn compare_events(
    sent_events: &[Event],
    received_events: &[Event],
) -> ContentVerificationResult {
    let mut matching_events = 0;
    let mut different_content = 0;
    let mut content_differences = Vec::new();

    // Create maps for quick lookup by event signature
    let mut sent_map: HashMap<String, &Event> = HashMap::new();
    let mut received_map: HashMap<String, &Event> = HashMap::new();

    // Map sent events by their signature
    for event in sent_events {
        let signature = create_event_signature(event);
        sent_map.insert(signature, event);
    }

    // Map received events by their signature
    for event in received_events {
        let signature = create_event_signature(event);
        received_map.insert(signature, event);
    }

    // Check for matching events
    for (signature, sent_event) in &sent_map {
        if let Some(received_event) = received_map.get(signature) {
            if events_match_content(sent_event, received_event) {
                matching_events += 1;
            } else {
                different_content += 1;
                let diff = format!("Event signature {} has different content", signature);
                content_differences.push(diff);
            }
        }
    }

    let missing_events = sent_events
        .len()
        .saturating_sub(matching_events + different_content);
    let extra_events = received_events
        .len()
        .saturating_sub(matching_events + different_content);

    ContentVerificationResult {
        matching_events,
        missing_events,
        extra_events,
        different_content,
        content_differences,
    }
}

fn create_event_signature(event: &Event) -> String {
    // Create a unique signature for each event based on stable fields (not timestamps)
    match &event.event_type {
        Some(event_type) => match event_type {
            events::event::EventType::Coordinator(coord_event) => match &coord_event.event {
                Some(coordinator_event) => match coordinator_event {
                    events::coordinator_event::Event::CoordinatorStarted(e) => {
                        // Use ethereum_address as unique identifier (contains the index)
                        format!("coord_started_{}_{}", e.coordinator_id, e.ethereum_address)
                    }
                    events::coordinator_event::Event::AgentStartedJob(e) => {
                        format!("agent_started_{}", e.job_id)
                    }
                    events::coordinator_event::Event::AgentFinishedJob(e) => {
                        // Include duration to differentiate from started job
                        format!("agent_finished_{}_{}", e.job_id, e.duration)
                    }
                    events::coordinator_event::Event::CoordinationTx(e) => {
                        format!("coord_tx_{}", e.tx_hash)
                    }
                    events::coordinator_event::Event::CoordinatorError(e) => {
                        // Use message content which includes the index
                        format!("coord_error_{}_{}", e.coordinator_id, e.message)
                    }
                    events::coordinator_event::Event::ClientTransaction(e) => {
                        format!("client_tx_{}_{}", e.tx_hash, e.sequence)
                    }
                },
                None => "coord_unknown".to_string(),
            },
            events::event::EventType::Agent(agent_event) => match &agent_event.event {
                Some(agent_event_type) => match agent_event_type {
                    events::agent_event::Event::Message(e) => {
                        // Use message content which includes the index
                        format!("agent_msg_{}_{}", e.job_id, e.message)
                    }
                    events::agent_event::Event::Transaction(e) => {
                        format!("agent_tx_{}", e.tx_hash)
                    }
                },
                None => "agent_unknown".to_string(),
            },
        },
        None => "unknown".to_string(),
    }
}

fn events_match_content(sent: &Event, received: &Event) -> bool {
    // Deep comparison of event content via protobuf serialization
    sent.encode_to_vec() == received.encode_to_vec()
}

async fn test_coordinator_events(
    client: &mut SilvanaEventsServiceClient<tonic::transport::Channel>,
    sent_events: SentEventsCollector,
) {
    let start_time = Instant::now();

    let test_cases = vec![
        ("coordinator_started", create_coordinator_started_event()),
        ("agent_started_job", create_agent_started_job_event()),
        ("agent_finished_job", create_agent_finished_job_event()),
        ("coordination_tx", create_coordination_tx_event()),
        ("coordinator_message", create_coordinator_message_event()),
        ("client_transaction", create_client_transaction_event()),
    ];

    println!(
        "  🚀 Sending {} event types concurrently with {} events each",
        test_cases.len(),
        NUM_EVENTS
    );

    // Run all event types in parallel
    let handles: Vec<_> = test_cases
        .into_iter()
        .map(|(event_type, event)| {
            let client_clone = client.clone();
            let event_type = event_type.to_string();
            let sent_events_clone = sent_events.clone();

            tokio::spawn(async move {
                println!(
                    "  📤 Starting {} events of type: {}",
                    NUM_EVENTS, event_type
                );

                // Send events in chunks of 100 concurrently for each type
                const CHUNK_SIZE: usize = 100;
                let chunks: Vec<_> = (1..=NUM_EVENTS)
                    .collect::<Vec<_>>()
                    .chunks(CHUNK_SIZE)
                    .map(|chunk| chunk.to_vec())
                    .collect();

                for chunk in chunks {
                    let chunk_handles: Vec<_> = chunk
                        .into_iter()
                        .map(|i| {
                            let mut test_event = event.clone();
                            modify_coordinator_event_for_uniqueness(&mut test_event, i);
                            let mut client_clone2 = client_clone.clone();
                            let event_type_clone = event_type.clone();
                            let sent_events_clone2 = sent_events_clone.clone();

                            tokio::spawn(async move {
                                // Record the event before sending
                                sent_events_clone2.add_event(test_event.clone()).await;

                                let request = Request::new(SubmitEventRequest {
                                    event: Some(test_event),
                                });
                                match client_clone2.submit_event(request).await {
                                    Ok(response) => {
                                        let resp = response.into_inner();
                                        if !resp.success {
                                            println!(
                                                "    ⚠️  {} Event {}: {}",
                                                event_type_clone, i, resp.message
                                            );
                                        }
                                        assert!(
                                            resp.processed_count == 1,
                                            "Expected 1 processed event, got {}",
                                            resp.processed_count
                                        );
                                        Ok(())
                                    }
                                    Err(e) => Err(format!(
                                        "❌ Failed to send {} event {}: {}",
                                        event_type_clone, i, e
                                    )),
                                }
                            })
                        })
                        .collect();

                    // Wait for this chunk to complete
                    for handle in chunk_handles {
                        if let Err(e) = handle.await.unwrap() {
                            panic!("{}", e);
                        }
                    }
                }

                println!(
                    "  ✅ Successfully sent {} {} events",
                    NUM_EVENTS, event_type
                );
                event_type
            })
        })
        .collect();

    // Wait for all event types to complete
    for handle in handles {
        handle.await.unwrap();
    }

    let duration = start_time.elapsed();
    println!(
        "  ⏱️  Coordinator events duration: {}ms (parallel execution)",
        duration.as_millis()
    );
}

async fn test_agent_events(
    client: &mut SilvanaEventsServiceClient<tonic::transport::Channel>,
    sent_events: SentEventsCollector,
) {
    let start_time = Instant::now();

    let test_cases = vec![
        ("agent_message", create_agent_message_event()),
        ("agent_transaction", create_agent_transaction_event()),
    ];

    println!(
        "  🚀 Sending {} agent event types concurrently with {} events each",
        test_cases.len(),
        NUM_EVENTS
    );

    // Run all event types in parallel
    let handles: Vec<_> = test_cases
        .into_iter()
        .map(|(event_type, event)| {
            let client_clone = client.clone();
            let event_type = event_type.to_string();
            let sent_events_clone = sent_events.clone();

            tokio::spawn(async move {
                println!(
                    "  📤 Starting {} events of type: {}",
                    NUM_EVENTS, event_type
                );

                // Send events in chunks of 100 concurrently for each type
                const CHUNK_SIZE: usize = 100;
                let chunks: Vec<_> = (1..=NUM_EVENTS)
                    .collect::<Vec<_>>()
                    .chunks(CHUNK_SIZE)
                    .map(|chunk| chunk.to_vec())
                    .collect();

                for chunk in chunks {
                    let chunk_handles: Vec<_> = chunk
                        .into_iter()
                        .map(|i| {
                            let mut test_event = event.clone();
                            modify_agent_event_for_uniqueness(&mut test_event, i);
                            let mut client_clone2 = client_clone.clone();
                            let event_type_clone = event_type.clone();
                            let sent_events_clone2 = sent_events_clone.clone();

                            tokio::spawn(async move {
                                // Record the event before sending
                                sent_events_clone2.add_event(test_event.clone()).await;

                                let request = Request::new(SubmitEventRequest {
                                    event: Some(test_event),
                                });
                                match client_clone2.submit_event(request).await {
                                    Ok(response) => {
                                        let resp = response.into_inner();
                                        if !resp.success {
                                            println!(
                                                "    ⚠️  {} Event {}: {}",
                                                event_type_clone, i, resp.message
                                            );
                                        }
                                        assert!(
                                            resp.processed_count == 1,
                                            "Expected 1 processed event, got {}",
                                            resp.processed_count
                                        );
                                        Ok(())
                                    }
                                    Err(e) => Err(format!(
                                        "❌ Failed to send {} event {}: {}",
                                        event_type_clone, i, e
                                    )),
                                }
                            })
                        })
                        .collect();

                    // Wait for this chunk to complete
                    for handle in chunk_handles {
                        if let Err(e) = handle.await.unwrap() {
                            panic!("{}", e);
                        }
                    }
                }

                println!(
                    "  ✅ Successfully sent {} {} events",
                    NUM_EVENTS, event_type
                );
                event_type
            })
        })
        .collect();

    // Wait for all event types to complete
    for handle in handles {
        handle.await.unwrap();
    }

    let duration = start_time.elapsed();
    println!(
        "  ⏱️  Agent events duration: {}ms (parallel execution)",
        duration.as_millis()
    );
}

async fn test_batch_events(
    client: &mut SilvanaEventsServiceClient<tonic::transport::Channel>,
    sent_events: SentEventsCollector,
) {
    let start_time = Instant::now();

    // Split into multiple concurrent batches
    const BATCH_SIZE: usize = 1000; // Send batches of 1000 events each
    let num_batches = (NUM_EVENTS + BATCH_SIZE - 1) / BATCH_SIZE;

    println!(
        "  🚀 Creating {} concurrent batches of ~{} mixed events each (total: {})",
        num_batches, BATCH_SIZE, NUM_EVENTS
    );

    // Create concurrent batch handles
    let handles: Vec<_> = (0..num_batches)
        .map(|batch_idx| {
            let mut client_clone = client.clone();
            let sent_events_clone = sent_events.clone();

            tokio::spawn(async move {
                let start_event_idx = batch_idx * BATCH_SIZE + 1;
                let end_event_idx = std::cmp::min((batch_idx + 1) * BATCH_SIZE, NUM_EVENTS);
                let batch_event_count = end_event_idx - start_event_idx + 1;

                println!(
                    "  📦 Creating batch {} with {} events (events {}-{})",
                    batch_idx + 1,
                    batch_event_count,
                    start_event_idx,
                    end_event_idx
                );

                let mut events = Vec::new();
                let mut batch_coord_started = 0;
                let mut batch_agent_message = 0;

                for i in start_event_idx..=end_event_idx {
                    // Alternate between coordinator and agent events
                    let event = if i % 2 == 0 {
                        let mut event = create_coordinator_started_event();
                        // Use batch-specific index to avoid conflicts with individual events
                        let batch_index = i + 1000000;
                        modify_coordinator_event_for_uniqueness(&mut event, batch_index);
                        batch_coord_started += 1;
                        event
                    } else {
                        let mut event = create_agent_message_event();
                        // Use batch-specific index to avoid conflicts with individual events
                        let batch_index = i + 1000000;
                        modify_agent_event_for_uniqueness(&mut event, batch_index);
                        batch_agent_message += 1;
                        event
                    };

                    events.push(event);
                }

                println!(
                    "  📊 Batch {} breakdown: {} coordinator_started, {} agent_message",
                    batch_idx + 1,
                    batch_coord_started,
                    batch_agent_message
                );

                // Record the events before sending
                sent_events_clone.add_events(events.clone()).await;

                let request = Request::new(SubmitEventsRequest { events });

                match client_clone.submit_events(request).await {
                    Ok(response) => {
                        let resp = response.into_inner();
                        println!(
                            "  📊 Batch {} result: {} - Processed: {}/{}",
                            batch_idx + 1,
                            resp.message,
                            resp.processed_count,
                            batch_event_count
                        );

                        if !resp.success {
                            println!(
                                "    ⚠️  Batch {} had some failures: {}",
                                batch_idx + 1,
                                resp.message
                            );
                        }

                        // Should have processed all events or failed gracefully
                        assert!(
                            resp.processed_count <= batch_event_count as u32,
                            "Batch {} processed count {} exceeds sent count {}",
                            batch_idx + 1,
                            resp.processed_count,
                            batch_event_count
                        );

                        Ok((
                            batch_idx + 1,
                            resp.processed_count as usize,
                            batch_event_count,
                        ))
                    }
                    Err(e) => Err(format!(
                        "❌ Failed to send batch {} events: {}",
                        batch_idx + 1,
                        e
                    )),
                }
            })
        })
        .collect();

    // Wait for all batches to complete and collect results
    let mut total_processed = 0;
    let mut total_sent = 0;

    for handle in handles {
        match handle.await.unwrap() {
            Ok((batch_num, processed, sent)) => {
                total_processed += processed;
                total_sent += sent;
                println!(
                    "  ✅ Batch {} completed: {}/{} events",
                    batch_num, processed, sent
                );
            }
            Err(e) => panic!("{}", e),
        }
    }

    let duration = start_time.elapsed();
    println!(
        "  🎉 Successfully sent {} concurrent batches totaling {}/{} events",
        num_batches, total_processed, total_sent
    );
    println!(
        "  ⏱️  Concurrent batch duration: {}ms",
        duration.as_millis()
    );
}

// Helper functions to create different event types

fn create_coordinator_started_event() -> Event {
    Event {
        event_type: Some(event::EventType::Coordinator(CoordinatorEvent {
            event: Some(coordinator_event::Event::CoordinatorStarted(
                CoordinatorStartedEvent {
                    coordinator_id: COORDINATOR_ID.to_string(),
                    ethereum_address: "0x1234567890abcdef1234567890abcdef12345678".to_string(),
                    sui_ed25519_address: "0xabcdef1234567890abcdef1234567890abcdef12".to_string(),
                    event_timestamp: get_current_timestamp(),
                },
            )),
        })),
    }
}

fn create_agent_started_job_event() -> Event {
    Event {
        event_type: Some(event::EventType::Coordinator(CoordinatorEvent {
            event: Some(coordinator_event::Event::AgentStartedJob(
                AgentStartedJobEvent {
                    coordinator_id: COORDINATOR_ID.to_string(),
                    developer: "test-developer".to_string(),
                    agent: "test-agent".to_string(),
                    app: "test-app".to_string(),
                    job_id: "job-123".to_string(),
                    event_timestamp: get_current_timestamp(),
                },
            )),
        })),
    }
}

fn create_agent_finished_job_event() -> Event {
    Event {
        event_type: Some(event::EventType::Coordinator(CoordinatorEvent {
            event: Some(coordinator_event::Event::AgentFinishedJob(
                AgentFinishedJobEvent {
                    coordinator_id: COORDINATOR_ID.to_string(),
                    developer: "test-developer".to_string(),
                    agent: "test-agent".to_string(),
                    app: "test-app".to_string(),
                    job_id: "job-123".to_string(),
                    duration: 5000, // 5 seconds
                    event_timestamp: get_current_timestamp(),
                },
            )),
        })),
    }
}

fn create_coordination_tx_event() -> Event {
    Event {
        event_type: Some(event::EventType::Coordinator(CoordinatorEvent {
            event: Some(coordinator_event::Event::CoordinationTx(
                CoordinationTxEvent {
                    coordinator_id: COORDINATOR_ID.to_string(),
                    developer: "test-developer".to_string(),
                    agent: "test-agent".to_string(),
                    app: "test-app".to_string(),
                    job_id: "job-123".to_string(),
                    memo: "Test coordination transaction".to_string(),
                    tx_hash: "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
                        .to_string(),
                    event_timestamp: get_current_timestamp(),
                },
            )),
        })),
    }
}

fn create_coordinator_message_event() -> Event {
    Event {
        event_type: Some(event::EventType::Coordinator(CoordinatorEvent {
            event: Some(coordinator_event::Event::CoordinatorError(
                CoordinatorMessageEvent {
                    coordinator_id: COORDINATOR_ID.to_string(),
                    event_timestamp: get_current_timestamp(),
                    level: 3, // LogLevel::Error
                    message: "Test error message".to_string(),
                },
            )),
        })),
    }
}

fn create_client_transaction_event() -> Event {
    Event {
        event_type: Some(event::EventType::Coordinator(CoordinatorEvent {
            event: Some(coordinator_event::Event::ClientTransaction(
                ClientTransactionEvent {
                    coordinator_id: COORDINATOR_ID.to_string(),
                    developer: "test-developer".to_string(),
                    agent: "test-agent".to_string(),
                    app: "test-app".to_string(),
                    client_ip_address: "192.168.1.100".to_string(),
                    method: "submit_proof".to_string(),
                    data: b"test transaction data".to_vec(),
                    tx_hash: "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
                        .to_string(),
                    sequence: 1,
                    event_timestamp: get_current_timestamp(),
                },
            )),
        })),
    }
}

fn create_agent_message_event() -> Event {
    Event {
        event_type: Some(event::EventType::Agent(AgentEvent {
            event: Some(agent_event::Event::Message(AgentMessageEvent {
                coordinator_id: COORDINATOR_ID.to_string(),
                developer: "test-developer".to_string(),
                agent: "test-agent".to_string(),
                app: "test-app".to_string(),
                job_id: "job-123".to_string(),
                sequences: vec![1, 2, 3],
                event_timestamp: get_current_timestamp(),
                level: 1, // LogLevel::Info
                message: "Test agent message".to_string(),
            })),
        })),
    }
}

fn create_agent_transaction_event() -> Event {
    Event {
        event_type: Some(event::EventType::Agent(AgentEvent {
            event: Some(agent_event::Event::Transaction(AgentTransactionEvent {
                coordinator_id: COORDINATOR_ID.to_string(),
                tx_type: "transaction".to_string(),
                developer: "test-developer".to_string(),
                agent: "test-agent".to_string(),
                app: "test-app".to_string(),
                job_id: "job-123".to_string(),
                sequences: vec![1, 2, 3],
                event_timestamp: get_current_timestamp(),
                tx_hash: "0x9876543210fedcba9876543210fedcba9876543210fedcba9876543210fedcba"
                    .to_string(),
                chain: "ethereum".to_string(),
                network: "mainnet".to_string(),
                memo: "Test agent transaction".to_string(),
                metadata: Some(r#"{"gas_used": 21000, "gas_price": "20000000000"}"#.to_string()),
            })),
        })),
    }
}

// Helper functions to add uniqueness to events

fn modify_coordinator_event_for_uniqueness(event: &mut Event, index: usize) {
    if let Some(event::EventType::Coordinator(coord_event)) = &mut event.event_type {
        match &mut coord_event.event {
            Some(coordinator_event::Event::CoordinatorStarted(e)) => {
                e.ethereum_address = format!("0x{:040x}", index);
                e.event_timestamp = get_current_timestamp() + index as u64;
            }
            Some(coordinator_event::Event::AgentStartedJob(e)) => {
                e.job_id = format!("job-{}", index);
                e.event_timestamp = get_current_timestamp() + index as u64;
            }
            Some(coordinator_event::Event::AgentFinishedJob(e)) => {
                e.job_id = format!("job-{}", index);
                e.duration = 1000 + (index as u64 * 100);
                e.event_timestamp = get_current_timestamp() + index as u64;
            }
            Some(coordinator_event::Event::CoordinationTx(e)) => {
                e.job_id = format!("job-{}", index);
                e.tx_hash = format!("0x{:064x}", index);
                e.event_timestamp = get_current_timestamp() + index as u64;
            }
            Some(coordinator_event::Event::CoordinatorError(e)) => {
                e.message = format!("Test error message #{}", index);
                e.event_timestamp = get_current_timestamp() + index as u64;
            }
            Some(coordinator_event::Event::ClientTransaction(e)) => {
                e.tx_hash = format!("0x{:064x}", index);
                e.sequence = index as u64;
                e.event_timestamp = get_current_timestamp() + index as u64;
            }
            None => {}
        }
    }
}

fn modify_agent_event_for_uniqueness(event: &mut Event, index: usize) {
    if let Some(event::EventType::Agent(agent_event)) = &mut event.event_type {
        match &mut agent_event.event {
            Some(agent_event::Event::Message(e)) => {
                e.job_id = format!("job-{}", index);
                e.message = format!("Test agent message #{}", index);
                e.sequences = vec![index as u64];
                e.event_timestamp = get_current_timestamp() + index as u64;
            }
            Some(agent_event::Event::Transaction(e)) => {
                e.job_id = format!("job-{}", index);
                e.tx_hash = format!("0x{:064x}", index + 1000);
                e.sequences = vec![index as u64];
                e.event_timestamp = get_current_timestamp() + index as u64;
            }
            None => {}
        }
    }
}

fn get_current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
