use rpc::database::EventDatabase;
use rpc::database_search::search_all_events_parallel;
use rpc::proto;
use std::env;
use tracing::{info, debug};

#[tokio::test]
async fn test_search_coordinator_id() -> Result<(), Box<dyn std::error::Error>> {
    // Load environment variables from .env file manually
    dotenvy::dotenv().ok();

    // Get database URL from environment
    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set in .env file");

    println!("Connecting to database...");

    // Create database connection
    let database = EventDatabase::new(&database_url).await?;

    // First, let's check what's actually in the database
    println!("Checking database contents...");
    {
        use sea_orm::{ConnectionTrait, Statement};
        let conn = rpc::database::get_connection(&database);

        let query = "SELECT COUNT(*) as count FROM agent_message_event";
        let stmt = Statement::from_sql_and_values(sea_orm::DatabaseBackend::MySql, query, vec![]);
        if let Ok(result) = conn.query_one(stmt).await {
            if let Some(row) = result {
                let count: i64 = row.try_get_by_index(0).unwrap_or(0);
                println!("Total agent_message_event records: {}", count);
            }
        }

        // Check for a specific event
        let query2 = "SELECT id, coordinator_id FROM agent_message_event ORDER BY id DESC LIMIT 5";
        let stmt2 = Statement::from_sql_and_values(sea_orm::DatabaseBackend::MySql, query2, vec![]);
        if let Ok(results) = conn.query_all(stmt2).await {
            println!("Latest 5 agent_message_event records:");
            for row in results {
                if let (Ok(id), Ok(coord)) = (row.try_get_by_index::<i64>(0), row.try_get_by_index::<String>(1)) {
                    println!("  ID: {}, Coordinator: {}", id, &coord[..20.min(coord.len())]);
                }
            }
        }
    }

    // Test search queries - just test one for now to debug
    let test_queries = vec![
        "0x7d8e26e7da287c86e3fc3fab644dae00d4730ae32c69cd9814d2757130c116d8",
    ];

    // Debug logging is built into the search function

    for query in test_queries {
        println!("\n===========================================");
        println!("Testing search for: {}", query);
        println!("===========================================");

        // Add some temporary debug output
        info!("Starting search for query: {}", query);
        debug!("Database connection established");

        let result = search_all_events_parallel(&database, query, Some(10)).await?;

        println!("Found {} events (total_count: {}, returned_count: {})",
                 result.events.len(), result.total_count, result.returned_count);

        // Let's also test with a direct LIKE query to compare
        println!("\nDirect SQL test:");
        {
            use sea_orm::{ConnectionTrait, Statement};
            let conn = rpc::database::get_connection(&database);
            let direct_query = format!(
                "SELECT id, coordinator_id FROM agent_message_event WHERE coordinator_id LIKE '%{}%' LIMIT 5",
                query
            );
            let stmt = Statement::from_sql_and_values(sea_orm::DatabaseBackend::MySql, &direct_query, vec![]);
            if let Ok(results) = conn.query_all(stmt).await {
                println!("Direct SQL found {} rows", results.len());
                for row in results {
                    if let Ok(id) = row.try_get_by_index::<i64>(0) {
                        println!("  Found ID: {}", id);
                    }
                }
            }
        }

        for (i, event) in result.events.iter().enumerate() {
            println!("\nEvent #{}: ", i + 1);
            println!("  Relevance score: {}", event.relevance_score);

            if let Some(ref event_data) = event.event {
                if let Some(ref event_case) = event_data.event {
                    match event_case {
                        proto::event::Event::AgentMessage(msg) => {
                            println!("  Type: AgentMessage");
                            println!("  Coordinator ID: {}", msg.coordinator_id);
                            println!("  Message: {}", msg.message);
                        },
                        proto::event::Event::CoordinatorMessage(msg) => {
                            println!("  Type: CoordinatorMessage");
                            println!("  Coordinator ID: {}", msg.coordinator_id);
                            println!("  Message: {}", msg.message);
                        },
                        _ => {
                            println!("  Type: Other event type");
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

#[tokio::test]
async fn test_direct_database_query() -> Result<(), Box<dyn std::error::Error>> {
    use sea_orm::{Database, ConnectionTrait, Statement};
    use std::env;

    // Load environment variables
    dotenvy::dotenv().ok();

    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set in .env file");

    println!("Connecting directly to database...");
    let conn = Database::connect(database_url).await?;

    // Test 1: Check if we can find events by coordinator_id with LIKE
    let query1 = "SELECT id, coordinator_id, message FROM agent_message_event WHERE coordinator_id LIKE '%0x7d8e26e7da287c86e3fc3fab644dae00d4730ae32c69cd9814d2757130c116d8%' LIMIT 5";
    println!("\nRunning query: {}", query1);

    let stmt = Statement::from_sql_and_values(sea_orm::DatabaseBackend::MySql, query1, vec![]);
    let results = conn.query_all(stmt).await?;

    println!("Found {} rows with LIKE query", results.len());
    for row in &results {
        let id: i64 = row.try_get_by_index(0)?;
        let coord_id: String = row.try_get_by_index(1)?;
        let message: String = row.try_get_by_index(2)?;
        println!("  ID: {}, Coordinator: {}, Message: {}", id, &coord_id[..20], &message[..50]);
    }

    // Test 2: If we found an ID, try to fetch it directly
    if let Some(first_row) = results.first() {
        let id: i64 = first_row.try_get_by_index(0)?;

        let query2 = format!("SELECT * FROM agent_message_event WHERE id = {}", id);
        println!("\nRunning query: {}", query2);

        let stmt = Statement::from_sql_and_values(sea_orm::DatabaseBackend::MySql, &query2, vec![]);
        let result = conn.query_one(stmt).await?;

        if let Some(row) = result {
            println!("Successfully fetched event with ID {}", id);
            let coord_id: String = row.try_get_by_index(1)?;
            println!("  Coordinator ID: {}", coord_id);
        } else {
            println!("ERROR: Could not fetch event with ID {}", id);
        }
    }

    // Test 3: Check the actual primary key column return
    let query3 = "SELECT id as pk, 'agent_message_event' as table_name, 'coordinator_id' as matched_column, 1.0 as relevance_score FROM agent_message_event WHERE coordinator_id LIKE '%0x7d8e26e7da287c86e3fc3fab644dae00d4730ae32c69cd9814d2757130c116d8%' LIMIT 5";
    println!("\nRunning exact search query: {}", query3);

    let stmt = Statement::from_sql_and_values(sea_orm::DatabaseBackend::MySql, query3, vec![]);
    let results = conn.query_all(stmt).await?;

    println!("Query results:");
    for row in &results {
        // Try different ways to get the pk value
        match row.try_get_by_index::<i64>(0) {
            Ok(pk) => println!("  pk as i64: {}", pk),
            Err(e) => println!("  Failed to get pk as i64: {:?}", e),
        }

        match row.try_get_by_index::<String>(0) {
            Ok(pk) => println!("  pk as String: {}", pk),
            Err(e) => println!("  Failed to get pk as String: {:?}", e),
        }

        let table_name: String = row.try_get_by_index(1)?;
        println!("  table_name: {}", table_name);
    }

    Ok(())
}