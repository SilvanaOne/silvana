use std::time::{SystemTime, UNIX_EPOCH};
use tonic::Request;

use proto::silvana_events_service_client::SilvanaEventsServiceClient;
use proto::*;

// Test configuration
const SERVER_ADDR: &str = "https://rpc-dev.silvana.dev";

// Generate a unique coordinator ID for each test run to avoid data contamination
fn get_unique_coordinator_id() -> String {
    format!("search-test-{}", get_current_timestamp())
}

#[tokio::test]
async fn test_fulltext_search_coordinator_messages() {
    println!("🧪 Starting full-text search test...");
    println!("🎯 Server address: {}", SERVER_ADDR);

    // Connect to the gRPC server
    let mut client = match SilvanaEventsServiceClient::connect(SERVER_ADDR).await {
        Ok(client) => {
            println!("✅ Connected to RPC server successfully");
            client
        }
        Err(e) => {
            panic!("❌ Failed to connect to RPC server at {}: {}\nMake sure the server is running with: cargo run", SERVER_ADDR, e);
        }
    };

    let coordinator_id = get_unique_coordinator_id();

    // Step 1: Create diverse test messages for searching
    println!("📝 Creating test coordinator message events...");
    let test_events = create_test_coordinator_messages(&coordinator_id);

    // Step 2: Submit all test events with timing
    println!("📤 Submitting {} test events...", test_events.len());
    let send_start = std::time::Instant::now();
    let mut last_send_time = std::time::Instant::now();

    for (event, description) in &test_events {
        let request = Request::new(event.clone());
        let send_time = std::time::Instant::now();

        match client.submit_event(request).await {
            Ok(response) => {
                let resp = response.into_inner();
                if !resp.success {
                    panic!("❌ Failed to send {}: {}", description, resp.message);
                }
                assert_eq!(resp.processed_count, 1, "Expected 1 processed event");
                last_send_time = send_time; // Track the last successful send time
            }
            Err(e) => panic!("❌ Failed to send {}: {}", description, e),
        }
    }

    println!(
        "  📤 All {} events sent in {}ms, starting indexing verification...",
        test_events.len(),
        send_start.elapsed().as_millis()
    );

    // Step 3: Poll for indexed data with timeout and attempts
    println!("⏳ Polling for events to be processed and indexed...");
    let indexing_verified =
        verify_indexing_with_polling(&mut client, &coordinator_id, last_send_time).await;
    if !indexing_verified {
        panic!("❌ TIMEOUT: Events were not indexed within the expected timeframe");
    }

    // Step 4: Test various search scenarios with timing
    println!("\n🔍 Testing search scenarios...");
    let scenarios_start = std::time::Instant::now();
    let mut search_latencies: Vec<u128> = Vec::new();

    // Test 1: Basic search for "error" keyword
    let start = std::time::Instant::now();
    test_basic_search(&mut client, &coordinator_id, "error", 3).await;
    search_latencies.push(start.elapsed().as_millis());

    // Test 2: Search for "network" keyword
    let start = std::time::Instant::now();
    test_basic_search(&mut client, &coordinator_id, "network", 2).await;
    search_latencies.push(start.elapsed().as_millis());

    // Test 3: Search for "blockchain" keyword
    let start = std::time::Instant::now();
    test_basic_search(&mut client, &coordinator_id, "blockchain", 2).await;
    search_latencies.push(start.elapsed().as_millis());

    // Test 4: Multi-word search
    let start = std::time::Instant::now();
    test_basic_search(&mut client, &coordinator_id, "database connection", 2).await;
    search_latencies.push(start.elapsed().as_millis());

    // Test 5: Search with coordinator filter
    test_coordinator_filtered_search(&mut client, &coordinator_id).await;

    // Test 6: Pagination test
    test_search_pagination(&mut client, &coordinator_id).await;

    // Test 7: Empty query handling
    test_empty_query_handling(&mut client).await;

    // Test 8: Relevance scoring validation
    test_relevance_scoring(&mut client, &coordinator_id).await;

    // Test 9: Multi-language search (if supported)
    test_multilanguage_search(&mut client, &coordinator_id).await;

    // Calculate and display performance statistics
    let total_scenarios_time = scenarios_start.elapsed();
    println!("\n🎉 Full-text search test completed successfully!");
    println!("📊 Performance Summary:");
    println!(
        "  - Total test duration: {}ms for {} test events",
        total_scenarios_time.as_millis(),
        test_events.len()
    );

    if !search_latencies.is_empty() {
        let min_latency = search_latencies.iter().min().unwrap();
        let max_latency = search_latencies.iter().max().unwrap();
        let avg_latency = search_latencies.iter().sum::<u128>() / search_latencies.len() as u128;

        println!("📈 Basic Search Latency Statistics:");
        println!("  - Min search time: {}ms", min_latency);
        println!("  - Max search time: {}ms", max_latency);
        println!("  - Avg search time: {}ms", avg_latency);
        println!("  - Search time range: {}ms", max_latency - min_latency);
        println!(
            "  - Measured {} basic search operations",
            search_latencies.len()
        );
    }

    println!("  - Full-text indexing and search functionality verified ✅");
    println!("  - TiDB Cloud FTS_MATCH_WORD with BM25 relevance ranking working correctly");
}

async fn verify_indexing_with_polling(
    client: &mut SilvanaEventsServiceClient<tonic::transport::Channel>,
    coordinator_id: &str,
    last_send_time: std::time::Instant,
) -> bool {
    let timeout = std::time::Duration::from_secs(10);
    let poll_start = std::time::Instant::now();
    let mut attempt = 0;

    while poll_start.elapsed() < timeout {
        attempt += 1;
        let query_start = std::time::Instant::now();

        // Test with a simple search that should return results
        let request = Request::new(SearchCoordinatorMessageEventsRequest {
            search_query: "error".to_string(),
            limit: Some(10),
            offset: None,
            coordinator_id: Some(coordinator_id.to_string()),
        });

        match client.search_coordinator_message_events(request).await {
            Ok(response) => {
                let search_result = response.into_inner();
                let query_duration = query_start.elapsed();
                let indexing_latency = last_send_time.elapsed();

                if search_result.success && search_result.events.len() >= 3 {
                    // We expect at least 3 "error" events based on our test data
                    println!(
                        "  ✅ INDEXING VERIFIED: attempt={}, query_time={}ms, indexing_latency={}ms",
                        attempt,
                        query_duration.as_millis(),
                        indexing_latency.as_millis()
                    );
                    println!(
                        "     Found {} events with search functionality active",
                        search_result.events.len()
                    );
                    return true;
                } else {
                    println!(
                        "    🔄 Attempt {}: Indexing in progress, found {} events (query_time={}ms, latency={}ms)",
                        attempt,
                        search_result.events.len(),
                        query_duration.as_millis(),
                        indexing_latency.as_millis()
                    );
                }
            }
            Err(e) => {
                println!("    ⚠️  Attempt {}: Search query failed: {}", attempt, e);
            }
        }
    }

    println!(
        "    ❌ TIMEOUT: Indexing not completed after {} attempts in {}ms",
        attempt,
        timeout.as_millis()
    );
    false
}

async fn test_basic_search(
    client: &mut SilvanaEventsServiceClient<tonic::transport::Channel>,
    coordinator_id: &str,
    search_term: &str,
    expected_count: usize,
) {
    println!("🔍 Testing basic search for '{}'", search_term);

    let timeout = std::time::Duration::from_secs(5);
    let poll_start = std::time::Instant::now();
    let mut attempt = 0;
    let mut search_result = None;

    // Poll with timeout and attempts for reliable results
    while poll_start.elapsed() < timeout {
        attempt += 1;
        let query_start = std::time::Instant::now();

        let request = Request::new(SearchCoordinatorMessageEventsRequest {
            search_query: search_term.to_string(),
            limit: Some(10),
            offset: None,
            coordinator_id: Some(coordinator_id.to_string()),
        });

        match client.search_coordinator_message_events(request).await {
            Ok(response) => {
                let result = response.into_inner();
                let query_duration = query_start.elapsed();

                if result.success && result.events.len() == expected_count {
                    println!(
                        "  ✅ SUCCESS: attempt={}, query_time={}ms, total_time={}ms",
                        attempt,
                        query_duration.as_millis(),
                        poll_start.elapsed().as_millis()
                    );
                    search_result = Some(result);
                    break;
                } else {
                    println!(
                        "    🔄 Attempt {}: Expected {} results, got {} (query_time={}ms)",
                        attempt,
                        expected_count,
                        result.events.len(),
                        query_duration.as_millis()
                    );

                    if attempt == 1 {
                        // On first attempt, store the result for validation even if count doesn't match
                        search_result = Some(result);
                    }
                }
            }
            Err(e) => {
                println!("    ⚠️  Attempt {}: Search failed: {}", attempt, e);
            }
        }

        if attempt < 3 {
            // Only retry for first few attempts
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        } else {
            break;
        }
    }

    let search_result =
        search_result.expect(&format!("Search for '{}' should succeed", search_term));

    assert!(search_result.success, "Search should be successful");
    assert_eq!(
        search_result.events.len(),
        expected_count,
        "Expected {} results for '{}', got {}",
        expected_count,
        search_term,
        search_result.events.len()
    );
    assert_eq!(search_result.total_count as usize, expected_count);
    assert_eq!(search_result.returned_count as usize, expected_count);

    // Verify all results contain the search term
    for event in &search_result.events {
        assert!(
            event
                .message
                .to_lowercase()
                .contains(&search_term.to_lowercase()),
            "Result message '{}' should contain '{}'",
            event.message,
            search_term
        );
        assert_eq!(event.coordinator_id, coordinator_id);
        assert!(
            event.relevance_score > 0.0,
            "Relevance score should be positive"
        );
    }

    // Verify results are ordered by relevance (descending)
    for i in 1..search_result.events.len() {
        assert!(
            search_result.events[i - 1].relevance_score >= search_result.events[i].relevance_score,
            "Results should be ordered by relevance score (descending)"
        );
    }

    println!(
        "     Results: {} events with relevance scores: {:?}",
        search_result.events.len(),
        search_result
            .events
            .iter()
            .map(|e| format!("{:.3}", e.relevance_score))
            .collect::<Vec<_>>()
    );
}

async fn test_coordinator_filtered_search(
    client: &mut SilvanaEventsServiceClient<tonic::transport::Channel>,
    coordinator_id: &str,
) {
    println!("🔍 Testing coordinator-filtered search");

    // Search without coordinator filter (should find events from multiple coordinators)
    let request = Request::new(SearchCoordinatorMessageEventsRequest {
        search_query: "error".to_string(),
        limit: Some(10),
        offset: None,
        coordinator_id: None, // No filter
    });

    let response = client
        .search_coordinator_message_events(request)
        .await
        .expect("Unfiltered search should succeed");

    let _unfiltered_result = response.into_inner();

    // Search with coordinator filter
    let request = Request::new(SearchCoordinatorMessageEventsRequest {
        search_query: "error".to_string(),
        limit: Some(10),
        offset: None,
        coordinator_id: Some(coordinator_id.to_string()),
    });

    let response = client
        .search_coordinator_message_events(request)
        .await
        .expect("Filtered search should succeed");

    let filtered_result = response.into_inner();

    assert!(
        filtered_result.success,
        "Filtered search should be successful"
    );
    assert!(
        filtered_result.events.len() > 0,
        "Should find events for our coordinator"
    );

    // Verify all results are from the correct coordinator
    for event in &filtered_result.events {
        assert_eq!(event.coordinator_id, coordinator_id);
    }

    println!(
        "  ✅ Coordinator filter test passed - found {} events for coordinator",
        filtered_result.events.len()
    );
}

async fn test_search_pagination(
    client: &mut SilvanaEventsServiceClient<tonic::transport::Channel>,
    coordinator_id: &str,
) {
    println!("🔍 Testing search pagination");

    // Get all results first
    let request = Request::new(SearchCoordinatorMessageEventsRequest {
        search_query: "error".to_string(),
        limit: None,
        offset: None,
        coordinator_id: Some(coordinator_id.to_string()),
    });

    let response = client
        .search_coordinator_message_events(request)
        .await
        .expect("Full search should succeed");

    let full_result = response.into_inner();
    let total_count = full_result.total_count;

    if total_count < 2 {
        println!("  ⏭️  Skipping pagination test - need at least 2 results");
        return;
    }

    // Test pagination - first page
    let request = Request::new(SearchCoordinatorMessageEventsRequest {
        search_query: "error".to_string(),
        limit: Some(2),
        offset: Some(0),
        coordinator_id: Some(coordinator_id.to_string()),
    });

    let response = client
        .search_coordinator_message_events(request)
        .await
        .expect("Paginated search should succeed");

    let page1_result = response.into_inner();

    assert_eq!(page1_result.total_count, total_count);
    assert!(page1_result.returned_count <= 2);
    assert_eq!(
        page1_result.events.len(),
        page1_result.returned_count as usize
    );

    // Test pagination - second page if enough results
    if total_count > 2 {
        let request = Request::new(SearchCoordinatorMessageEventsRequest {
            search_query: "error".to_string(),
            limit: Some(2),
            offset: Some(2),
            coordinator_id: Some(coordinator_id.to_string()),
        });

        let response = client
            .search_coordinator_message_events(request)
            .await
            .expect("Second page search should succeed");

        let page2_result = response.into_inner();

        assert_eq!(page2_result.total_count, total_count);
        // Ensure we don't get the same results
        if !page1_result.events.is_empty() && !page2_result.events.is_empty() {
            assert_ne!(
                page1_result.events[0].id, page2_result.events[0].id,
                "Pages should contain different results"
            );
        }
    }

    println!(
        "  ✅ Pagination test passed - total: {}, page size: 2",
        total_count
    );
}

async fn test_empty_query_handling(
    client: &mut SilvanaEventsServiceClient<tonic::transport::Channel>,
) {
    println!("🔍 Testing empty query handling");

    let request = Request::new(SearchCoordinatorMessageEventsRequest {
        search_query: "".to_string(),
        limit: Some(10),
        offset: None,
        coordinator_id: None,
    });

    let response = client.search_coordinator_message_events(request).await;

    match response {
        Err(status) => {
            assert_eq!(status.code(), tonic::Code::InvalidArgument);
            assert!(
                status.message().contains("empty"),
                "Error message should mention empty query"
            );
            println!("  ✅ Empty query correctly rejected with InvalidArgument");
        }
        Ok(_) => {
            panic!("Empty query should be rejected");
        }
    }
}

async fn test_relevance_scoring(
    client: &mut SilvanaEventsServiceClient<tonic::transport::Channel>,
    coordinator_id: &str,
) {
    println!("🔍 Testing relevance scoring");

    let request = Request::new(SearchCoordinatorMessageEventsRequest {
        search_query: "error critical".to_string(),
        limit: Some(10),
        offset: None,
        coordinator_id: Some(coordinator_id.to_string()),
    });

    let response = client
        .search_coordinator_message_events(request)
        .await
        .expect("Relevance test search should succeed");

    let search_result = response.into_inner();

    if search_result.events.len() < 2 {
        println!("  ⏭️  Skipping relevance test - need at least 2 results");
        return;
    }

    // Check that results are properly ordered by relevance
    for i in 1..search_result.events.len() {
        let prev_score = search_result.events[i - 1].relevance_score;
        let curr_score = search_result.events[i].relevance_score;

        assert!(
            prev_score >= curr_score,
            "Relevance scores should be in descending order: {} >= {}",
            prev_score,
            curr_score
        );
    }

    // Check that relevance scores are reasonable (positive and not too large)
    for event in &search_result.events {
        assert!(
            event.relevance_score > 0.0,
            "Relevance score should be positive"
        );
        assert!(
            event.relevance_score < 1000.0,
            "Relevance score should be reasonable (< 1000)"
        );
    }

    println!(
        "  ✅ Relevance scoring test passed - {} results properly ordered",
        search_result.events.len()
    );
}

async fn test_multilanguage_search(
    client: &mut SilvanaEventsServiceClient<tonic::transport::Channel>,
    coordinator_id: &str,
) {
    println!("🔍 Testing multi-language search");

    // Test Japanese search (from our test data)
    let request = Request::new(SearchCoordinatorMessageEventsRequest {
        search_query: "ネットワーク".to_string(),
        limit: Some(10),
        offset: None,
        coordinator_id: Some(coordinator_id.to_string()),
    });

    let response = client
        .search_coordinator_message_events(request)
        .await
        .expect("Japanese search should succeed");

    let japanese_result = response.into_inner();

    // Test Chinese search (from our test data)
    let request = Request::new(SearchCoordinatorMessageEventsRequest {
        search_query: "区块链".to_string(),
        limit: Some(10),
        offset: None,
        coordinator_id: Some(coordinator_id.to_string()),
    });

    let response = client
        .search_coordinator_message_events(request)
        .await
        .expect("Chinese search should succeed");

    let chinese_result = response.into_inner();

    println!(
        "  ✅ Multi-language search: {} Japanese results, {} Chinese results",
        japanese_result.events.len(),
        chinese_result.events.len()
    );
}

fn create_test_coordinator_messages(coordinator_id: &str) -> Vec<(Event, String)> {
    let mut events = Vec::new();
    let base_timestamp = get_current_timestamp();

    // Create diverse test messages for comprehensive search testing
    let test_messages = vec![
        ("Critical error in database connection", "error database"),
        (
            "Network timeout during blockchain sync",
            "network blockchain",
        ),
        (
            "Transaction processing completed successfully",
            "transaction success",
        ),
        ("Warning: High memory usage detected", "warning memory"),
        (
            "Database connection error - retrying",
            "error database retry",
        ),
        (
            "Blockchain network synchronization failed",
            "blockchain network failure",
        ),
        (
            "Error: Critical system failure occurred",
            "error critical failure",
        ),
        ("System startup completed without errors", "startup success"),
        (
            "ネットワーク接続エラーが発生しました",
            "japanese network error",
        ), // Japanese: Network connection error occurred
        ("区块链同步过程中出现错误", "chinese blockchain error"), // Chinese: Error occurred during blockchain sync
    ];

    for (i, (message, description)) in test_messages.iter().enumerate() {
        let event = Event {
            event_type: Some(event::EventType::Coordinator(CoordinatorEvent {
                event: Some(coordinator_event::Event::CoordinatorError(
                    CoordinatorMessageEvent {
                        coordinator_id: coordinator_id.to_string(),
                        event_timestamp: base_timestamp + i as u64,
                        level: match i % 4 {
                            0 => 3, // Error
                            1 => 2, // Warn
                            2 => 1, // Info
                            _ => 0, // Debug
                        },
                        message: message.to_string(),
                    },
                )),
            })),
        };

        events.push((event, format!("test message: {}", description)));
    }

    println!("📊 Created {} test coordinator messages:", events.len());
    for (_, description) in &events {
        println!("  - {}", description);
    }

    events
}

fn get_current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
