use crate::error::{CoordinatorError, Result};
use crate::pending::{PendingJob, JobStatus};
use crate::state::SharedState;
use std::collections::HashSet;
use sui_rpc::Client;
use sui_rpc::proto::sui::rpc::v2beta2::{GetObjectRequest, ListDynamicFieldsRequest};
use tracing::{debug, info, warn, error};
use base64::{engine::general_purpose, Engine as _};

/// Extract PendingJob from JSON representation
pub fn extract_job_from_json(json_value: &prost_types::Value) -> Result<PendingJob> {
    if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
        //debug!("Job JSON fields: {:?}", struct_value.fields.keys().collect::<Vec<_>>());
        let mut job = PendingJob {
            job_sequence: 0,
            description: None,
            developer: String::new(),
            agent: String::new(),
            agent_method: String::new(),
            app: String::new(),
            app_instance: String::new(),
            app_instance_method: String::new(),
            sequences: None,
            data: Vec::new(),
            status: JobStatus::Pending,
            attempts: 0,
            created_at: 0,
            updated_at: 0,
        };
        
        // Extract all fields from the job struct
        if let Some(field) = struct_value.fields.get("job_sequence") {
            if let Some(prost_types::value::Kind::StringValue(id_str)) = &field.kind {
                job.job_sequence = id_str.parse().unwrap_or(0);
            }
        }
        
        if let Some(field) = struct_value.fields.get("description") {
            match &field.kind {
                Some(prost_types::value::Kind::StringValue(desc)) => {
                    job.description = Some(desc.clone());
                }
                Some(prost_types::value::Kind::NullValue(_)) => {
                    job.description = None;
                }
                _ => {}
            }
        }
        
        if let Some(field) = struct_value.fields.get("developer") {
            if let Some(prost_types::value::Kind::StringValue(val)) = &field.kind {
                job.developer = val.clone();
            }
        }
        
        if let Some(field) = struct_value.fields.get("agent") {
            if let Some(prost_types::value::Kind::StringValue(val)) = &field.kind {
                job.agent = val.clone();
            }
        }
        
        if let Some(field) = struct_value.fields.get("agent_method") {
            if let Some(prost_types::value::Kind::StringValue(val)) = &field.kind {
                job.agent_method = val.clone();
            }
        }
        
        if let Some(field) = struct_value.fields.get("app") {
            if let Some(prost_types::value::Kind::StringValue(val)) = &field.kind {
                job.app = val.clone();
            }
        }
        
        if let Some(field) = struct_value.fields.get("app_instance") {
            if let Some(prost_types::value::Kind::StringValue(val)) = &field.kind {
                job.app_instance = val.clone();
            }
        }
        
        if let Some(field) = struct_value.fields.get("app_instance_method") {
            if let Some(prost_types::value::Kind::StringValue(val)) = &field.kind {
                job.app_instance_method = val.clone();
            }
        }
        
        if let Some(field) = struct_value.fields.get("sequences") {
            match &field.kind {
                Some(prost_types::value::Kind::ListValue(list)) => {
                    let mut sequences = Vec::new();
                    for val in &list.values {
                        if let Some(prost_types::value::Kind::StringValue(seq_str)) = &val.kind {
                            if let Ok(seq) = seq_str.parse::<u64>() {
                                sequences.push(seq);
                            }
                        }
                    }
                    if !sequences.is_empty() {
                        job.sequences = Some(sequences);
                    }
                }
                Some(prost_types::value::Kind::NullValue(_)) => {
                    job.sequences = None;
                }
                _ => {}
            }
        }
        
        if let Some(field) = struct_value.fields.get("data") {
            //debug!("Found data field: {:?}", field.kind);
            if let Some(prost_types::value::Kind::StringValue(data_str)) = &field.kind {
                //debug!("Data string: {}", data_str);
                // Data might be base64 encoded or hex encoded
                if let Ok(data) = hex::decode(data_str.trim_start_matches("0x")) {
                    //debug!("Decoded hex data length: {}", data.len());
                    job.data = data;
                } else if let Ok(data) = general_purpose::STANDARD.decode(data_str) {
                    //debug!("Decoded base64 data length: {}", data.len());
                    job.data = data;
                } else {
                    //debug!("Failed to decode data as hex or base64: {}", data_str);
                }
            }
        } else {
            debug!("No data field found in job JSON");
        }
        
        if let Some(field) = struct_value.fields.get("status") {
            if let Some(prost_types::value::Kind::StructValue(status_struct)) = &field.kind {
                // Parse JobStatus enum
                if let Some(variant_field) = status_struct.fields.iter().next() {
                    match variant_field.0.as_str() {
                        "Pending" => job.status = JobStatus::Pending,
                        "Running" => job.status = JobStatus::Running,
                        "Failed" => {
                            if let Some(prost_types::value::Kind::StringValue(msg)) = &variant_field.1.kind {
                                job.status = JobStatus::Failed(msg.clone());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        
        if let Some(field) = struct_value.fields.get("attempts") {
            if let Some(prost_types::value::Kind::NumberValue(attempts)) = &field.kind {
                job.attempts = *attempts as u8;
            } else if let Some(prost_types::value::Kind::StringValue(attempts_str)) = &field.kind {
                job.attempts = attempts_str.parse().unwrap_or(0);
            }
        }
        
        if let Some(field) = struct_value.fields.get("created_at") {
            if let Some(prost_types::value::Kind::StringValue(ts_str)) = &field.kind {
                job.created_at = ts_str.parse().unwrap_or(0);
            }
        }
        
        if let Some(field) = struct_value.fields.get("updated_at") {
            if let Some(prost_types::value::Kind::StringValue(ts_str)) = &field.kind {
                job.updated_at = ts_str.parse().unwrap_or(0);
            }
        }
        
        return Ok(job);
    }
    
    Err(CoordinatorError::ConfigError(
        "Failed to extract job from JSON".to_string()
    ))
}

/// Fetch pending jobs from a specific app_instance
pub async fn fetch_pending_jobs_from_app_instance(
    client: &mut Client,
    app_instance_id: &str,
    _state: &SharedState,
    only_check: bool,
) -> Result<Option<PendingJob>> {
    // Ensure the app_instance_id has 0x prefix
    let formatted_id = if app_instance_id.starts_with("0x") {
        app_instance_id.to_string()
    } else {
        format!("0x{}", app_instance_id)
    };
    
    debug!("Fetching app_instance object: {}", formatted_id);
    
    // Fetch the AppInstance object
    let app_instance_request = GetObjectRequest {
        object_id: Some(formatted_id.clone()),
        version: None,
        read_mask: Some(prost_types::FieldMask {
            paths: vec![
                "object_id".to_string(),
                "json".to_string(),
            ],
        }),
    };

    let app_instance_response = client
        .ledger_client()
        .get_object(app_instance_request)
        .await
        .map_err(|e| CoordinatorError::RpcConnectionError(
            format!("Failed to fetch app_instance {}: {}", formatted_id, e)
        ))?;

    let response = app_instance_response.into_inner();
    
    if let Some(proto_object) = response.object {
        if let Some(json_value) = &proto_object.json {
            if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
                // Look for the jobs field in the AppInstance
                if let Some(jobs_field) = struct_value.fields.get("jobs") {
                    if let Some(prost_types::value::Kind::StructValue(jobs_struct)) = &jobs_field.kind {
                        // For check-only mode, just look at pending_jobs_count
                        if only_check {
                            if let Some(count_field) = jobs_struct.fields.get("pending_jobs_count") {
                                if let Some(prost_types::value::Kind::StringValue(count_str)) = &count_field.kind {
                                    if let Ok(count) = count_str.parse::<u64>() {
                                        if count > 0 {
                                            debug!("Found {} pending jobs in app_instance (check-only mode)", count);
                                            return Ok(None);
                                        } else {
                                            debug!("No pending jobs in app_instance {} (count=0)", app_instance_id);
                                            //state.remove_app_instance(app_instance_id).await;
                                            return Ok(None);
                                        }
                                    }
                                }
                            }
                        }
                        
                        // Get the pending_jobs VecSet
                        if let Some(pending_jobs_field) = jobs_struct.fields.get("pending_jobs") {
                            if let Some(prost_types::value::Kind::StructValue(pending_jobs_struct)) = &pending_jobs_field.kind {
                                if let Some(contents_field) = pending_jobs_struct.fields.get("contents") {
                                    if let Some(prost_types::value::Kind::ListValue(list)) = &contents_field.kind {
                                        let mut pending_job_sequences = HashSet::new();
                                        for job_sequence_value in &list.values {
                                            if let Some(prost_types::value::Kind::StringValue(job_sequence_str)) = &job_sequence_value.kind {
                                                if let Ok(job_sequence) = job_sequence_str.parse::<u64>() {
                                                    pending_job_sequences.insert(job_sequence);
                                                }
                                            }
                                        }
                                        
                                        if pending_job_sequences.is_empty() {
                                            debug!("No pending jobs in app_instance {}", app_instance_id);
                                            //state.remove_app_instance(app_instance_id).await;
                                            return Ok(None);
                                        }
                                        
                                        // Get the smallest job_sequence
                                        let mut job_sequences: Vec<u64> = pending_job_sequences.iter().cloned().collect();
                                        job_sequences.sort();
                                        let target_job_sequence = job_sequences[0];
                                        
                                        debug!("Found {} pending jobs, will fetch job_sequence {}", job_sequences.len(), target_job_sequence);
                                        
                                        // Now fetch the specific job from the jobs ObjectTable
                                        if let Some(jobs_table_field) = jobs_struct.fields.get("jobs") {
                                            if let Some(prost_types::value::Kind::StructValue(jobs_table_struct)) = &jobs_table_field.kind {
                                                if let Some(table_id_field) = jobs_table_struct.fields.get("id") {
                                                    if let Some(prost_types::value::Kind::StringValue(table_id)) = &table_id_field.kind {
                                                        // Fetch the specific job
                                                        if let Some(job) = fetch_job_by_id(client, table_id, target_job_sequence).await? {
                                                            return Ok(Some(job));
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    
    debug!("No jobs found in app_instance {}", app_instance_id);
    Ok(None)
}

/// Fetch a specific job by ID from the jobs ObjectTable
pub async fn fetch_job_by_id(
    client: &mut Client,
    jobs_table_id: &str,
    job_sequence: u64,
) -> Result<Option<PendingJob>> {
    debug!("Fetching job {} from jobs table {}", job_sequence, jobs_table_id);
    
    // List dynamic fields to find the specific job
    let list_request = ListDynamicFieldsRequest {
        parent: Some(jobs_table_id.to_string()),
        page_size: Some(100),
        page_token: None,
        read_mask: Some(prost_types::FieldMask {
            paths: vec![
                "field_id".to_string(),
                "name_type".to_string(),
                "name_value".to_string(),
            ],
        }),
    };
    
    let list_response = client
        .live_data_client()
        .list_dynamic_fields(list_request)
        .await
        .map_err(|e| CoordinatorError::RpcConnectionError(
            format!("Failed to list jobs in table: {}", e)
        ))?;
    
    let response = list_response.into_inner();
    
    // Find the specific job entry
    for field in &response.dynamic_fields {
        if let Some(name_value) = &field.name_value {
            // The name_value is BCS-encoded u64 (job_sequence)
            if let Ok(field_job_sequence) = bcs::from_bytes::<u64>(name_value) {
                if field_job_sequence == job_sequence {
                    if let Some(field_id) = &field.field_id {
                        // Fetch the job field wrapper
                        let job_field_request = GetObjectRequest {
                            object_id: Some(field_id.clone()),
                            version: None,
                            read_mask: Some(prost_types::FieldMask {
                                paths: vec![
                                    "object_id".to_string(),
                                    "json".to_string(),
                                ],
                            }),
                        };
                        
                        let job_field_response = client
                            .ledger_client()
                            .get_object(job_field_request)
                            .await
                            .map_err(|e| CoordinatorError::RpcConnectionError(
                                format!("Failed to fetch job field {}: {}", job_sequence, e)
                            ))?;
                        
                        if let Some(job_field_object) = job_field_response.into_inner().object {
                            // Extract the actual job object ID from the Field wrapper
                            if let Some(json_value) = &job_field_object.json {
                                if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
                                    if let Some(value_field) = struct_value.fields.get("value") {
                                        if let Some(prost_types::value::Kind::StringValue(job_object_id)) = &value_field.kind {
                                            // Fetch the actual job object
                                            let job_request = GetObjectRequest {
                                                object_id: Some(job_object_id.clone()),
                                                version: None,
                                                read_mask: Some(prost_types::FieldMask {
                                                    paths: vec![
                                                        "object_id".to_string(),
                                                        "json".to_string(),
                                                    ],
                                                }),
                                            };
                                            
                                            let job_response = client
                                                .ledger_client()
                                                .get_object(job_request)
                                                .await
                                                .map_err(|e| CoordinatorError::RpcConnectionError(
                                                    format!("Failed to fetch job {}: {}", job_sequence, e)
                                                ))?;
                                            
                                            if let Some(job_object) = job_response.into_inner().object {
                                                if let Some(job_json) = &job_object.json {
                                                    if let Ok(job) = extract_job_from_json(job_json) {
                                                        return Ok(Some(job));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    
    debug!("Job {} not found in jobs table", job_sequence);
    Ok(None)
}

/// Fetch pending job IDs for a specific (developer, agent, agent_method) from the embedded Jobs in AppInstance
pub async fn fetch_pending_job_sequences_from_app_instance(
    client: &mut Client,
    app_instance_id: &str,
    developer: &str,
    agent: &str,
    agent_method: &str,
) -> Result<Vec<u64>> {
    info!(
        "Fetching pending job IDs for {}/{}/{} from app_instance {}",
        developer, agent, agent_method, app_instance_id
    );
    
    // Ensure the app_instance_id has 0x prefix
    let formatted_id = if app_instance_id.starts_with("0x") {
        app_instance_id.to_string()
    } else {
        format!("0x{}", app_instance_id)
    };
    
    // Fetch the AppInstance object
    let request = GetObjectRequest {
        object_id: Some(formatted_id.clone()),
        version: None,
        read_mask: Some(prost_types::FieldMask {
            paths: vec!["json".to_string()],
        }),
    };

    let response = client
        .ledger_client()
        .get_object(request)
        .await
        .map_err(|e| CoordinatorError::RpcConnectionError(
            format!("Failed to fetch AppInstance {}: {}", formatted_id, e)
        ))?;

    if let Some(proto_object) = response.into_inner().object {
        if let Some(json_value) = &proto_object.json {
            if let Some(prost_types::value::Kind::StructValue(app_instance_struct)) = &json_value.kind {
                // Get the embedded jobs field
                if let Some(jobs_field) = app_instance_struct.fields.get("jobs") {
                    if let Some(prost_types::value::Kind::StructValue(jobs_struct)) = &jobs_field.kind {
                        // Look for the pending_jobs_indexes field
                        if let Some(indexes_field) = jobs_struct.fields.get("pending_jobs_indexes") {
                            if let Some(prost_types::value::Kind::StructValue(indexes_struct)) = &indexes_field.kind {
                                // Navigate through the nested VecMap structure
                                if let Some(contents_field) = indexes_struct.fields.get("contents") {
                                    if let Some(prost_types::value::Kind::ListValue(dev_list)) = &contents_field.kind {
                                        // Look for the developer entry
                                        for dev_entry in &dev_list.values {
                                            if let Some(prost_types::value::Kind::StructValue(dev_struct)) = &dev_entry.kind {
                                                if let Some(key_field) = dev_struct.fields.get("key") {
                                                    if let Some(prost_types::value::Kind::StringValue(dev_key)) = &key_field.kind {
                                                        if dev_key == developer {
                                                            // Found the developer, now look for agent
                                                            if let Some(value_field) = dev_struct.fields.get("value") {
                                                                if let Some(prost_types::value::Kind::StructValue(agent_map)) = &value_field.kind {
                                                                    if let Some(agent_contents) = agent_map.fields.get("contents") {
                                                                        if let Some(prost_types::value::Kind::ListValue(agent_list)) = &agent_contents.kind {
                                                                            // Look for the agent entry
                                                                            for agent_entry in &agent_list.values {
                                                                                if let Some(prost_types::value::Kind::StructValue(agent_struct)) = &agent_entry.kind {
                                                                                    if let Some(agent_key_field) = agent_struct.fields.get("key") {
                                                                                        if let Some(prost_types::value::Kind::StringValue(agent_key)) = &agent_key_field.kind {
                                                                                            if agent_key == agent {
                                                                                                // Found the agent, now look for agent_method
                                                                                                if let Some(agent_value_field) = agent_struct.fields.get("value") {
                                                                                                    if let Some(prost_types::value::Kind::StructValue(method_map)) = &agent_value_field.kind {
                                                                                                        if let Some(method_contents) = method_map.fields.get("contents") {
                                                                                                            if let Some(prost_types::value::Kind::ListValue(method_list)) = &method_contents.kind {
                                                                                                                // Look for the agent_method entry
                                                                                                                for method_entry in &method_list.values {
                                                                                                                    if let Some(prost_types::value::Kind::StructValue(method_struct)) = &method_entry.kind {
                                                                                                                        if let Some(method_key_field) = method_struct.fields.get("key") {
                                                                                                                            if let Some(prost_types::value::Kind::StringValue(method_key)) = &method_key_field.kind {
                                                                                                                                if method_key == agent_method {
                                                                                                                                    // Found the method, extract job IDs
                                                                                                                                    if let Some(method_value_field) = method_struct.fields.get("value") {
                                                                                                                                        if let Some(prost_types::value::Kind::StructValue(job_set)) = &method_value_field.kind {
                                                                                                                                            if let Some(job_contents) = job_set.fields.get("contents") {
                                                                                                                                                if let Some(prost_types::value::Kind::ListValue(job_list)) = &job_contents.kind {
                                                                                                                                                    let mut job_sequences = Vec::new();
                                                                                                                                                    for job_value in &job_list.values {
                                                                                                                                                        if let Some(prost_types::value::Kind::StringValue(job_sequence_str)) = &job_value.kind {
                                                                                                                                                            if let Ok(job_sequence) = job_sequence_str.parse::<u64>() {
                                                                                                                                                                job_sequences.push(job_sequence);
                                                                                                                                                            }
                                                                                                                                                        }
                                                                                                                                                    }
                                                                                                                                                    debug!("Found {} pending job sequences for {}/{}/{}", 
                                                                                                                                                        job_sequences.len(), developer, agent, agent_method);
                                                                                                                                                    return Ok(job_sequences);
                                                                                                                                                }
                                                                                                                                            }
                                                                                                                                        }
                                                                                                                                    }
                                                                                                                                }
                                                                                                                            }
                                                                                                                        }
                                                                                                                    }
                                                                                                                }
                                                                                                            }
                                                                                                        }
                                                                                                    }
                                                                                                }
                                                                                            }
                                                                                        }
                                                                                    }
                                                                                }
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    
    debug!("No pending jobs found for {}/{}/{}", developer, agent, agent_method);
    Ok(Vec::new())
}

/// Get the Jobs table ID from an AppInstance (Jobs is embedded, so we just need the table ID)
pub async fn get_jobs_info_from_app_instance(
    client: &mut Client,
    app_instance_id: &str,
) -> Result<Option<(String, String)>> {
    // Ensure the app_instance_id has 0x prefix
    let formatted_id = if app_instance_id.starts_with("0x") {
        app_instance_id.to_string()
    } else {
        format!("0x{}", app_instance_id)
    };
    
    debug!("Fetching Jobs info from app_instance: {}", formatted_id);
    
    // Fetch the AppInstance object
    let app_instance_request = GetObjectRequest {
        object_id: Some(formatted_id.clone()),
        version: None,
        read_mask: Some(prost_types::FieldMask {
            paths: vec![
                "object_id".to_string(),
                "json".to_string(),
            ],
        }),
    };

    let app_instance_response = client
        .ledger_client()
        .get_object(app_instance_request)
        .await
        .map_err(|e| CoordinatorError::RpcConnectionError(
            format!("Failed to fetch app_instance {}: {}", formatted_id, e)
        ))?;

    let response = app_instance_response.into_inner();
    
    if let Some(proto_object) = response.object {
        if let Some(json_value) = &proto_object.json {
            if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
                // Debug: print all fields in AppInstance
                debug!("AppInstance {} fields: {:?}", app_instance_id, struct_value.fields.keys().collect::<Vec<_>>());
                
                // Look for the jobs field in the AppInstance
                if let Some(jobs_field) = struct_value.fields.get("jobs") {
                    if let Some(prost_types::value::Kind::StructValue(jobs_struct)) = &jobs_field.kind {
                        // Debug: print all fields in Jobs struct
                        debug!("Jobs struct fields: {:?}", jobs_struct.fields.keys().collect::<Vec<_>>());
                        
                        // For embedded Jobs, we return the app_instance_id as the "Jobs object ID" 
                        // and extract the jobs table ID
                        if let Some(jobs_table_field) = jobs_struct.fields.get("jobs") {
                            if let Some(prost_types::value::Kind::StructValue(jobs_table_struct)) = &jobs_table_field.kind {
                                debug!("Jobs table struct fields: {:?}", jobs_table_struct.fields.keys().collect::<Vec<_>>());
                                if let Some(table_id_field) = jobs_table_struct.fields.get("id") {
                                    if let Some(prost_types::value::Kind::StringValue(jobs_table_id)) = &table_id_field.kind {
                                        //debug!("Found Jobs table ID: {}", jobs_table_id);
                                        // Return app_instance_id as the "Jobs object" since Jobs is embedded
                                        return Ok(Some((formatted_id, jobs_table_id.clone())));
                                    } else {
                                        warn!("Jobs table id field is not a string: {:?}", table_id_field.kind);
                                    }
                                } else {
                                    warn!("No 'id' field in jobs table struct");
                                }
                            } else {
                                warn!("Jobs table field is not a struct: {:?}", jobs_table_field.kind);
                            }
                        } else {
                            warn!("No 'jobs' field in Jobs struct");
                        }
                    } else {
                        warn!("Jobs field is not a struct: {:?}", jobs_field.kind);
                    }
                } else {
                    warn!("No 'jobs' field found in AppInstance. Available fields: {:?}", struct_value.fields.keys().collect::<Vec<_>>());
                }
            }
        }
    } else {
        warn!("No object returned for app_instance {}", app_instance_id);
    }
    
    Ok(None)
}

/// Try to fetch a pending job from any of the given app_instances using the index
pub async fn fetch_pending_job_from_instances(
    client: &mut Client,
    app_instances: &[String],
    developer: &str,
    agent: &str,
    agent_method: &str,
) -> Result<Option<PendingJob>> {
    // Collect all job IDs from all app_instances
    let mut all_jobs: Vec<(u64, String, String)> = Vec::new(); // (job_sequence, app_instance_id, jobs_table_id)
    
    for app_instance in app_instances {
        // Get Jobs table ID from the AppInstance
        let (_app_instance_id, jobs_table_id) = match get_jobs_info_from_app_instance(client, app_instance).await? {
            Some(info) => info,
            None => {
                warn!("Could not extract Jobs info from app_instance {}", app_instance);
                continue;
            }
        };
        
        // Use the index to get pending job IDs for this method
        let job_sequences = fetch_pending_job_sequences_from_app_instance(
            client,
            app_instance,
            developer,
            agent,
            agent_method,
        ).await?;
        
        for job_sequence in job_sequences {
            all_jobs.push((job_sequence, app_instance.clone(), jobs_table_id.clone()));
        }
    }
    
    if all_jobs.is_empty() {
        debug!("No pending jobs found in any app_instance for {}/{}/{}", developer, agent, agent_method);
        return Ok(None);
    }
    
    // Sort by job_sequence to get the lowest one
    all_jobs.sort_by_key(|&(job_sequence, _, _)| job_sequence);
    let (lowest_job_sequence, app_instance, jobs_table_id) = &all_jobs[0];
    
    info!(
        "Found {} total pending jobs across {} app_instances for {}/{}/{}, fetching job {} from {}",
        all_jobs.len(), app_instances.len(), developer, agent, agent_method, lowest_job_sequence, app_instance
    );
    
    // Fetch the specific job by ID
    fetch_job_by_id(client, jobs_table_id, *lowest_job_sequence).await
}

/// Fetch the pending job with the smallest job_sequence from multiple app_instances
pub async fn fetch_all_pending_jobs(
    client: &mut Client,
    app_instance_ids: &[String],
    state: &SharedState,
    only_check: bool,
) -> Result<Option<PendingJob>> {
    let mut all_pending_jobs = Vec::new();
    
    for app_instance_id in app_instance_ids {
        match fetch_pending_jobs_from_app_instance(client, app_instance_id, state, only_check).await {
            Ok(job_opt) => {
                if !only_check {
                    if let Some(job) = job_opt {
                        debug!("Found pending job with job_sequence {} in app_instance {}", job.job_sequence, app_instance_id);
                        all_pending_jobs.push(job);
                    }
                }
            }
            Err(e) => {
                error!("Failed to fetch pending job from app_instance {}: {}", app_instance_id, e);
            }
        }
    }
    
    // Sort all collected jobs and return the one with smallest job_sequence
    if all_pending_jobs.is_empty() {
        if !only_check {
            debug!("No pending jobs found across all app_instances");
        }
        Ok(None)
    } else {
        all_pending_jobs.sort_by_key(|job| job.job_sequence);
        let job = all_pending_jobs.into_iter().next().unwrap();
        info!("Returning pending job with smallest job_sequence: {}", job.job_sequence);
        Ok(Some(job))
    }
}