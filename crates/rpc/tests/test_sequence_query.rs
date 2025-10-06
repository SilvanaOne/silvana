use anyhow::Result;
use dotenvy::dotenv;
use std::env;
use std::sync::Arc;
use proto::silvana_rpc_service_server::SilvanaRpcService;
use proto::{GetEventsByAppInstanceSequenceRequest, Event};
use rpc::rpc::SilvanaRpcServiceImpl;
use rpc::database::EventDatabase;
use rpc::adapters::RpcEventDatabaseAdapter;
use buffer::EventBuffer;
use tidb::TidbBackend;
use tonic::Request;

#[tokio::test]
async fn test_get_events_by_sequence() -> Result<()> {
    dotenv().ok();

    println!("\n=== Testing get_events_by_app_instance_sequence ===");

    // Initialize the database connection
    let database_url = env::var("DATABASE_URL")?;
    println!("Connecting to database: {}", database_url);
    let event_database = Arc::new(EventDatabase::new(&database_url).await?);
    println!("✅ Connected to database");

    // Create the adapter for TiDB backend
    let adapter = Arc::new(RpcEventDatabaseAdapter::new(event_database.clone()));
    let tidb_backend = Arc::new(TidbBackend::<Event>::new(adapter));

    // Create the event buffer
    let event_buffer = EventBuffer::<Event>::new(tidb_backend);

    // Create the RPC service
    let service = SilvanaRpcServiceImpl::new(
        event_buffer,
        event_database,
    );

    // Test parameters from production
    let app_instance_id = "0xfcc614ba9a4af99454bc7ec67e6a560e52d99076c75778d65abf5333a44ba186";
    let sequence = 1u64;

    println!("\nCalling get_events_by_app_instance_sequence:");
    println!("  - app_instance_id: {}", app_instance_id);
    println!("  - sequence: {}", sequence);

    // Create the request
    let request = Request::new(GetEventsByAppInstanceSequenceRequest {
        app_instance_id: app_instance_id.to_string(),
        sequence,
        limit: None,
        offset: None,
    });

    // Call the actual RPC function
    println!("\n--- Making RPC call ---");
    match service.get_events_by_app_instance_sequence(request).await {
        Ok(response) => {
            let resp = response.into_inner();
            println!("✅ RPC call succeeded!");
            println!("  - Found {} events", resp.events.len());

            // Print events for debugging
            for (i, event) in resp.events.iter().enumerate() {
                println!("\n  Event #{}:", i + 1);

                // Print the event details
                if let Some(event_data) = &event.event {
                    match event_data {
                        proto::event::Event::JobCreated(job) => {
                            println!("    Type: JobCreated");
                            println!("    - job_id: {}", job.job_id);
                            println!("    - app_instance_id: {}", job.app_instance_id);
                            println!("    - app_method: {}", job.app_method);
                            println!("    - job_sequence: {}", job.job_sequence);
                            println!("    - sequences: {:?}", job.sequences);
                            println!("    - merged_sequences_1: {:?}", job.merged_sequences_1);
                            println!("    - merged_sequences_2: {:?}", job.merged_sequences_2);
                        }
                        proto::event::Event::ProofEvent(proof) => {
                            println!("    Type: ProofEvent");
                            println!("    - job_id: {}", proof.job_id);
                            println!("    - app_instance_id: {}", proof.app_instance_id);
                            println!("    - block_number: {}", proof.block_number);
                            println!("    - sequences: {:?}", proof.sequences);
                            println!("    - proof_event_type: {:?}", proof.proof_event_type());
                        }
                        _ => {
                            println!("    Type: Other - {:?}", std::any::type_name_of_val(&event_data));
                        }
                    }
                }
            }
        }
        Err(status) => {
            println!("❌ RPC call failed!");
            println!("   Status code: {}", status.code());
            println!("   Status message: {}", status.message());

            // Check if it's the JSON/BLOB error
            if status.message().contains("mismatched types") {
                println!("\n🚨 JSON/BLOB TYPE MISMATCH ERROR DETECTED!");
                println!("   This is the error we're debugging.");
                println!("   The entity expects Vec<u8> (BLOB) but database has JSON column.");
                println!("\n   This error occurs in production at rpc-devnet.silvana.dev");
                println!("   but the local entities should have JsonBinary annotations.");
            }
        }
    }

    Ok(())
}

#[tokio::test]
async fn test_direct_jobs_query() -> Result<()> {
    use sea_orm::{Database, QuerySelect};
    use tidb::entity::jobs;
    use sea_orm::prelude::*;

    dotenv().ok();

    println!("\n=== Testing direct jobs table query ===");

    let database_url = env::var("DATABASE_URL")?;
    println!("Connecting to database: {}", database_url);
    let db = Database::connect(&database_url).await?;
    println!("✅ Connected to database");

    println!("\n--- Attempting to query all jobs (limit 5) ---");

    // Try to query all jobs to see if the JSON column is the issue
    let jobs_result = jobs::Entity::find()
        .limit(5)
        .all(&db)
        .await;

    match jobs_result {
        Ok(jobs_list) => {
            println!("✅ Successfully queried {} jobs", jobs_list.len());
            for (i, job) in jobs_list.iter().enumerate() {
                println!("\n  Job #{}:", i + 1);
                println!("    - job_id: {}", job.job_id);
                println!("    - sequences: {:?}", job.sequences);
                println!("    - app_instance_id: {}", job.app_instance_id);
            }
        }
        Err(e) => {
            println!("❌ Failed to query jobs table!");
            println!("   Error: {:?}", e);
            println!("   Error type: {}", std::any::type_name_of_val(&e));
            println!("   Full error string: {}", e);

            if e.to_string().contains("mismatched types") {
                println!("\n🚨 JSON/BLOB TYPE MISMATCH ERROR DETECTED!");
                println!("   This confirms the entity definition doesn't match the database schema.");
                println!("   The entity expects Vec<u8> (BLOB) but database has JSON column.");

                // Additional debug info
                println!("\n   Debug info:");
                println!("   - Check if crates/tidb/src/entity/jobs.rs has JsonBinary annotation");
                println!("   - Verify the deployed binary was built with updated entities");
                println!("   - Ensure no caching of old entity definitions");
            }
        }
    }

    Ok(())
}