use std::collections::HashMap;
use anyhow::Result;
use serde_json;

/// Helper function to extract string value from prost_types::Struct
pub fn get_string(struct_value: &prost_types::Struct, field_name: &str) -> Option<String> {
    struct_value.fields.get(field_name).and_then(|f| {
        match &f.kind {
            Some(prost_types::value::Kind::StringValue(s)) => Some(s.clone()),
            _ => None,
        }
    })
}

/// Helper function to extract number value as u64 from prost_types::Struct
pub fn get_u64(struct_value: &prost_types::Struct, field_name: &str) -> u64 {
    struct_value.fields.get(field_name).and_then(|f| {
        match &f.kind {
            Some(prost_types::value::Kind::StringValue(s)) => s.parse::<u64>().ok(),
            Some(prost_types::value::Kind::NumberValue(n)) => Some(n.round() as u64),
            _ => None,
        }
    }).unwrap_or(0)
}

/// Helper function to extract boolean value from prost_types::Struct
pub fn get_bool(struct_value: &prost_types::Struct, field_name: &str) -> bool {
    struct_value.fields.get(field_name).and_then(|f| {
        match &f.kind {
            Some(prost_types::value::Kind::BoolValue(b)) => Some(*b),
            Some(prost_types::value::Kind::StringValue(s)) => {
                Some(s.to_lowercase() == "true")
            }
            _ => None,
        }
    }).unwrap_or(false)
}

/// Helper function to extract ObjectTable ID from nested struct
pub fn get_table_id(struct_value: &prost_types::Struct, field_name: &str) -> Result<String> {
    struct_value.fields.get(field_name)
        .and_then(|f| {
            if let Some(prost_types::value::Kind::StructValue(table_struct)) = &f.kind {
                table_struct.fields.get("id").and_then(|id_field| {
                    if let Some(prost_types::value::Kind::StringValue(id)) = &id_field.kind {
                        Some(id.clone())
                    } else {
                        None
                    }
                })
            } else {
                None
            }
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Failed to extract {} table ID from AppInstance",
                field_name
            )
        })
}

/// Convert proto value to JSON
pub fn proto_to_json(value: &prost_types::Value) -> serde_json::Value {
    match &value.kind {
        Some(prost_types::value::Kind::StringValue(s)) => serde_json::Value::String(s.clone()),
        Some(prost_types::value::Kind::NumberValue(n)) => serde_json::Value::Number(
            serde_json::Number::from_f64(*n).unwrap_or(serde_json::Number::from(0))
        ),
        Some(prost_types::value::Kind::BoolValue(b)) => serde_json::Value::Bool(*b),
        Some(prost_types::value::Kind::NullValue(_)) => serde_json::Value::Null,
        Some(prost_types::value::Kind::ListValue(list)) => {
            serde_json::Value::Array(
                list.values.iter().map(proto_to_json).collect()
            )
        }
        Some(prost_types::value::Kind::StructValue(s)) => {
            let map: serde_json::Map<String, serde_json::Value> = s.fields.iter()
                .map(|(k, v)| (k.clone(), proto_to_json(v)))
                .collect();
            serde_json::Value::Object(map)
        }
        None => serde_json::Value::Null,
    }
}

/// Helper function to parse VecMap<String, String> into HashMap
pub fn parse_string_vecmap(struct_value: &prost_types::Struct, field_name: &str) -> HashMap<String, String> {
    if let Some(field) = struct_value.fields.get(field_name) {
        if let Some(prost_types::value::Kind::StructValue(struct_)) = &field.kind {
            if let Some(contents_field) = struct_.fields.get("contents") {
                if let Some(prost_types::value::Kind::ListValue(list)) = &contents_field.kind {
                    let mut map = HashMap::new();
                    for entry in &list.values {
                        if let Some(prost_types::value::Kind::StructValue(entry_struct)) = &entry.kind {
                            if let (Some(key_field), Some(value_field)) = 
                                (entry_struct.fields.get("key"), entry_struct.fields.get("value")) {
                                if let (Some(prost_types::value::Kind::StringValue(key)),
                                        Some(prost_types::value::Kind::StringValue(value))) = 
                                    (&key_field.kind, &value_field.kind) {
                                    map.insert(key.clone(), value.clone());
                                }
                            }
                        }
                    }
                    return map;
                }
            }
        }
    }
    HashMap::new()
}

/// Helper function to parse VecMap<String, Struct> into HashMap<String, serde_json::Value>
/// This is used for VecMap fields where the value is a struct (like AppMethod)
pub fn parse_struct_vecmap(struct_value: &prost_types::Struct, field_name: &str) -> HashMap<String, serde_json::Value> {
    if let Some(field) = struct_value.fields.get(field_name) {
        if let Some(prost_types::value::Kind::StructValue(vecmap_struct)) = &field.kind {
            if let Some(contents_field) = vecmap_struct.fields.get("contents") {
                if let Some(prost_types::value::Kind::ListValue(list)) = &contents_field.kind {
                    let mut map = HashMap::new();
                    for entry in &list.values {
                        if let Some(prost_types::value::Kind::StructValue(entry_struct)) = &entry.kind {
                            if let (Some(key_field), Some(value_field)) = 
                                (entry_struct.fields.get("key"), entry_struct.fields.get("value")) {
                                if let Some(prost_types::value::Kind::StringValue(key)) = &key_field.kind {
                                    map.insert(key.clone(), proto_to_json(value_field));
                                }
                            }
                        }
                    }
                    return map;
                }
            }
        }
    }
    HashMap::new()
}