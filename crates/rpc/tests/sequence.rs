use rpc::database::EventDatabase;
use rpc::proto::{GetEventsByAppInstanceSequenceRequest, GetEventsByAppInstanceSequenceResponse, Event};
use std::env;

// Direct implementation that mirrors the actual RPC function with 0x prefix handling
async fn get_events_by_app_instance_sequence_direct(
    database: &EventDatabase,
    request: GetEventsByAppInstanceSequenceRequest,
) -> Result<GetEventsByAppInstanceSequenceResponse, Box<dyn std::error::Error>> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    use tidb::entity::{job_sequences, proof_event_sequences};

    println!("\nDirect function called with:");
    println!("  app_instance_id: {}", request.app_instance_id);
    println!("  sequence: {}", request.sequence);

    // Normalize app_instance_id by removing 0x prefix if present (same as the fix in rpc.rs)
    let normalized_app_id = if request.app_instance_id.starts_with("0x") || request.app_instance_id.starts_with("0X") {
        request.app_instance_id[2..].to_string()
    } else {
        request.app_instance_id.clone()
    };

    println!("  normalized_app_id: {}", normalized_app_id);

    let db = rpc::database::get_connection(database);
    let mut all_events = Vec::new();

    // Query job_sequences table
    let job_sequence_results = job_sequences::Entity::find()
        .filter(job_sequences::Column::AppInstanceId.eq(&normalized_app_id))
        .filter(job_sequences::Column::Sequence.eq(request.sequence as i64))
        .all(db)
        .await?;

    println!("  Found {} job_sequences entries", job_sequence_results.len());

    // For each job_id found, we would fetch the job from jobs table
    for job_seq in &job_sequence_results {
        println!("    Found job_sequences entry with job_id: {}", job_seq.job_id);
        // In the real implementation, we would fetch the full job details here
        // For now, just create a placeholder event to show it's working
        let event = Event {
            event: Some(rpc::proto::event::Event::JobCreated(rpc::proto::JobCreatedEvent {
                coordinator_id: "test".to_string(),
                session_id: "test".to_string(),
                app_instance_id: normalized_app_id.clone(),
                app_method: "test".to_string(),
                job_sequence: request.sequence,
                sequences: vec![request.sequence],
                merged_sequences_1: vec![],
                merged_sequences_2: vec![],
                job_id: job_seq.job_id.clone(),
                event_timestamp: 0,
            })),
        };
        all_events.push(event);
    }

    // Query proof_event_sequences table
    let proof_sequence_results = proof_event_sequences::Entity::find()
        .filter(proof_event_sequences::Column::AppInstanceId.eq(&normalized_app_id))
        .filter(proof_event_sequences::Column::Sequence.eq(request.sequence as i64))
        .all(db)
        .await?;

    println!("  Found {} proof_event_sequences entries", proof_sequence_results.len());

    // For each proof_event_id found, we would fetch the proof event
    for proof_seq in &proof_sequence_results {
        println!("    Found proof_event_sequences entry with proof_event_id: {}", proof_seq.proof_event_id);
        // In the real implementation, we would fetch the full proof event details here
        // For now, just create a placeholder event to show it's working
        let event = Event {
            event: Some(rpc::proto::event::Event::ProofEvent(rpc::proto::ProofEvent {
                coordinator_id: "test".to_string(),
                session_id: "test".to_string(),
                app_instance_id: normalized_app_id.clone(),
                job_id: "test".to_string(),
                data_availability: "test".to_string(),
                block_number: 0,
                block_proof: Some(false),
                proof_event_type: 1, // ProofSubmitted
                sequences: vec![request.sequence],
                merged_sequences_1: vec![],
                merged_sequences_2: vec![],
                event_timestamp: 0,
            })),
        };
        all_events.push(event);
    }

    let total_count = all_events.len() as u32;

    Ok(GetEventsByAppInstanceSequenceResponse {
        success: true,
        message: format!("Found {} events", total_count),
        events: all_events,
        total_count,
        returned_count: total_count,
    })
}

#[tokio::test]
async fn test_get_events_by_sequence() -> Result<(), Box<dyn std::error::Error>> {

    // Load environment variables from .env file manually
    dotenvy::dotenv().ok();

    // Get database URL from environment
    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set in .env file");

    println!("Connecting to database...");

    // Create database connection
    let database = EventDatabase::new(&database_url).await?;

    // Test parameters from the user's example
    let app_instance_id = "0x02b089c58854c9189c5cf8ae5ec320f94312ebe2a94a4a58b05d6c0e120f8ba2";
    let sequence = 1u64;

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
    println!("\n=== Testing direct database function with 0x prefix handling ===");

    let request = GetEventsByAppInstanceSequenceRequest {
        app_instance_id: app_instance_id.to_string(),
        sequence,
        limit: Some(100),
        offset: None,
    };

    match get_events_by_app_instance_sequence_direct(&database, request).await {
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

    // First let's check if there are ANY records in proof_event_sequences table
    println!("\nChecking proof_event_sequences table for ANY records:");
    {
        use sea_orm::{ConnectionTrait, Statement};
        let conn = rpc::database::get_connection(&database);

        let query = "SELECT COUNT(*) as count FROM proof_event_sequences";
        let stmt = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::MySql,
            query,
            vec![],
        );

        if let Ok(results) = conn.query_all(stmt).await {
            if let Some(row) = results.first() {
                if let Ok(count) = row.try_get_by_index::<i64>(0) {
                    println!("Total records in proof_event_sequences: {}", count);
                }
            }
        }

        // First, let's see what app_instance_ids are actually in the table
        let query_all = "SELECT DISTINCT app_instance_id FROM proof_event_sequences LIMIT 10";
        let stmt_all = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::MySql,
            query_all,
            vec![],
        );

        if let Ok(results) = conn.query_all(stmt_all).await {
            println!("\nDistinct app_instance_ids in proof_event_sequences:");
            for row in &results {
                if let Ok(app_id) = row.try_get_by_index::<String>(0) {
                    println!("  - {}", app_id);
                    // Check if this matches our search (case-insensitive)
                    if app_id.to_lowercase() == app_instance_id.to_lowercase() {
                        println!("    ✓ MATCHES our search (case-insensitive)");
                    }
                }
            }
        }

        // Check specific records
        let query2 = "SELECT * FROM proof_event_sequences WHERE app_instance_id = ? LIMIT 10";
        let stmt2 = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::MySql,
            query2,
            vec![sea_orm::Value::String(Some(Box::new(app_instance_id.to_string())))],
        );

        if let Ok(results) = conn.query_all(stmt2).await {
            println!("Found {} records for app_instance_id {}", results.len(), app_instance_id);
            for row in &results {
                println!("\nRow data:");
                if let Ok(id) = row.try_get_by_index::<i64>(0) {
                    println!("  id: {}", id);
                }
                if let Ok(app_id) = row.try_get_by_index::<String>(1) {
                    println!("  app_instance_id: {}", app_id);
                }
                if let Ok(seq) = row.try_get_by_index::<i64>(2) {
                    println!("  sequence: {}", seq);
                }
                if let Ok(proof_id) = row.try_get_by_index::<i64>(3) {
                    println!("  proof_event_id: {}", proof_id);
                }
                if let Ok(seq_type) = row.try_get_by_index::<String>(4) {
                    println!("  sequence_type: {}", seq_type);
                }
            }
        }
    }
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