use futures::StreamExt;
use futures::TryStreamExt;
use prost::Message;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{Duration, timeout};
use tonic::Request;

use proto::silvana_rpc_service_client::SilvanaRpcServiceClient;
use proto::*;

// Test configuration
fn get_server_addr() -> String {
    // Load .env file if it exists
    dotenvy::dotenv().ok();

    // Get TEST_SERVER from environment, fallback to default if not set
    std::env::var("TEST_SERVER").unwrap_or_else(|_| "http://127.0.0.1:50051".to_string())
}
const NATS_URL: &str = "nats://rpc-devnet.silvana.dev:4222";
const NATS_STREAM_NAME: &str = "silvana";

// Generate a unique coordinator ID for each test run to avoid data contamination
fn get_unique_coordinator_id() -> String {
    format!("nats-test-{}", get_current_timestamp())
}

#[tokio::test]
async fn test_nats_roundtrip_latency() {
    // Initialize Rustls crypto provider for HTTPS connections
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let server_addr = get_server_addr();
    println!("🧪 Starting NATS roundtrip latency test...");
    println!("🎯 gRPC Server: {}", server_addr);
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
    let jetstream = async_nats::jetstream::new(nats_client);

    // Step 2: Connect to gRPC server
    let mut grpc_client = match SilvanaRpcServiceClient::connect(server_addr.clone()).await {
        Ok(client) => {
            println!("✅ Connected to gRPC server successfully");
            client
        }
        Err(e) => {
            panic!(
                "❌ Failed to connect to gRPC server at {}: {}\nMake sure the server is running with: cargo run",
                server_addr, e
            );
        }
    };

    // Step 3: Setup NATS subscription for the specific event type we'll send
    let coordinator_id = get_unique_coordinator_id();
    let consumer_name = format!("nats-test-{}", coordinator_id);

    println!("📡 Setting up NATS subscription to: {}", NATS_STREAM_NAME);
    let stream = match jetstream
        .get_or_create_stream(async_nats::jetstream::stream::Config {
            name: NATS_STREAM_NAME.to_string(),
            ..Default::default()
        })
        .await
    {
        Ok(stream) => {
            println!("✅ Stream created successfully");
            stream
        }
        Err(e) => {
            panic!("❌ Failed to create stream: {}", e);
        }
    };
    let consumer = match stream
        .get_or_create_consumer(
            &consumer_name,
            async_nats::jetstream::consumer::pull::Config {
                durable_name: Some(consumer_name.clone()),
                // Only get new messages, not replay from the beginning
                deliver_policy: async_nats::jetstream::consumer::DeliverPolicy::New,
                ..Default::default()
            },
        )
        .await
    {
        Ok(consumer) => {
            println!(
                "✅ Consumer created successfully with name: {}",
                consumer_name
            );
            consumer
        }
        Err(e) => {
            panic!("❌ Failed to create consumer: {}", e);
        }
    };

    // let mut subscription = match nats_client.subscribe(target_subject.clone()).await {
    //     Ok(sub) => {
    //         println!("✅ NATS subscription established successfully");
    //         sub
    //     }
    //     Err(e) => {
    //         panic!(
    //             "❌ Failed to subscribe to NATS subject {}: {}",
    //             target_subject, e
    //         );
    //     }
    // };

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

    let request = Request::new(SubmitEventRequest {
        event: Some(test_event.clone()),
    });
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
        match timeout(receive_timeout, consumer.messages()).await {
            Ok(messages_stream) => {
                let mut messages = match messages_stream {
                    Ok(messages) => messages.take(100),
                    Err(e) => {
                        panic!("❌ Failed to get messages: {}", e);
                    }
                };

                while let Ok(Some(message)) = messages.try_next().await {
                    if let Err(e) = message.ack().await {
                        println!("⚠️  Failed to ack message: {}", e);
                    }

                    roundtrip_latency = send_start.elapsed();

                    // Try to deserialize the event
                    match proto::events::Event::decode(message.payload.clone()) {
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
                                    println!(
                                        "     ⚠️  Event content verification: DIFFERENT (but same signature)"
                                    );
                                }

                                // Extract coordinator info for logging
                                if let Some(event::Event::CoordinatorStarted(started)) = &received_event.event {
                                    println!(
                                        "     📊 Received event: coordinator_id={}, ethereum_address={}",
                                        started.coordinator_id, started.ethereum_address
                                    );
                                }
                                break; // Exit the inner loop once we find the matching message
                            } else {
                                println!(
                                    "    🔄 Attempt {}: Received different event (signature: {}, latency: {}ms)",
                                    attempt,
                                    received_signature,
                                    roundtrip_latency.as_millis()
                                );
                            }
                        }
                        Err(e) => {
                            println!(
                                "    ⚠️  Attempt {}: Failed to deserialize NATS message: {} (latency: {}ms)",
                                attempt,
                                e,
                                roundtrip_latency.as_millis()
                            );
                        }
                    }
                }
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
    println!("  - NATS subject: {}", NATS_STREAM_NAME);
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
        event: Some(event::Event::CoordinatorStarted(CoordinatorStartedEvent {
            coordinator_id: coordinator_id.to_string(),
            ethereum_address: unique_address,
            event_timestamp: timestamp,
        })),
    }
}

fn create_event_signature(event: &Event) -> String {
    // Create a unique signature based on stable fields (same logic as integration_test.rs)
    match &event.event {
        Some(event::Event::CoordinatorStarted(e)) => {
            format!("coord_started_{}_{}", e.coordinator_id, e.ethereum_address)
        }
        _ => "unknown".to_string(),
    }
}

fn events_match_content(sent: &Event, received: &Event) -> bool {
    // Deep comparison of event content via protobuf serialization
    sent.encode_to_vec() == received.encode_to_vec()
}

fn get_current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
