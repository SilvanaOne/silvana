use futures::StreamExt;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{sleep, timeout, Duration};
use tonic::Request;

use proto::silvana_events_service_client::SilvanaEventsServiceClient;
use proto::*;

// Test configuration
const SERVER_ADDR: &str = "https://rpc-dev.silvana.dev";
const NATS_URL: &str = "nats://rpc-dev.silvana.dev:4222";
const NATS_STREAM_NAME: &str = "silvana-events";

// Generate a unique coordinator ID for each test run to avoid data contamination
fn get_unique_coordinator_id() -> String {
    format!("nats-test-{}", get_current_timestamp())
}

#[tokio::test]
async fn test_nats_roundtrip_latency() {
    println!("🧪 Starting NATS roundtrip latency test...");
    println!("🎯 gRPC Server: {}", SERVER_ADDR);
    println!("📡 NATS Server: {}", NATS_URL);

    // Step 1: Connect to NATS
    let nats_client = match async_nats::connect(NATS_URL).await {
        Ok(client) => {
            println!("✅ Connected to NATS server successfully");
            client
        }
        Err(e) => {
            panic!(
                "❌ Failed to connect to NATS server at {}: {}\nMake sure NATS is running and accessible",
                NATS_URL, e
            );
        }
    };

    // Step 2: Connect to gRPC server
    let mut grpc_client = match SilvanaEventsServiceClient::connect(SERVER_ADDR).await {
        Ok(client) => {
            println!("✅ Connected to gRPC server successfully");
            client
        }
        Err(e) => {
            panic!(
                "❌ Failed to connect to gRPC server at {}: {}\nMake sure the server is running with: cargo run",
                SERVER_ADDR, e
            );
        }
    };

    // Step 3: Setup NATS subscription for the specific event type we'll send
    let coordinator_id = get_unique_coordinator_id();
    let base_subject = format!("{}.events", NATS_STREAM_NAME);
    let target_subject = format!("{}.coordinator.started", base_subject);

    println!("📡 Setting up NATS subscription to: {}", target_subject);

    let mut subscription = match nats_client.subscribe(target_subject.clone()).await {
        Ok(sub) => {
            println!("✅ NATS subscription established successfully");
            sub
        }
        Err(e) => {
            panic!(
                "❌ Failed to subscribe to NATS subject {}: {}",
                target_subject, e
            );
        }
    };

    // Give subscription time to be established
    sleep(Duration::from_millis(100)).await;

    // Step 4: Create a unique test event
    println!(
        "📝 Creating test event with coordinator_id: {}",
        coordinator_id
    );
    let test_event = create_test_coordinator_started_event(&coordinator_id);
    let expected_signature = create_event_signature(&test_event);

    println!("🎯 Expected event signature: {}", expected_signature);

    // Step 5: Send event via gRPC and start timing
    println!("🚀 Sending event via gRPC and starting roundtrip timer...");
    let send_start = std::time::Instant::now();

    let request = Request::new(test_event.clone());
    match grpc_client.submit_event(request).await {
        Ok(response) => {
            let resp = response.into_inner();
            if !resp.success {
                panic!("❌ Failed to send event: {}", resp.message);
            }
            assert_eq!(resp.processed_count, 1, "Expected 1 processed event");
            println!("  ✅ Event sent successfully via gRPC");
        }
        Err(e) => panic!("❌ Failed to send event via gRPC: {}", e),
    }

    // Step 6: Poll NATS subscription for the event with timeout
    println!("⏳ Polling NATS subscription for roundtrip event...");
    let roundtrip_timeout = Duration::from_secs(10);
    let poll_start = std::time::Instant::now();
    let mut attempt = 0;
    let mut success = false;
    let mut roundtrip_latency = Duration::ZERO;

    while poll_start.elapsed() < roundtrip_timeout && !success {
        attempt += 1;

        // Wait for NATS message with timeout
        let receive_timeout = Duration::from_millis(500);
        match timeout(receive_timeout, subscription.next()).await {
            Ok(Some(message)) => {
                roundtrip_latency = send_start.elapsed();

                // Try to deserialize the event
                match serde_json::from_slice::<Event>(&message.payload) {
                    Ok(received_event) => {
                        let received_signature = create_event_signature(&received_event);

                        if received_signature == expected_signature {
                            success = true;
                            println!(
                                "  ✅ SUCCESS: attempt={}, roundtrip_latency={}ms",
                                attempt,
                                roundtrip_latency.as_millis()
                            );

                            // Verify event content matches
                            if events_match_content(&test_event, &received_event) {
                                println!("     ✅ Event content verification: PERFECT MATCH");
                            } else {
                                println!("     ⚠️  Event content verification: DIFFERENT (but same signature)");
                            }

                            // Extract coordinator info for logging
                            if let Some(event::EventType::Coordinator(coord_event)) =
                                &received_event.event_type
                            {
                                if let Some(coordinator_event::Event::CoordinatorStarted(started)) =
                                    &coord_event.event
                                {
                                    println!("     📊 Received event: coordinator_id={}, ethereum_address={}", 
                                            started.coordinator_id, started.ethereum_address);
                                }
                            }
                        } else {
                            println!(
                                "    🔄 Attempt {}: Received different event (signature: {}, latency: {}ms)",
                                attempt, received_signature, roundtrip_latency.as_millis()
                            );
                        }
                    }
                    Err(e) => {
                        println!(
                            "    ⚠️  Attempt {}: Failed to deserialize NATS message: {} (latency: {}ms)",
                            attempt, e, roundtrip_latency.as_millis()
                        );
                    }
                }
            }
            Ok(None) => {
                println!(
                    "    🔄 Attempt {}: No NATS message received in {}ms timeout",
                    attempt,
                    receive_timeout.as_millis()
                );
            }
            Err(_) => {
                println!(
                    "    🔄 Attempt {}: NATS receive timeout after {}ms (total_time: {}ms)",
                    attempt,
                    receive_timeout.as_millis(),
                    poll_start.elapsed().as_millis()
                );
            }
        }
    }

    if !success {
        panic!(
            "❌ TIMEOUT: Event was not received from NATS within {}ms after {} attempts",
            roundtrip_timeout.as_millis(),
            attempt
        );
    }

    // Step 7: Report final results
    println!("\n🎉 NATS roundtrip latency test completed successfully!");
    println!("📊 Performance Summary:");
    println!("  - Event sent via gRPC and received from NATS ✅");
    println!("  - Roundtrip latency: {}ms", roundtrip_latency.as_millis());
    println!("  - NATS subject: {}", target_subject);
    println!("  - Event signature: {}", expected_signature);

    // Categorize latency performance
    let latency_ms = roundtrip_latency.as_millis();
    let performance_category = if latency_ms < 100 {
        "🚀 EXCELLENT"
    } else if latency_ms < 500 {
        "✅ GOOD"
    } else if latency_ms < 1000 {
        "⚠️  ACCEPTABLE"
    } else {
        "🐌 SLOW"
    };

    println!(
        "  - Performance rating: {} ({}ms)",
        performance_category, latency_ms
    );
    println!("  - Pipeline: gRPC → TiDB → NATS → Subscriber ✅");

    // Verify latency is reasonable (less than 5 seconds for this test)
    assert!(
        roundtrip_latency < Duration::from_secs(5),
        "Roundtrip latency should be less than 5 seconds, got {}ms",
        latency_ms
    );

    println!("  - Latency assertion passed: < 5000ms ✅");
}

fn create_test_coordinator_started_event(coordinator_id: &str) -> Event {
    let timestamp = get_current_timestamp();
    let unique_address = format!("0x{:040x}", timestamp); // Use timestamp for uniqueness

    Event {
        event_type: Some(event::EventType::Coordinator(CoordinatorEvent {
            event: Some(coordinator_event::Event::CoordinatorStarted(
                CoordinatorStartedEvent {
                    coordinator_id: coordinator_id.to_string(),
                    ethereum_address: unique_address,
                    sui_ed25519_address: format!("0x{:040x}", timestamp + 1),
                    event_timestamp: timestamp,
                },
            )),
        })),
    }
}

fn create_event_signature(event: &Event) -> String {
    // Create a unique signature based on stable fields (same logic as integration_test.rs)
    match &event.event_type {
        Some(event::EventType::Coordinator(coord_event)) => match &coord_event.event {
            Some(coordinator_event::Event::CoordinatorStarted(e)) => {
                format!("coord_started_{}_{}", e.coordinator_id, e.ethereum_address)
            }
            _ => "coord_other".to_string(),
        },
        _ => "unknown".to_string(),
    }
}

fn events_match_content(sent: &Event, received: &Event) -> bool {
    // Deep comparison of event content via JSON serialization
    match (serde_json::to_string(sent), serde_json::to_string(received)) {
        (Ok(sent_json), Ok(received_json)) => sent_json == received_json,
        _ => false,
    }
}

fn get_current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[tokio::test]
async fn test_nats_multiple_event_types_latency() {
    println!("🧪 Starting NATS multiple event types latency test...");
    println!("🎯 Testing different event types for latency comparison");

    // Connect to services
    let nats_client = async_nats::connect(NATS_URL)
        .await
        .expect("Failed to connect to NATS");
    let mut grpc_client = SilvanaEventsServiceClient::connect(SERVER_ADDR)
        .await
        .expect("Failed to connect to gRPC");

    let coordinator_id = get_unique_coordinator_id();
    let base_subject = format!("{}.events", NATS_STREAM_NAME);

    // Test different event types and their latencies
    let test_cases = vec![
        (
            "coordinator.started",
            create_coordinator_started_event(&coordinator_id),
        ),
        ("agent.message", create_agent_message_event(&coordinator_id)),
        (
            "agent.transaction",
            create_agent_transaction_event(&coordinator_id),
        ),
    ];

    let mut latency_results = Vec::new();

    for (event_type_name, test_event) in test_cases {
        println!("\n🔍 Testing event type: {}", event_type_name);

        // Setup subscription for this event type
        let subject = format!("{}.{}", base_subject, event_type_name);
        let mut subscription = nats_client
            .subscribe(subject.clone())
            .await
            .expect(&format!("Failed to subscribe to {}", subject));

        sleep(Duration::from_millis(50)).await; // Brief setup time

        // Send event and measure latency
        let send_start = std::time::Instant::now();
        let request = Request::new(test_event.clone());

        grpc_client
            .submit_event(request)
            .await
            .expect("Failed to send event");

        // Wait for NATS message
        let timeout_duration = Duration::from_secs(3);
        match timeout(timeout_duration, subscription.next()).await {
            Ok(Some(_message)) => {
                let latency = send_start.elapsed();
                latency_results.push((event_type_name, latency.as_millis()));
                println!("  ✅ Received via NATS: {}ms", latency.as_millis());
            }
            _ => {
                println!("  ❌ Timeout waiting for NATS message");
                latency_results.push((event_type_name, u128::MAX)); // Mark as failed
            }
        }
    }

    // Report comparative results
    println!("\n📊 Latency Comparison Results:");
    let mut successful_latencies = Vec::new();

    for (event_type, latency_ms) in &latency_results {
        if *latency_ms == u128::MAX {
            println!("  ❌ {}: TIMEOUT", event_type);
        } else {
            println!("  ✅ {}: {}ms", event_type, latency_ms);
            successful_latencies.push(*latency_ms);
        }
    }

    if !successful_latencies.is_empty() {
        let min_latency = successful_latencies.iter().min().unwrap();
        let max_latency = successful_latencies.iter().max().unwrap();
        let avg_latency =
            successful_latencies.iter().sum::<u128>() / successful_latencies.len() as u128;

        println!("\n📈 Aggregate Statistics:");
        println!("  - Min latency: {}ms", min_latency);
        println!("  - Max latency: {}ms", max_latency);
        println!("  - Avg latency: {}ms", avg_latency);
        println!("  - Latency range: {}ms", max_latency - min_latency);
        println!(
            "  - Successful tests: {}/{}",
            successful_latencies.len(),
            latency_results.len()
        );
    }

    // Ensure at least one test succeeded
    assert!(
        !successful_latencies.is_empty(),
        "At least one event type should succeed"
    );

    println!("\n🎉 Multiple event types latency test completed!");
}

fn create_coordinator_started_event(coordinator_id: &str) -> Event {
    create_test_coordinator_started_event(coordinator_id)
}

fn create_agent_message_event(coordinator_id: &str) -> Event {
    Event {
        event_type: Some(event::EventType::Agent(AgentEvent {
            event: Some(agent_event::Event::Message(AgentMessageEvent {
                coordinator_id: coordinator_id.to_string(),
                developer: "nats-test-developer".to_string(),
                agent: "nats-test-agent".to_string(),
                app: "nats-test-app".to_string(),
                job_id: format!("nats-job-{}", get_current_timestamp()),
                sequences: vec![1],
                event_timestamp: get_current_timestamp(),
                level: 1, // Info
                message: format!("NATS test message at {}", get_current_timestamp()),
            })),
        })),
    }
}

fn create_agent_transaction_event(coordinator_id: &str) -> Event {
    let timestamp = get_current_timestamp();

    Event {
        event_type: Some(event::EventType::Agent(AgentEvent {
            event: Some(agent_event::Event::Transaction(AgentTransactionEvent {
                coordinator_id: coordinator_id.to_string(),
                tx_type: "nats_test".to_string(),
                developer: "nats-test-developer".to_string(),
                agent: "nats-test-agent".to_string(),
                app: "nats-test-app".to_string(),
                job_id: format!("nats-job-{}", timestamp),
                sequences: vec![1],
                event_timestamp: timestamp,
                tx_hash: format!("0x{:064x}", timestamp),
                chain: "ethereum".to_string(),
                network: "testnet".to_string(),
                memo: format!("NATS test transaction at {}", timestamp),
                metadata: format!(r#"{{"test_timestamp": {}}}"#, timestamp),
            })),
        })),
    }
}
