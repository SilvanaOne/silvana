use rpc::database::EventDatabase;
use rpc::proto::{GetEventsByAppInstanceSequenceRequest};
use rpc::rpc::get_events_by_app_instance_sequence_impl;
use std::env;
use tracing::{info, debug};

#[tokio::test]
async fn test_get_events_by_sequence() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Load environment variables from .env file manually
    dotenvy::dotenv().ok();

    // Get database URL from environment
    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set in .env file");

    println!("Connecting to database...");

    // Create database connection
    let database = EventDatabase::new(&database_url).await?;

    // Test parameters from the user's example
    let app_instance_id = "0xe4094d71ce851dda51efd6a459a1d096c414fd899ac79e655a1349f39288d45e";
    let sequence = 4u64;

    println!("\n=== Testing sequence lookup for app_instance={}, sequence={} ===", app_instance_id, sequence);

    // First, let's check what's actually in the database for this app instance
    {
        use sea_orm::{ConnectionTrait, Statement, Value};
        let conn = rpc::database::get_connection(&database);

        // Check job_created_event table
        println!("\n1. Checking job_created_event table:");
        let query = "SELECT job_sequence, app_instance_id, event_timestamp FROM job_created_event WHERE app_instance_id = ? ORDER BY job_sequence DESC LIMIT 10";
        let stmt = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::MySql,
            query,
            vec![Value::String(Some(Box::new(app_instance_id.to_string())))],
        );

        if let Ok(results) = conn.query_all(stmt).await {
            println!("Found {} job_created_event records for this app instance", results.len());
            for row in results {
                if let (Ok(seq), Ok(app_id), Ok(ts)) = (
                    row.try_get_by_index::<i64>(0),
                    row.try_get_by_index::<String>(1),
                    row.try_get_by_index::<i64>(2),
                ) {
                    println!("  job_sequence: {}, app_instance_id: {}, timestamp: {}", seq, &app_id[..32.min(app_id.len())], ts);
                }
            }
        }

        // Check proof_event table
        println!("\n2. Checking proof_event table:");
        let query2 = "SELECT sequences, app_instance_id, event_timestamp FROM proof_event WHERE app_instance_id = ? ORDER BY event_timestamp DESC LIMIT 10";
        let stmt2 = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::MySql,
            query2,
            vec![Value::String(Some(Box::new(app_instance_id.to_string())))],
        );

        if let Ok(results) = conn.query_all(stmt2).await {
            println!("Found {} proof_event records for this app instance", results.len());
            for row in results {
                if let (Ok(seq), Ok(app_id), Ok(ts)) = (
                    row.try_get_by_index::<String>(0),
                    row.try_get_by_index::<String>(1),
                    row.try_get_by_index::<i64>(2),
                ) {
                    println!("  sequences: {}, app_instance_id: {}, timestamp: {}", seq, &app_id[..32.min(app_id.len())], ts);
                }
            }
        }

        // Check proof_event table with sequences containing "4"
        println!("\n3. Checking proof_event table for sequences containing '4':");
        let query3 = "SELECT sequences, app_instance_id, event_timestamp, job_id FROM proof_event WHERE app_instance_id = ? AND sequences LIKE '%4%' ORDER BY event_timestamp DESC LIMIT 10";
        let stmt3 = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::MySql,
            query3,
            vec![Value::String(Some(Box::new(app_instance_id.to_string())))],
        );

        if let Ok(results) = conn.query_all(stmt3).await {
            println!("Found {} proof_event records with sequence containing '4'", results.len());
            for row in results {
                if let (Ok(seq), Ok(app_id), Ok(ts), Ok(job_id)) = (
                    row.try_get_by_index::<String>(0),
                    row.try_get_by_index::<String>(1),
                    row.try_get_by_index::<i64>(2),
                    row.try_get_by_index::<String>(3),
                ) {
                    println!("  sequences: {}, app_instance_id: {}, timestamp: {}, job_id: {}", seq, &app_id[..32.min(app_id.len())], ts, job_id);
                }
            }
        }

        // Let's also check if the sequences field is stored as JSON or as a simple string
        println!("\n4. Checking proof_event sequences field format:");
        let query4 = "SELECT sequences FROM proof_event WHERE app_instance_id = ? LIMIT 5";
        let stmt4 = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::MySql,
            query4,
            vec![Value::String(Some(Box::new(app_instance_id.to_string())))],
        );

        if let Ok(results) = conn.query_all(stmt4).await {
            for row in results {
                if let Ok(seq) = row.try_get_by_index::<String>(0) {
                    println!("  Raw sequences value: {:?}", seq);
                    // Try to parse as JSON array
                    if let Ok(parsed) = serde_json::from_str::<Vec<String>>(&seq) {
                        println!("    Parsed as JSON array: {:?}", parsed);
                    } else {
                        println!("    Not a JSON array, raw string: {}", seq);
                    }
                }
            }
        }
    }

    // Now test the actual RPC function
    println!("\n=== Testing RPC function get_events_by_app_instance_sequence ===");

    let request = GetEventsByAppInstanceSequenceRequest {
        app_instance_id: app_instance_id.to_string(),
        sequence,
        limit: Some(100),
        offset: None,
    };

    match get_events_by_app_instance_sequence_impl(&database, request).await {
        Ok(response) => {
            println!("\nRPC Response:");
            println!("  success: {}", response.success);
            println!("  message: {}", response.message);
            println!("  total_count: {}", response.total_count);
            println!("  returned_count: {}", response.returned_count);
            println!("  events count: {}", response.events.len());

            if !response.events.is_empty() {
                println!("\nEvents found:");
                for (i, event) in response.events.iter().enumerate() {
                    println!("  Event {}: {:?}", i + 1, event);
                }
            } else {
                println!("\nNo events found!");
            }
        }
        Err(e) => {
            println!("Error calling RPC function: {:?}", e);
        }
    }

    // Let's also test with sequence as string "4" in case there's a type mismatch
    println!("\n=== Additional debug: Check job_sequences and proof_event_sequences tables directly ===");
    {
        use sea_orm::{ConnectionTrait, Statement, Value};
        let conn = rpc::database::get_connection(&database);

        // Check job_sequences table
        println!("\n5. Checking job_sequences table:");
        let query = "SELECT * FROM job_sequences WHERE app_instance_id = ? AND job_sequence = ? LIMIT 10";
        let stmt = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::MySql,
            query,
            vec![
                Value::String(Some(Box::new(app_instance_id.to_string()))),
                Value::BigInt(Some(sequence as i64)),
            ],
        );

        if let Ok(results) = conn.query_all(stmt).await {
            println!("Found {} records in job_sequences", results.len());
            // Print column names if we have results
            if !results.is_empty() {
                // Try to get all columns from the first row
                if let Some(row) = results.first() {
                    for i in 0..10 {  // Try first 10 columns
                        if let Ok(val) = row.try_get_by_index::<String>(i) {
                            println!("  Column {}: {}", i, val);
                        } else if let Ok(val) = row.try_get_by_index::<i64>(i) {
                            println!("  Column {}: {}", i, val);
                        } else {
                            break;
                        }
                    }
                }
            }
        }

        // Check proof_event_sequences table
        println!("\n6. Checking proof_event_sequences table:");
        let query2 = "SELECT * FROM proof_event_sequences WHERE app_instance_id = ? AND sequence = ? LIMIT 10";
        let stmt2 = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::MySql,
            query2,
            vec![
                Value::String(Some(Box::new(app_instance_id.to_string()))),
                Value::BigInt(Some(sequence as i64)),
            ],
        );

        if let Ok(results) = conn.query_all(stmt2).await {
            println!("Found {} records in proof_event_sequences", results.len());
            // Print column names if we have results
            if !results.is_empty() {
                // Try to get all columns from the first row
                if let Some(row) = results.first() {
                    for i in 0..10 {  // Try first 10 columns
                        if let Ok(val) = row.try_get_by_index::<String>(i) {
                            println!("  Column {}: {}", i, val);
                        } else if let Ok(val) = row.try_get_by_index::<i64>(i) {
                            println!("  Column {}: {}", i, val);
                        } else {
                            break;
                        }
                    }
                }
            }
        }
    }

    Ok(())
}