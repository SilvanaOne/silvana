use crate::error::{SilvanaSuiInterfaceError, Result};
use super::jobs_types::{Job, JobStatus};
use std::collections::HashSet;
use sui_rpc::Client;
use sui_rpc::proto::sui::rpc::v2beta2::{GetObjectRequest, ListDynamicFieldsRequest};
use tracing::{debug, info, warn};
use base64::{engine::general_purpose, Engine as _};

/// Extract Job from JSON representation
pub fn extract_job_from_json(json_value: &prost_types::Value) -> Result<Job> {
    if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
        //debug!("Job JSON fields: {:?}", struct_value.fields.keys().collect::<Vec<_>>());
        
        // Helper function to extract string value
        let get_string = |field_name: &str| -> Option<String> {
            struct_value.fields.get(field_name).and_then(|f| {
                match &f.kind {
                    Some(prost_types::value::Kind::StringValue(s)) => Some(s.clone()),
                    _ => None,
                }
            })
        };
        
        // Helper function to extract number as u64
        let get_u64 = |field_name: &str| -> u64 {
            struct_value.fields.get(field_name).and_then(|f| {
                match &f.kind {
                    Some(prost_types::value::Kind::StringValue(s)) => s.parse::<u64>().ok(),
                    Some(prost_types::value::Kind::NumberValue(n)) => Some(n.round() as u64),
                    _ => None,
                }
            }).unwrap_or(0)
        };
        
        // Helper function to extract number as u8
        let get_u8 = |field_name: &str| -> u8 {
            struct_value.fields.get(field_name).and_then(|f| {
                match &f.kind {
                    Some(prost_types::value::Kind::StringValue(s)) => s.parse::<u8>().ok(),
                    Some(prost_types::value::Kind::NumberValue(n)) => Some(n.round() as u8),
                    _ => None,
                }
            }).unwrap_or(0)
        };
        
        // Helper function to extract vector of u64
        let get_vec_u64 = |field_name: &str| -> Option<Vec<u64>> {
            struct_value.fields.get(field_name).and_then(|f| {
                match &f.kind {
                    Some(prost_types::value::Kind::ListValue(list)) => {
                        let mut values = Vec::new();
                        for val in &list.values {
                            match &val.kind {
                                Some(prost_types::value::Kind::StringValue(s)) => {
                                    if let Ok(num) = s.parse::<u64>() {
                                        values.push(num);
                                    }
                                }
                                Some(prost_types::value::Kind::NumberValue(n)) => {
                                    values.push(n.round() as u64);
                                }
                                _ => {}
                            }
                        }
                        if !values.is_empty() {
                            Some(values)
                        } else {
                            None
                        }
                    }
                    Some(prost_types::value::Kind::NullValue(_)) => None,
                    _ => None,
                }
            })
        };
        
        // Helper function to extract bytes data
        let get_bytes = |field_name: &str| -> Vec<u8> {
            struct_value.fields.get(field_name).and_then(|f| {
                if let Some(prost_types::value::Kind::StringValue(data_str)) = &f.kind {
                    // Try hex decode first, then base64
                    if let Ok(data) = hex::decode(data_str.trim_start_matches("0x")) {
                        Some(data)
                    } else if let Ok(data) = general_purpose::STANDARD.decode(data_str) {
                        Some(data)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }).unwrap_or_default()
        };
        
        // Extract job ID from the nested id field
        let id = struct_value.fields.get("id")
            .and_then(|field| {
                if let Some(prost_types::value::Kind::StructValue(id_struct)) = &field.kind {
                    id_struct.fields.get("id")
                        .and_then(|id_field| {
                            if let Some(prost_types::value::Kind::StringValue(id_str)) = &id_field.kind {
                                Some(id_str.clone())
                            } else {
                                None
                            }
                        })
                } else {
                    None
                }
            })
            .unwrap_or_default();
        
        // Parse status enum
        let status = struct_value.fields.get("status")
            .and_then(|field| {
                if let Some(prost_types::value::Kind::StructValue(status_struct)) = &field.kind {
                    // Parse JobStatus enum
                    if let Some(variant_field) = status_struct.fields.iter().next() {
                        match variant_field.0.as_str() {
                            "Pending" => Some(JobStatus::Pending),
                            "Running" => Some(JobStatus::Running),
                            "Failed" => {
                                if let Some(prost_types::value::Kind::StringValue(msg)) = &variant_field.1.kind {
                                    Some(JobStatus::Failed(msg.clone()))
                                } else {
                                    Some(JobStatus::Failed("Unknown error".to_string()))
                                }
                            }
                            _ => None,
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .unwrap_or(JobStatus::Pending);
        
        // Helper function to extract Option<u64>
        let get_option_u64 = |field_name: &str| -> Option<u64> {
            struct_value.fields.get(field_name).and_then(|f| {
                match &f.kind {
                    Some(prost_types::value::Kind::StringValue(s)) => s.parse::<u64>().ok(),
                    Some(prost_types::value::Kind::NumberValue(n)) => Some(n.round() as u64),
                    Some(prost_types::value::Kind::NullValue(_)) => None,
                    _ => None,
                }
            })
        };
        
        // Build the Job struct with all fields
        let job = Job {
            id,
            job_sequence: get_u64("job_sequence"),
            description: get_string("description"),
            developer: get_string("developer").unwrap_or_default(),
            agent: get_string("agent").unwrap_or_default(),
            agent_method: get_string("agent_method").unwrap_or_default(),
            app: get_string("app").unwrap_or_default(),
            app_instance: get_string("app_instance").unwrap_or_default(),
            app_instance_method: get_string("app_instance_method").unwrap_or_default(),
            block_number: if get_u64("block_number") > 0 { Some(get_u64("block_number")) } else { None },
            sequences: get_vec_u64("sequences"),
            sequences1: get_vec_u64("sequences1"),
            sequences2: get_vec_u64("sequences2"),
            data: get_bytes("data"),
            status,
            attempts: get_u8("attempts"),
            interval_ms: get_option_u64("interval_ms"),
            next_scheduled_at: get_option_u64("next_scheduled_at"),
            created_at: get_u64("created_at"),
            updated_at: get_u64("updated_at"),
        };
        
        return Ok(job);
    }
    
    Err(SilvanaSuiInterfaceError::ParseError(
        "Failed to extract job from JSON".to_string()
    ))
}

/// Fetch pending jobs from a specific app_instance
pub async fn fetch_pending_jobs_from_app_instance(
    client: &mut Client,
    app_instance_id: &str,
    only_check: bool,
) -> Result<Option<Job>> {
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
        .map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(
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
) -> Result<Option<Job>> {
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
        .map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(
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
                            .map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(
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
                                                .map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(
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
        .map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(
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
        .map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(
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

/// Fetch all jobs from an app instance
pub async fn fetch_all_jobs_from_app_instance(
    client: &mut Client,
    app_instance_id: &str,
) -> Result<Vec<Job>> {
    debug!("Fetching all jobs from app_instance {}", app_instance_id);
    
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
        .map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(
            format!("Failed to fetch AppInstance {}: {}", formatted_id, e)
        ))?;

    let mut all_jobs = Vec::new();

    if let Some(proto_object) = response.into_inner().object {
        if let Some(json_value) = &proto_object.json {
            if let Some(prost_types::value::Kind::StructValue(app_instance_struct)) = &json_value.kind {
                // Get the embedded jobs field
                if let Some(jobs_field) = app_instance_struct.fields.get("jobs") {
                    if let Some(prost_types::value::Kind::StructValue(jobs_struct)) = &jobs_field.kind {
                        // Get the jobs ObjectTable
                        if let Some(jobs_table_field) = jobs_struct.fields.get("jobs") {
                            if let Some(prost_types::value::Kind::StructValue(jobs_table_struct)) = &jobs_table_field.kind {
                                if let Some(table_id_field) = jobs_table_struct.fields.get("id") {
                                    if let Some(prost_types::value::Kind::StringValue(table_id)) = &table_id_field.kind {
                                        // Fetch all jobs from the ObjectTable
                                        let mut page_token = None;
                                        loop {
                                            let list_request = ListDynamicFieldsRequest {
                                                parent: Some(table_id.clone()),
                                                page_size: Some(100),
                                                page_token: page_token.clone(),
                                                read_mask: Some(prost_types::FieldMask {
                                                    paths: vec!["field_id".to_string()],
                                                }),
                                            };

                                            let fields_response = client
                                                .live_data_client()
                                                .list_dynamic_fields(list_request)
                                                .await
                                                .map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(
                                                    format!("Failed to list jobs: {}", e)
                                                ))?;

                                            let response = fields_response.into_inner();
                                            
                                            // Fetch each job
                                            for field in &response.dynamic_fields {
                                                if let Some(field_id) = &field.field_id {
                                                    // Fetch the job field wrapper
                                                    let job_request = GetObjectRequest {
                                                        object_id: Some(field_id.clone()),
                                                        version: None,
                                                        read_mask: Some(prost_types::FieldMask {
                                                            paths: vec!["json".to_string()],
                                                        }),
                                                    };

                                                    if let Ok(job_response) = client.ledger_client().get_object(job_request).await {
                                                        if let Some(job_object) = job_response.into_inner().object {
                                                            if let Some(job_json) = &job_object.json {
                                                                // Extract actual job ID from field wrapper
                                                                if let Some(prost_types::value::Kind::StructValue(field_struct)) = &job_json.kind {
                                                                    if let Some(value_field) = field_struct.fields.get("value") {
                                                                        if let Some(prost_types::value::Kind::StringValue(job_object_id)) = &value_field.kind {
                                                                            // Fetch actual job object
                                                                            let actual_job_request = GetObjectRequest {
                                                                                object_id: Some(job_object_id.clone()),
                                                                                version: None,
                                                                                read_mask: Some(prost_types::FieldMask {
                                                                                    paths: vec!["json".to_string()],
                                                                                }),
                                                                            };

                                                                            if let Ok(actual_job_response) = client.ledger_client().get_object(actual_job_request).await {
                                                                                if let Some(actual_job_object) = actual_job_response.into_inner().object {
                                                                                    if let Some(actual_job_json) = &actual_job_object.json {
                                                                                        if let Ok(job) = extract_job_from_json(actual_job_json) {
                                                                                            all_jobs.push(job);
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

                                            // Check for next page
                                            if let Some(next_token) = response.next_page_token {
                                                if !next_token.is_empty() {
                                                    page_token = Some(next_token);
                                                } else {
                                                    break;
                                                }
                                            } else {
                                                break;
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
    
    debug!("Found {} total jobs in app_instance {}", all_jobs.len(), app_instance_id);
    Ok(all_jobs)
}
