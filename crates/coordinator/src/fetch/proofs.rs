use crate::error::{CoordinatorError, Result};
use sui_rpc::Client;
use sui_rpc::proto::sui::rpc::v2beta2::{GetObjectRequest, ListDynamicFieldsRequest};
use tracing::debug;

/// ProofCalculation information fetched from blockchain
#[derive(Debug, Clone)]
pub struct ProofCalculationInfo {
    pub block_number: u64,
    pub sequences: Vec<u64>,
    pub job_id: String,
}

/// Fetch all ProofCalculations for a block from AppInstance
pub async fn fetch_proof_calculations(
    client: &mut Client,
    app_instance: &str,
    block_number: u64,
) -> Result<Vec<ProofCalculationInfo>> {
    debug!("Fetching ProofCalculations for block {} from app_instance {}", block_number, app_instance);
    
    // Ensure the app_instance has 0x prefix
    let formatted_id = if app_instance.starts_with("0x") {
        app_instance.to_string()
    } else {
        format!("0x{}", app_instance)
    };
    
    let request = GetObjectRequest {
        object_id: Some(formatted_id.clone()),
        version: None,
        read_mask: Some(prost_types::FieldMask {
            paths: vec![
                "object_id".to_string(),
                "json".to_string(),
            ],
        }),
    };
    
    let object_response = client.ledger_client().get_object(request).await.map_err(|e| CoordinatorError::RpcConnectionError(
        format!("Failed to fetch app_instance {}: {}", app_instance, e)
    ))?;
    
    let response = object_response.into_inner();
    
    if let Some(proto_object) = response.object {
        if let Some(json_value) = &proto_object.json {
            if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
                // Get the proof_calculations ObjectTable
                if let Some(proofs_field) = struct_value.fields.get("proof_calculations") {
                    if let Some(prost_types::value::Kind::StructValue(proofs_struct)) = &proofs_field.kind {
                        if let Some(table_id_field) = proofs_struct.fields.get("id") {
                            if let Some(prost_types::value::Kind::StringValue(table_id)) = &table_id_field.kind {
                                // Fetch all ProofCalculations from the ObjectTable
                                return fetch_proof_calculations_from_table(client, table_id, block_number).await;
                            }
                        }
                    }
                }
            }
        }
    }
    
    Ok(vec![])
}

/// Fetch ProofCalculations from ObjectTable, filtering by block number
async fn fetch_proof_calculations_from_table(
    client: &mut Client,
    table_id: &str,
    target_block_number: u64,
) -> Result<Vec<ProofCalculationInfo>> {
    let request = ListDynamicFieldsRequest {
        parent: Some(table_id.to_string()),
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
    
    let fields_response = client.live_data_client().list_dynamic_fields(request).await.map_err(|e| CoordinatorError::RpcConnectionError(
        format!("Failed to list dynamic fields: {}", e)
    ))?;
    
    let response = fields_response.into_inner();
    let mut proofs = Vec::new();
    
    for field in &response.dynamic_fields {
        if let Some(field_id) = &field.field_id {
            // Fetch each ProofCalculation
            let proof_request = GetObjectRequest {
                object_id: Some(field_id.clone()),
                version: None,
                read_mask: Some(prost_types::FieldMask {
                    paths: vec![
                        "object_id".to_string(),
                        "json".to_string(),
                    ],
                }),
            };
        
        let proof_response = client.ledger_client().get_object(proof_request).await.map_err(|e| CoordinatorError::RpcConnectionError(
        format!("Failed to fetch proof calculation: {}", e)
    ))?;
        
        let proof_response_inner = proof_response.into_inner();
        if let Some(proof_object) = proof_response_inner.object {
            if let Some(proof_json) = &proof_object.json {
                // Extract the actual proof calculation object ID from the Field wrapper
                if let Some(prost_types::value::Kind::StructValue(struct_value)) = &proof_json.kind {
                    if let Some(value_field) = struct_value.fields.get("value") {
                        if let Some(prost_types::value::Kind::StringValue(proof_object_id)) = &value_field.kind {
                            // Fetch the actual proof calculation object
                            let actual_proof_request = GetObjectRequest {
                                object_id: Some(proof_object_id.clone()),
                                version: None,
                                read_mask: Some(prost_types::FieldMask {
                                    paths: vec![
                                        "object_id".to_string(),
                                        "json".to_string(),
                                    ],
                                }),
                            };
                            
                            let actual_proof_response = client.ledger_client().get_object(actual_proof_request).await.map_err(|e| CoordinatorError::RpcConnectionError(
                                format!("Failed to fetch actual proof calculation: {}", e)
                            ))?;
                            
                            if let Some(actual_proof_object) = actual_proof_response.into_inner().object {
                                if let Some(actual_proof_json) = &actual_proof_object.json {
                                    if let Some(proof_info) = extract_proof_calculation_from_json(actual_proof_json) {
                                        // Filter by block number
                                        if proof_info.block_number == target_block_number {
                                            proofs.push(proof_info);
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
    
    debug!("Found {} ProofCalculations for block {}", proofs.len(), target_block_number);
    Ok(proofs)
}

/// Extract ProofCalculation information from JSON
fn extract_proof_calculation_from_json(json_value: &prost_types::Value) -> Option<ProofCalculationInfo> {
    if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
        let mut block_number = 0u64;
        let mut sequences = Vec::new();
        let mut job_id = String::new();
        
        if let Some(block_field) = struct_value.fields.get("block_number") {
            if let Some(prost_types::value::Kind::StringValue(block_str)) = &block_field.kind {
                block_number = block_str.parse().unwrap_or(0);
            }
        }
        
        if let Some(sequences_field) = struct_value.fields.get("sequences") {
            if let Some(prost_types::value::Kind::ListValue(list_value)) = &sequences_field.kind {
                for value in &list_value.values {
                    if let Some(prost_types::value::Kind::StringValue(seq_str)) = &value.kind {
                        if let Ok(seq) = seq_str.parse::<u64>() {
                            sequences.push(seq);
                        }
                    }
                }
            }
        }
        
        if let Some(job_id_field) = struct_value.fields.get("job_id") {
            if let Some(prost_types::value::Kind::StringValue(job_id_str)) = &job_id_field.kind {
                job_id = job_id_str.clone();
            }
        }
        
        return Some(ProofCalculationInfo {
            block_number,
            sequences,
            job_id,
        });
    }
    
    None
}