use rpc::database::EventDatabase;
use std::env;

#[tokio::test]
async fn test_debug_sequence_lookup() -> Result<(), Box<dyn std::error::Error>> {
    // Load environment variables from .env file manually
    dotenvy::dotenv().ok();

    // Get database URL from environment
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set in .env file");

    println!("Connecting to database...");

    // Create database connection
    let database = EventDatabase::new(&database_url).await?;

    // Test parameters from the user's example
    let app_instance_id = "0xe4094d71ce851dda51efd6a459a1d096c414fd899ac79e655a1349f39288d45e";
    let app_instance_id_no_prefix =
        "e4094d71ce851dda51efd6a459a1d096c414fd899ac79e655a1349f39288d45e";
    let sequence = 4i64;

    println!(
        "\n=== Debugging sequence lookup for app_instance={}, sequence={} ===",
        app_instance_id, sequence
    );

    {
        use sea_orm::{ConnectionTrait, Statement, Value};
        let conn = rpc::database::get_connection(&database);

        // Check proof_event_sequences table structure
        println!("\n1. Checking proof_event_sequences table structure:");
        let query = "DESCRIBE proof_event_sequences";
        let stmt = Statement::from_sql_and_values(sea_orm::DatabaseBackend::MySql, query, vec![]);

        if let Ok(results) = conn.query_all(stmt).await {
            println!("Table structure:");
            for row in results {
                if let (Ok(field), Ok(typ)) = (
                    row.try_get_by_index::<String>(0),
                    row.try_get_by_index::<String>(1),
                ) {
                    println!("  Field: {}, Type: {}", field, typ);
                }
            }
        }

        // Check if proof_event_sequences table has any data
        println!("\n2. Checking proof_event_sequences table data:");
        let query2 =
            "SELECT COUNT(*) as count FROM proof_event_sequences WHERE app_instance_id = ?";
        let stmt2 = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::MySql,
            query2,
            vec![Value::String(Some(Box::new(app_instance_id.to_string())))],
        );

        if let Ok(result) = conn.query_one(stmt2).await {
            if let Some(row) = result {
                let count: i64 = row.try_get_by_index(0).unwrap_or(0);
                println!(
                    "Total proof_event_sequences records for this app: {}",
                    count
                );
            }
        }

        // Check what sequences exist in proof_event_sequences
        println!("\n3. Listing all sequences in proof_event_sequences for this app:");
        let query3 = "SELECT DISTINCT sequence FROM proof_event_sequences WHERE app_instance_id = ? ORDER BY sequence";
        let stmt3 = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::MySql,
            query3,
            vec![Value::String(Some(Box::new(app_instance_id.to_string())))],
        );

        if let Ok(results) = conn.query_all(stmt3).await {
            println!("Found {} distinct sequences", results.len());
            for row in results {
                if let Ok(seq) = row.try_get_by_index::<i64>(0) {
                    println!("  Sequence: {}", seq);
                }
            }
        }

        // Check specific sequence 4
        println!("\n4. Looking for sequence 4 specifically:");
        let query4 =
            "SELECT * FROM proof_event_sequences WHERE app_instance_id = ? AND sequence = ?";
        let stmt4 = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::MySql,
            query4,
            vec![
                Value::String(Some(Box::new(app_instance_id.to_string()))),
                Value::BigInt(Some(sequence)),
            ],
        );

        if let Ok(results) = conn.query_all(stmt4).await {
            println!("Found {} records for sequence 4", results.len());
            if !results.is_empty() {
                // Print first record details
                if let Some(row) = results.first() {
                    if let Ok(proof_id) = row.try_get_by_index::<String>(2) {
                        println!("  proof_event_id: {}", proof_id);
                    }
                }
            }
        }

        // Check job_sequences table structure
        println!("\n5. Checking job_sequences table structure:");
        let query5 = "DESCRIBE job_sequences";
        let stmt5 = Statement::from_sql_and_values(sea_orm::DatabaseBackend::MySql, query5, vec![]);

        if let Ok(results) = conn.query_all(stmt5).await {
            println!("Table structure:");
            for row in results {
                if let (Ok(field), Ok(typ)) = (
                    row.try_get_by_index::<String>(0),
                    row.try_get_by_index::<String>(1),
                ) {
                    println!("  Field: {}, Type: {}", field, typ);
                }
            }
        }

        // Check what sequences exist in job_sequences
        println!("\n6. Listing all sequences in job_sequences for this app:");
        let query6 = "SELECT DISTINCT job_sequence FROM job_sequences WHERE app_instance_id = ? ORDER BY job_sequence";
        let stmt6 = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::MySql,
            query6,
            vec![Value::String(Some(Box::new(app_instance_id.to_string())))],
        );

        if let Ok(results) = conn.query_all(stmt6).await {
            println!("Found {} distinct job_sequences", results.len());
            for row in results {
                if let Ok(seq) = row.try_get_by_index::<i64>(0) {
                    println!("  Job Sequence: {}", seq);
                }
            }
        }

        // Check if column name is 'sequence' or 'job_sequence' in job_sequences table
        println!("\n7. Checking job_sequences for sequence 4:");
        let query7 = "SELECT * FROM job_sequences WHERE app_instance_id = ? AND job_sequence = ?";
        let stmt7 = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::MySql,
            query7,
            vec![
                Value::String(Some(Box::new(app_instance_id.to_string()))),
                Value::BigInt(Some(sequence)),
            ],
        );

        if let Ok(results) = conn.query_all(stmt7).await {
            println!("Found {} records for job_sequence 4", results.len());
            if !results.is_empty() {
                // Print first record details
                if let Some(row) = results.first() {
                    if let Ok(job_id) = row.try_get_by_index::<String>(2) {
                        println!("  job_id: {}", job_id);
                    }
                }
            }
        }

        // Check proof_event table for sequences containing "4"
        println!("\n8. Checking proof_event table for sequences containing '4':");
        let query8 = "SELECT id, sequences, event_timestamp FROM proof_event WHERE app_instance_id = ? AND (sequences LIKE '%4%' OR sequences LIKE '%[4]%' OR sequences = '4') ORDER BY event_timestamp DESC LIMIT 10";
        let stmt8 = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::MySql,
            query8,
            vec![Value::String(Some(Box::new(app_instance_id.to_string())))],
        );

        if let Ok(results) = conn.query_all(stmt8).await {
            println!("Found {} proof_event records", results.len());
            for row in results {
                if let (Ok(id), Ok(seq), Ok(ts)) = (
                    row.try_get_by_index::<String>(0),
                    row.try_get_by_index::<String>(1),
                    row.try_get_by_index::<i64>(2),
                ) {
                    println!("  id: {}, sequences: {}, timestamp: {}", id, seq, ts);

                    // Try to parse sequences as JSON array
                    if let Ok(parsed) = serde_json::from_str::<Vec<String>>(&seq) {
                        println!("    Parsed as: {:?}", parsed);
                        if parsed.contains(&"4".to_string()) {
                            println!("    -> Contains sequence 4!");
                        }
                    }
                }
            }
        }

        // Check if there are ANY proof_events for this app
        println!("\n9. Checking ALL proof_event records for this app:");
        let query9 = "SELECT id, sequences, event_timestamp FROM proof_event WHERE app_instance_id = ? ORDER BY event_timestamp DESC LIMIT 10";
        let stmt9 = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::MySql,
            query9,
            vec![Value::String(Some(Box::new(app_instance_id.to_string())))],
        );

        if let Ok(results) = conn.query_all(stmt9).await {
            println!(
                "Found {} total proof_event records for this app",
                results.len()
            );
            for row in results {
                if let (Ok(id), Ok(seq), Ok(ts)) = (
                    row.try_get_by_index::<String>(0),
                    row.try_get_by_index::<String>(1),
                    row.try_get_by_index::<i64>(2),
                ) {
                    println!("  id: {}, sequences: {}, timestamp: {}", id, seq, ts);
                }
            }
        }

        // Check without 0x prefix
        println!("\n10. Checking proof_event records WITHOUT 0x prefix:");
        let query10 = "SELECT id, sequences, app_instance_id, event_timestamp FROM proof_event WHERE app_instance_id = ? ORDER BY event_timestamp DESC LIMIT 10";
        let stmt10 = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::MySql,
            query10,
            vec![Value::String(Some(Box::new(
                app_instance_id_no_prefix.to_string(),
            )))],
        );

        if let Ok(results) = conn.query_all(stmt10).await {
            println!(
                "Found {} total proof_event records for app without 0x",
                results.len()
            );
            for row in results {
                if let (Ok(id), Ok(seq), Ok(app_id), Ok(ts)) = (
                    row.try_get_by_index::<String>(0),
                    row.try_get_by_index::<String>(1),
                    row.try_get_by_index::<String>(2),
                    row.try_get_by_index::<i64>(3),
                ) {
                    println!(
                        "  id: {}, sequences: {}, app: {}, timestamp: {}",
                        id, seq, app_id, ts
                    );
                }
            }
        }

        // Let's check if there are ANY proof events at all
        println!("\n11. Checking if there are ANY proof_event records in the database:");
        let query11 = "SELECT COUNT(*) as count FROM proof_event";
        let stmt11 =
            Statement::from_sql_and_values(sea_orm::DatabaseBackend::MySql, query11, vec![]);

        if let Ok(result) = conn.query_one(stmt11).await {
            if let Some(row) = result {
                let count: i64 = row.try_get_by_index(0).unwrap_or(0);
                println!("Total proof_event records in database: {}", count);
            }
        }

        // Let's see ALL proof_event records with all fields
        println!("\n12. Showing ALL proof_event records in detail:");
        let query12 = "SELECT id, coordinator_id, session_id, app_instance_id, job_id, data_availability, block_number, proof_event_type, sequences, merged_sequences_1, merged_sequences_2, event_timestamp, block_proof FROM proof_event";
        let stmt12 =
            Statement::from_sql_and_values(sea_orm::DatabaseBackend::MySql, query12, vec![]);

        if let Ok(results) = conn.query_all(stmt12).await {
            println!("Found {} proof_event records total:", results.len());
            for (i, row) in results.iter().enumerate() {
                println!("\n  Record #{}:", i + 1);
                if let Ok(id) = row.try_get_by_index::<String>(0) {
                    println!("    id: {}", id);
                }
                if let Ok(coord_id) = row.try_get_by_index::<String>(1) {
                    println!("    coordinator_id: {}", coord_id);
                }
                if let Ok(session_id) = row.try_get_by_index::<String>(2) {
                    println!("    session_id: {}", session_id);
                }
                if let Ok(app_id) = row.try_get_by_index::<String>(3) {
                    println!("    app_instance_id: {}", app_id);
                }
                if let Ok(job_id) = row.try_get_by_index::<String>(4) {
                    println!("    job_id: {}", job_id);
                }
                if let Ok(data_avail) = row.try_get_by_index::<String>(5) {
                    println!("    data_availability: {}", data_avail);
                }
                if let Ok(block_num) = row.try_get_by_index::<i64>(6) {
                    println!("    block_number: {}", block_num);
                }
                if let Ok(proof_type) = row.try_get_by_index::<i32>(7) {
                    println!("    proof_event_type: {}", proof_type);
                }
                if let Ok(sequences) = row.try_get_by_index::<String>(8) {
                    println!("    sequences: {}", sequences);
                    // Try to parse as JSON
                    if let Ok(parsed) = serde_json::from_str::<Vec<String>>(&sequences) {
                        println!("      -> Parsed as: {:?}", parsed);
                    }
                }
                if let Ok(merged1) = row.try_get_by_index::<String>(9) {
                    println!("    merged_sequences_1: {}", merged1);
                }
                if let Ok(merged2) = row.try_get_by_index::<String>(10) {
                    println!("    merged_sequences_2: {}", merged2);
                }
                if let Ok(timestamp) = row.try_get_by_index::<i64>(11) {
                    println!("    event_timestamp: {}", timestamp);
                }
                if let Ok(block_proof) = row.try_get_by_index::<bool>(12) {
                    println!("    block_proof: {}", block_proof);
                }
            }
        }
    }

    Ok(())
}
