use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tonic::Request;

use proto::silvana_events_service_client::SilvanaEventsServiceClient;
use proto::*;

// Test configuration
const TOTAL_EVENTS: usize = 100;
const SEQUENCE_COUNT: usize = 10;

fn get_server_addr() -> String {
    // Load .env file if it exists
    dotenvy::dotenv().ok();

    // Get TEST_SERVER from environment, fallback to default if not set
    std::env::var("TEST_SERVER").unwrap_or_else(|_| "http://127.0.0.1:50051".to_string())
}

// Generate a unique coordinator ID for each test run to avoid data contamination
fn get_unique_coordinator_id() -> String {
    format!("seq-test-{}", get_current_timestamp())
}

#[tokio::test]
async fn test_sequence_events_round_trip() {
    let server_addr = get_server_addr();

    println!("🧪 Starting sequence round-trip test...");
    println!(
        "📊 Configuration: {} events across {} sequences",
        TOTAL_EVENTS, SEQUENCE_COUNT
    );
    println!("🎯 Server address: {}", server_addr);

    // Connect to the gRPC server (tonic handles TLS automatically for https:// URLs)
    println!("🔗 Attempting to connect to: {}", server_addr);

    let mut client = match SilvanaEventsServiceClient::connect(server_addr.clone()).await {
        Ok(client) => {
            println!("✅ Connected to RPC server successfully");
            client
        }
        Err(e) => {
            panic!(
                "❌ Failed to connect to RPC server at {}: {}\nMake sure the server is running and accessible",
                server_addr, e
            );
        }
    };

    // Step 1: Create events grouped by sequence
    let coordinator_id = get_unique_coordinator_id();
    let all_events = create_test_events_with_sequences(&coordinator_id);

    // Group events by sequence for isolated send→query cycles
    let mut events_by_sequence: HashMap<u64, Vec<(Event, String, String)>> = HashMap::new();
    for (event, sequence, event_type, job_id) in all_events {
        events_by_sequence
            .entry(sequence)
            .or_insert_with(Vec::new)
            .push((event, event_type, job_id));
    }

    println!("📊 Starting isolated sequence send→query cycles...");
    println!("    Will send and immediately query each sequence individually");

    let mut total_retrieved = 0;
    let mut total_expected = 0;
    let mut latencies: Vec<u128> = Vec::new();

    // Step 2: Process each sequence individually (send → query cycle)
    for sequence in 1..=SEQUENCE_COUNT as u64 {
        let empty_vec = Vec::new();
        let events_for_sequence = events_by_sequence.get(&sequence).unwrap_or(&empty_vec);
        if events_for_sequence.is_empty() {
            continue;
        }

        total_expected += events_for_sequence.len();

        // Calculate expected counts
        let mut expected_message_events = 0;
        let mut expected_tx_events = 0;
        for (_, event_type, _) in events_for_sequence {
            if event_type == "message" {
                expected_message_events += 1;
            } else if event_type == "transaction" {
                expected_tx_events += 1;
            }
        }

        println!(
            "\n🔄 Sequence {}: Sending {} events ({} messages, {} transactions)",
            sequence,
            events_for_sequence.len(),
            expected_message_events,
            expected_tx_events
        );

        // Send all events for this sequence
        let send_start = std::time::Instant::now();
        let mut last_send_time = std::time::Instant::now();
        for (event, event_type, job_id) in events_for_sequence {
            let request = Request::new(SubmitEventRequest {
                event: Some(event.clone()),
            });
            let send_time = std::time::Instant::now();

            match client.submit_event(request).await {
                Ok(response) => {
                    let resp = response.into_inner();
                    if !resp.success {
                        panic!(
                            "❌ Failed to send {} event {}: {}",
                            event_type, job_id, resp.message
                        );
                    }
                    assert_eq!(resp.processed_count, 1, "Expected 1 processed event");
                    last_send_time = send_time; // Track the last successful send time
                }
                Err(e) => panic!("❌ Failed to send {} event {}: {}", event_type, job_id, e),
            }
        }

        println!(
            "  📤 All {} events sent in {}ms, starting immediate polling...",
            events_for_sequence.len(),
            send_start.elapsed().as_millis()
        );

        // Immediately start polling for this sequence
        let timeout = std::time::Duration::from_secs(5);
        let poll_start = std::time::Instant::now();
        let mut attempt = 0;
        let mut success = false;

        while poll_start.elapsed() < timeout && !success {
            attempt += 1;
            let query_start = std::time::Instant::now();

            // Query AgentMessageEvents by sequence
            let message_request = Request::new(GetAgentMessageEventsBySequenceRequest {
                sequence,
                limit: None,
                offset: None,
                coordinator_id: Some(coordinator_id.clone()),
                developer: None,
                agent: None,
                app: None,
            });

            println!("Message request: {:?}", &message_request);

            let message_response = match client
                .get_agent_message_events_by_sequence(message_request)
                .await
            {
                Ok(resp) => resp.into_inner(),
                Err(e) => {
                    println!("    ⚠️  Attempt {}: Message query failed: {}", attempt, e);
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    continue;
                }
            };

            // Query AgentTransactionEvents by sequence
            let tx_request = Request::new(GetAgentTransactionEventsBySequenceRequest {
                sequence,
                limit: None,
                offset: None,
                coordinator_id: Some(coordinator_id.clone()),
                developer: None,
                agent: None,
                app: None,
            });

            let tx_response = match client
                .get_agent_transaction_events_by_sequence(tx_request)
                .await
            {
                Ok(resp) => resp.into_inner(),
                Err(e) => {
                    println!(
                        "    ⚠️  Attempt {}: Transaction query failed: {}",
                        attempt, e
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    continue;
                }
            };

            let query_duration = query_start.elapsed();
            let end_to_end_latency = last_send_time.elapsed();

            // Check if we got the expected results
            if message_response.success
                && tx_response.success
                && message_response.events.len() == expected_message_events
                && tx_response.events.len() == expected_tx_events
            {
                // Verify data integrity
                let mut all_valid = true;

                for event in &message_response.events {
                    if event.coordinator_id != coordinator_id
                        || !event.sequences.contains(&sequence)
                    {
                        all_valid = false;
                        break;
                    }
                }

                for event in &tx_response.events {
                    if event.coordinator_id != coordinator_id
                        || !event.sequences.contains(&sequence)
                    {
                        all_valid = false;
                        break;
                    }
                }

                if all_valid {
                    success = true;
                    let retrieved_count = message_response.events.len() + tx_response.events.len();
                    total_retrieved += retrieved_count;

                    // Track latency for min/max calculation
                    latencies.push(end_to_end_latency.as_millis());

                    println!(
                        "  ✅ SUCCESS: attempt={}, query_time={}ms, send→query_latency={}ms",
                        attempt,
                        query_duration.as_millis(),
                        end_to_end_latency.as_millis()
                    );
                    println!(
                        "     Retrieved: {} events ({} messages, {} transactions)",
                        retrieved_count,
                        message_response.events.len(),
                        tx_response.events.len()
                    );
                } else {
                    println!(
                        "    🔄 Attempt {}: Got expected counts but data validation failed (latency={}ms)",
                        attempt,
                        end_to_end_latency.as_millis()
                    );
                }
            } else {
                println!(
                    "    🔄 Attempt {}: Expected {}/{} events, got {}/{} (query_time={}ms, latency={}ms)",
                    attempt,
                    expected_message_events,
                    expected_tx_events,
                    message_response.events.len(),
                    tx_response.events.len(),
                    query_duration.as_millis(),
                    end_to_end_latency.as_millis()
                );
            }
        }

        if !success {
            panic!(
                "❌ TIMEOUT: Sequence {} failed to return expected results within {}ms after {} attempts",
                sequence,
                timeout.as_millis(),
                attempt
            );
        }
    }

    // Final verification
    assert_eq!(
        total_retrieved, total_expected,
        "Total retrieved events ({}) should match total expected ({})",
        total_retrieved, total_expected
    );

    println!("\n🎉 Isolated sequence latency test completed successfully!");
    println!("📊 Summary:");
    println!(
        "  - Processed {} sequences individually with {} total events",
        SEQUENCE_COUNT, TOTAL_EVENTS
    );
    println!("  - Retrieved: {} events", total_retrieved);

    // Calculate and display latency statistics
    if !latencies.is_empty() {
        let min_latency = latencies.iter().min().unwrap();
        let max_latency = latencies.iter().max().unwrap();
        let avg_latency = latencies.iter().sum::<u128>() / latencies.len() as u128;

        println!("📈 Latency Statistics:");
        println!("  - Min latency: {}ms", min_latency);
        println!("  - Max latency: {}ms", max_latency);
        println!("  - Avg latency: {}ms", avg_latency);
        println!("  - Latency range: {}ms", max_latency - min_latency);
    }

    println!(
        "  - Each sequence used isolated send→query cycles for accurate latency measurement ✅"
    );
    println!("  - Measured pure processing latency without cross-sequence interference");
}

fn create_test_events_with_sequences(coordinator_id: &str) -> Vec<(Event, u64, String, String)> {
    let mut events = Vec::new();

    // Create events distributed across sequences
    for i in 0..TOTAL_EVENTS {
        let sequence = ((i % SEQUENCE_COUNT) + 1) as u64; // Sequences 1-10
        let job_id = format!("seq-test-job-{}", i);

        // Alternate between message and transaction events
        if i % 2 == 0 {
            // Agent Message Event
            let event = Event {
                event_type: Some(event::EventType::Agent(AgentEvent {
                    event: Some(agent_event::Event::Message(AgentMessageEvent {
                        coordinator_id: coordinator_id.to_string(),
                        developer: "sequence-test-developer".to_string(),
                        agent: "sequence-test-agent".to_string(),
                        app: "sequence-test-app".to_string(),
                        job_id: job_id.clone(),
                        sequences: vec![sequence], // Single sequence per event for this test
                        event_timestamp: get_current_timestamp() + i as u64,
                        level: 1, // LogLevel::Info
                        message: format!("Test message for sequence {} (event {})", sequence, i),
                    })),
                })),
            };
            events.push((event, sequence, "message".to_string(), job_id));
        } else {
            // Agent Transaction Event
            let event = Event {
                event_type: Some(event::EventType::Agent(AgentEvent {
                    event: Some(agent_event::Event::Transaction(AgentTransactionEvent {
                        coordinator_id: coordinator_id.to_string(),
                        tx_type: "test_transaction".to_string(),
                        developer: "sequence-test-developer".to_string(),
                        agent: "sequence-test-agent".to_string(),
                        app: "sequence-test-app".to_string(),
                        job_id: job_id.clone(),
                        sequences: vec![sequence], // Single sequence per event for this test
                        event_timestamp: get_current_timestamp() + i as u64,
                        tx_hash: format!("0x{:064x}", i),
                        chain: "ethereum".to_string(),
                        network: "testnet".to_string(),
                        memo: format!("Test transaction for sequence {} (event {})", sequence, i),
                        metadata: Some(format!(
                            r#"{{"event_index": {}, "sequence": {}}}"#,
                            i, sequence
                        )),
                    })),
                })),
            };
            events.push((event, sequence, "transaction".to_string(), job_id));
        }
    }

    println!("📊 Created {} events:", events.len());

    // Print distribution summary
    let mut sequence_counts: HashMap<u64, (usize, usize)> = HashMap::new(); // (messages, transactions)
    for (_, sequence, event_type, _) in &events {
        let entry = sequence_counts.entry(*sequence).or_insert((0, 0));
        if event_type == "message" {
            entry.0 += 1;
        } else {
            entry.1 += 1;
        }
    }

    for seq in 1..=SEQUENCE_COUNT as u64 {
        let (msg_count, tx_count) = sequence_counts.get(&seq).unwrap_or(&(0, 0));
        println!(
            "  - Sequence {}: {} messages, {} transactions (total: {})",
            seq,
            msg_count,
            tx_count,
            msg_count + tx_count
        );
    }

    events
}

fn get_current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
