use crate::error::{SilvanaSuiInterfaceError, Result};
use crate::state::SharedSuiState;
use serde::Deserialize;
use std::env;
use sui_rpc::client::v2::Client;
use sui_rpc::proto::sui::rpc::v2::GetObjectRequest;

#[derive(Debug, Deserialize)]
pub struct AgentMethod {
    pub docker_image: String,
    pub docker_sha256: Option<String>,
    pub min_memory_gb: u16,
    pub min_cpu_cores: u16,
    pub requires_tee: bool,
}
// TODO: add pagination
pub async fn fetch_agent_method(
    developer_name: &str,
    agent_name: &str,
    method_name: &str,
) -> Result<AgentMethod> {
    let mut client = SharedSuiState::get_instance().get_sui_client();

    // Get the registry package ID from environment variable
    let _registry_package = env::var("SILVANA_REGISTRY_PACKAGE")
        .map_err(|_| SilvanaSuiInterfaceError::ParseError("SILVANA_REGISTRY_PACKAGE not set".to_string()))?;
    
    // The registry object ID is a shared object at a fixed address
    let registry_id = env::var("SILVANA_REGISTRY")
        .map_err(|_| SilvanaSuiInterfaceError::ParseError("SILVANA_REGISTRY not set".to_string()))?;


    // First, fetch the registry object with JSON representation
    let mut registry_request = GetObjectRequest::default();
    registry_request.object_id = Some(registry_id.clone());
    registry_request.version = None; // Get latest version
    registry_request.read_mask = Some(prost_types::FieldMask {
        paths: vec![
            "object_id".to_string(),
            "version".to_string(),
            "object_type".to_string(),
            "owner".to_string(),
            "json".to_string(),  // Request JSON representation
        ],
    });

    let registry_response = client
        .ledger_client()
        .get_object(registry_request)
        .await
        .map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(format!("Failed to fetch registry: {}", e)))?;

    // Parse the response to extract the AgentMethod
    let response = registry_response.into_inner();
    
    if let Some(proto_object) = response.object {
        // Extract developers table ID from JSON
        if let Some(json_value) = &proto_object.json {
            if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
                if let Some(developers_field) = struct_value.fields.get("developers") {
                    if let Some(prost_types::value::Kind::StructValue(dev_struct)) = &developers_field.kind {
                        if let Some(id_field) = dev_struct.fields.get("id") {
                            if let Some(prost_types::value::Kind::StringValue(developers_table_id)) = &id_field.kind {
                                
                                // Now fetch the developer from the developers table
                                return fetch_developer_and_agent(
                                    &mut client,
                                    developers_table_id,
                                    developer_name,
                                    agent_name,
                                    method_name,
                                ).await;
                            }
                        }
                    }
                }
            }
        }
    }

    Err(SilvanaSuiInterfaceError::ParseError(format!(
        "Failed to fetch agent method {}/{}/{}",
        developer_name, agent_name, method_name
    )))
}

// Fetch developer from developers table, then agent from agents table
async fn fetch_developer_and_agent(
    client: &mut Client,
    developers_table_id: &str,
    developer_name: &str,
    agent_name: &str,
    method_name: &str,
) -> Result<AgentMethod> {
    use sui_rpc::proto::sui::rpc::v2::ListDynamicFieldsRequest;
    
    let mut page_token: Option<tonic::codegen::Bytes> = None;
    
    // Loop through pages to find the developer
    loop {
        // List dynamic fields in the developers table to find our developer
        let mut list_request = ListDynamicFieldsRequest::default();
        list_request.parent = Some(developers_table_id.to_string());
        list_request.page_size = Some(100);
        list_request.page_token = page_token.clone();
        // Specify minimal fields that trigger name loading
        // The name field is a Bcs message type with subfields like value
        list_request.read_mask = Some(prost_types::FieldMask {
            paths: vec![
                "parent".to_string(),
                "field_id".to_string(),
                "kind".to_string(),
                "name".to_string(), // Request the entire name Bcs message
            ],
        });
        
        let list_response = client
            .state_client()
            .list_dynamic_fields(list_request)
            .await
            .map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(format!("Failed to list developers: {}", e)))?;
        
        let response = list_response.into_inner();
        
        // Find the developer by name in this page
        for field in &response.dynamic_fields {
            if let Some(name_bcs) = &field.name {
                if let Some(name_value) = &name_bcs.value {
                    // The name_value is BCS-encoded String
                    if let Ok(name) = bcs::from_bytes::<String>(name_value) {
                        if name == developer_name {

                            // Get the developer field object ID
                            if let Some(field_id) = &field.field_id {
                                // Fetch the developer object with JSON
                                let mut dev_request = GetObjectRequest::default();
                                dev_request.object_id = Some(field_id.clone());
                                dev_request.version = None;
                                dev_request.read_mask = Some(prost_types::FieldMask {
                                    paths: vec![
                                        "object_id".to_string(),
                                        "object_type".to_string(),
                                        "json".to_string(),
                                    ],
                                });

                                let dev_response = client
                                    .ledger_client()
                                    .get_object(dev_request)
                                    .await
                                    .map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(format!("Failed to fetch developer: {}", e)))?;

                                if let Some(dev_object) = dev_response.into_inner().object {
                                    // This is a Field wrapper, extract the actual developer object ID from value
                                    if let Some(json_value) = &dev_object.json {
                                        if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
                                            // The Field has "value" which contains the actual developer object ID
                                            if let Some(value_field) = struct_value.fields.get("value") {
                                                if let Some(prost_types::value::Kind::StringValue(developer_object_id)) = &value_field.kind {

                                                    // Now fetch the actual developer object
                                                    let mut actual_dev_request = GetObjectRequest::default();
                                                    actual_dev_request.object_id = Some(developer_object_id.clone());
                                                    actual_dev_request.version = None;
                                                    actual_dev_request.read_mask = Some(prost_types::FieldMask {
                                                        paths: vec![
                                                            "object_id".to_string(),
                                                            "object_type".to_string(),
                                                            "json".to_string(),
                                                        ],
                                                    });

                                                    let actual_dev_response = client
                                                        .ledger_client()
                                                        .get_object(actual_dev_request)
                                                        .await
                                                        .map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(format!("Failed to fetch actual developer: {}", e)))?;

                                                    if let Some(actual_dev_object) = actual_dev_response.into_inner().object {
                                                        // Now extract agents table from the actual developer object
                                                        if let Some(json_value) = &actual_dev_object.json {
                                                            if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
                                                                // Look for agents field directly (it should be at top level now)
                                                                if let Some(agents_field) = struct_value.fields.get("agents") {
                                                                    if let Some(prost_types::value::Kind::StructValue(agents_struct)) = &agents_field.kind {
                                                                        if let Some(id_field) = agents_struct.fields.get("id") {
                                                                            if let Some(prost_types::value::Kind::StringValue(agents_table_id)) = &id_field.kind {

                                                                                // Now fetch the agent from the agents table
                                                                                return fetch_agent_and_method(
                                                                                    client,
                                                                                    agents_table_id,
                                                                                    agent_name,
                                                                                    method_name,
                                                                                ).await;
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
        
        // Check if there are more pages
        if let Some(next_token) = response.next_page_token {
            if !next_token.is_empty() {
                page_token = Some(next_token);
                // Continue to next page
            } else {
                break;
            }
        } else {
            break;
        }
    }
    
    Err(SilvanaSuiInterfaceError::ParseError(format!(
        "Developer {} not found in registry",
        developer_name
    )))
}

// Fetch agent from agents table and extract method
async fn fetch_agent_and_method(
    client: &mut Client,
    agents_table_id: &str,
    agent_name: &str,
    method_name: &str,
) -> Result<AgentMethod> {
    use sui_rpc::proto::sui::rpc::v2::ListDynamicFieldsRequest;
    
    let mut page_token: Option<tonic::codegen::Bytes> = None;
    
    // Loop through pages to find the agent
    loop {
        // List dynamic fields in the agents table to find our agent
        let mut list_request = ListDynamicFieldsRequest::default();
        list_request.parent = Some(agents_table_id.to_string());
        list_request.page_size = Some(100);
        list_request.page_token = page_token.clone();
        // Specify minimal fields that trigger name loading
        // The name field is a Bcs message type with subfields like value
        list_request.read_mask = Some(prost_types::FieldMask {
            paths: vec![
                "parent".to_string(),
                "field_id".to_string(),
                "kind".to_string(),
                "name".to_string(), // Request the entire name Bcs message
            ],
        });
        
        let list_response = client
            .state_client()
            .list_dynamic_fields(list_request)
            .await
            .map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(format!("Failed to list agents: {}", e)))?;
        
        let response = list_response.into_inner();
        
        // Find the agent by name in this page
        for field in &response.dynamic_fields {
            if let Some(name_bcs) = &field.name {
                if let Some(name_value) = &name_bcs.value {
                    // The name_value is BCS-encoded String
                    if let Ok(name) = bcs::from_bytes::<String>(name_value) {
                        if name == agent_name {
                        
                        // Get the agent object ID
                        if let Some(field_id) = &field.field_id {
                        // Fetch the agent object with JSON
                        let mut agent_request = GetObjectRequest::default();
                        agent_request.object_id = Some(field_id.clone());
                        agent_request.version = None;
                        agent_request.read_mask = Some(prost_types::FieldMask {
                            paths: vec![
                                "object_id".to_string(),
                                "object_type".to_string(),
                                "json".to_string(),
                            ],
                        });
                        
                        let agent_response = client
                            .ledger_client()
                            .get_object(agent_request)
                            .await
                            .map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(format!("Failed to fetch agent: {}", e)))?;
                        
                        if let Some(agent_object) = agent_response.into_inner().object {
                            // This is a Field wrapper, extract the actual agent object ID from value
                            if let Some(json_value) = &agent_object.json {
                                if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
                                    // The Field has "value" which contains the actual agent object ID
                                    if let Some(value_field) = struct_value.fields.get("value") {
                                        if let Some(prost_types::value::Kind::StringValue(agent_object_id)) = &value_field.kind {
                                            
                                            // Now fetch the actual agent object
                                            let mut actual_agent_request = GetObjectRequest::default();
                                            actual_agent_request.object_id = Some(agent_object_id.clone());
                                            actual_agent_request.version = None;
                                            actual_agent_request.read_mask = Some(prost_types::FieldMask {
                                                paths: vec![
                                                    "object_id".to_string(),
                                                    "object_type".to_string(),
                                                    "json".to_string(),
                                                ],
                                            });
                                            
                                            let actual_agent_response = client
                                                .ledger_client()
                                                .get_object(actual_agent_request)
                                                .await
                                                .map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(format!("Failed to fetch actual agent: {}", e)))?;
                                            
                                            if let Some(actual_agent_object) = actual_agent_response.into_inner().object {
                                                // Extract methods from actual agent JSON
                                                if let Some(json_value) = &actual_agent_object.json {
                                                    return extract_method_from_agent_json(json_value, method_name);
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

        // Check if there are more pages
        if let Some(next_token) = response.next_page_token {
            if !next_token.is_empty() {
                page_token = Some(next_token);
                // Continue to next page
            } else {
                break;
            }
        } else {
            break;
        }
    }
    
    Err(SilvanaSuiInterfaceError::ParseError(format!(
        "Agent {} not found in developer's agents",
        agent_name
    )))
}

// Extract AgentMethod from agent's JSON representation
fn extract_method_from_agent_json(
    json_value: &prost_types::Value,
    method_name: &str,
) -> Result<AgentMethod> {
    if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
        // The agent object should have methods field directly at top level
        if let Some(methods_field) = struct_value.fields.get("methods") {
            // VecMap is represented as a struct with a "contents" field
            if let Some(prost_types::value::Kind::StructValue(methods_struct)) = &methods_field.kind {
                if let Some(contents_field) = methods_struct.fields.get("contents") {
                    if let Some(prost_types::value::Kind::ListValue(methods_list)) = &contents_field.kind {
                        // Iterate through methods array (VecMap entries are structs with key/value fields)
                        for method_entry in &methods_list.values {
                            if let Some(prost_types::value::Kind::StructValue(entry_struct)) = &method_entry.kind {
                                // Extract key and value from the entry struct
                                if let (Some(key_field), Some(value_field)) = 
                                    (entry_struct.fields.get("key"), entry_struct.fields.get("value")) {
                                    
                                    if let Some(prost_types::value::Kind::StringValue(name)) = &key_field.kind {
                                        if name == method_name {
                                            // Extract the method details from the value field
                                            if let Some(prost_types::value::Kind::StructValue(method_struct)) = &value_field.kind {
                                                return extract_agent_method_from_struct(method_struct);
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
    
    Err(SilvanaSuiInterfaceError::ParseError(format!(
        "Method {} not found in agent",
        method_name
    )))
}

// Extract AgentMethod fields from the method struct
fn extract_agent_method_from_struct(method_struct: &prost_types::Struct) -> Result<AgentMethod> {
    let mut docker_image = String::new();
    let mut docker_sha256 = None;
    let mut min_memory_gb = 1;
    let mut min_cpu_cores = 1;
    let mut requires_tee = false;
    
    // Extract fields from the method struct
    if let Some(image_field) = method_struct.fields.get("docker_image") {
        if let Some(prost_types::value::Kind::StringValue(image)) = &image_field.kind {
            docker_image = image.clone();
        }
    }
    
    if let Some(sha_field) = method_struct.fields.get("docker_sha256") {
        match &sha_field.kind {
            Some(prost_types::value::Kind::StringValue(sha)) => {
                docker_sha256 = Some(sha.clone());
            }
            Some(prost_types::value::Kind::NullValue(_)) => {
                docker_sha256 = None;
            }
            _ => {}
        }
    }
    
    if let Some(memory_field) = method_struct.fields.get("min_memory_gb") {
        match &memory_field.kind {
            Some(prost_types::value::Kind::NumberValue(memory)) => {
                min_memory_gb = *memory as u16;
            }
            Some(prost_types::value::Kind::StringValue(memory_str)) => {
                min_memory_gb = memory_str.parse().unwrap_or(1);
            }
            _ => {}
        }
    }
    
    if let Some(cpu_field) = method_struct.fields.get("min_cpu_cores") {
        match &cpu_field.kind {
            Some(prost_types::value::Kind::NumberValue(cpu)) => {
                min_cpu_cores = *cpu as u16;
            }
            Some(prost_types::value::Kind::StringValue(cpu_str)) => {
                min_cpu_cores = cpu_str.parse().unwrap_or(1);
            }
            _ => {}
        }
    }
    
    if let Some(tee_field) = method_struct.fields.get("requires_tee") {
        if let Some(prost_types::value::Kind::BoolValue(tee)) = &tee_field.kind {
            requires_tee = *tee;
        }
    }
    
    if docker_image.is_empty() {
        return Err(SilvanaSuiInterfaceError::ParseError(
            "Docker image not found in method".to_string()
        ));
    }
    
    Ok(AgentMethod {
        docker_image,
        docker_sha256,
        min_memory_gb,
        min_cpu_cores,
        requires_tee,
    })
}