use anyhow::{Context, Result};
use inflector::Inflector;
use regex::Regex;
use std::fs;

#[derive(Debug, Clone)]
pub struct ProtoMessage {
    pub name: String,
    pub fields: Vec<ProtoField>,
}

#[derive(Debug, Clone)]
pub struct ProtoField {
    pub name: String,
    pub field_type: String,
    #[allow(dead_code)]
    pub field_number: u32,
    pub is_repeated: bool,
    pub is_optional: bool,
    pub has_search_option: bool,
    pub has_sequences_option: bool,
}

pub fn parse_proto_file(content: &str) -> Result<Vec<ProtoMessage>> {
    let mut messages = Vec::new();

    // Remove comments and normalize whitespace
    let content = remove_comments(content);

    // Regex to match message definitions
    let message_regex = Regex::new(r"message\s+(\w+)\s*\{([^{}]*(?:\{[^{}]*\}[^{}]*)*)\}")
        .context("Failed to compile message regex")?;

    for captures in message_regex.captures_iter(&content) {
        let message_name = captures.get(1).unwrap().as_str().to_string();
        let message_body = captures.get(2).unwrap().as_str();

        // Focus on Event messages that represent actual data tables
        if !message_name.ends_with("Event") {
            continue;
        }

        // Skip union/wrapper types and query request/response types
        if message_name == "CoordinatorEvent"
            || message_name == "AgentEvent"
            || message_name == "Event"
            || message_name.contains("Request")
            || message_name.contains("Response")
            || message_name.contains("WithId")
        {
            continue;
        }

        let fields = parse_message_fields(message_body)?;

        if !fields.is_empty() {
            messages.push(ProtoMessage {
                name: message_name,
                fields,
            });
        }
    }

    Ok(messages)
}

fn parse_message_fields(message_body: &str) -> Result<Vec<ProtoField>> {
    let mut fields = Vec::new();

    // Regex to match field definitions with optional field options
    let field_regex =
        Regex::new(r"(?:repeated\s+|optional\s+)?(\w+)\s+(\w+)\s*=\s*(\d+)(?:\s*\[([^\]]*)\])?")
            .context("Failed to compile field regex")?;

    for line in message_body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") || line.starts_with("oneof") {
            continue;
        }

        if let Some(captures) = field_regex.captures(line) {
            let is_repeated = line.trim_start().starts_with("repeated");
            let is_optional = line.trim_start().starts_with("optional");

            let field_type = captures.get(1).unwrap().as_str().to_string();
            let field_name = captures.get(2).unwrap().as_str().to_string();
            let field_number: u32 = captures
                .get(3)
                .unwrap()
                .as_str()
                .parse()
                .context("Failed to parse field number")?;

            // Parse field options if present
            let mut has_search_option = false;
            let mut has_sequences_option = false;

            if let Some(options_match) = captures.get(4) {
                let options_str = options_match.as_str();
                has_search_option = options_str.contains("(silvana.options.v1.search) = true")
                    || options_str.contains("(search) = true");
                has_sequences_option = options_str
                    .contains("(silvana.options.v1.sequences) = true")
                    || options_str.contains("(sequences) = true");
            }

            fields.push(ProtoField {
                name: field_name,
                field_type,
                field_number,
                is_repeated,
                is_optional,
                has_search_option,
                has_sequences_option,
            });
        }
    }

    Ok(fields)
}

fn remove_comments(content: &str) -> String {
    let comment_regex = Regex::new(r"//.*$").unwrap();
    content
        .lines()
        .map(|line| comment_regex.replace(line, "").to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn generate_mysql_ddl(messages: &[ProtoMessage], database: &str) -> Result<String> {
    let mut ddl = String::new();

    ddl.push_str("-- Generated DDL from protobuf file\n");
    ddl.push_str(
        "-- This schema represents the main event tables derived from protobuf messages\n\n",
    );

    // Generate individual event tables based on proto messages
    for message in messages {
        let table_name = message.name.to_snake_case();

        // Collect fields with `sequences` option – these will become child tables and NOT columns
        let mut sequence_fields: Vec<&ProtoField> = Vec::new();

        // Collect fields with `search` option for FULLTEXT indexes
        let mut search_fields: Vec<&ProtoField> = Vec::new();

        ddl.push_str(&format!("-- {} Table\n", message.name));
        ddl.push_str(&format!(
            "CREATE TABLE IF NOT EXISTS {} (\n",
            if database.is_empty() {
                table_name.clone()
            } else {
                format!("`{}`.`{}`", database, table_name)
            }
        ));

        // Add auto-increment primary key
        ddl.push_str("    `id` BIGINT AUTO_INCREMENT PRIMARY KEY,\n");

        // Convert proto fields to MySQL columns
        for field in &message.fields {
            // Fields with sequences option are extracted into child tables
            if field.has_sequences_option {
                sequence_fields.push(field);
                continue;
            }

            // Collect fields with search option for FULLTEXT indexes
            if field.has_search_option {
                search_fields.push(field);
            }

            let column_name = field.name.to_snake_case();
            let mysql_type = proto_type_to_mysql_with_options(
                &field.field_type,
                field.is_repeated,
                field.has_search_option,
                field.name.contains("metadata") || field.name.contains("message"),
            );
            let nullable = if field.is_optional || field.is_repeated {
                "NULL"
            } else {
                "NOT NULL"
            };

            // Wrap column name in backticks to handle reserved keywords
            ddl.push_str(&format!(
                "    `{}` {} {},\n",
                column_name, mysql_type, nullable
            ));
        }

        // Add metadata columns
        ddl.push_str("    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,\n");
        ddl.push_str(
            "    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,\n",
        );

        // Add indexes
        ddl.push_str("    INDEX idx_created_at (`created_at`)");

        // Add specific indexes based on common patterns
        for field in &message.fields {
            // Skip fields that become child tables
            if field.has_sequences_option {
                continue;
            }

            let column_name = field.name.to_snake_case();

            // For TEXT columns (search fields), use key length for regular indexes
            if column_name.contains("id") && column_name != "id" {
                ddl.push_str(&format!(
                    ",\n    INDEX idx_{} (`{}`)",
                    column_name, column_name
                ));
            }
            if column_name.contains("timestamp") {
                ddl.push_str(&format!(
                    ",\n    INDEX idx_{} (`{}`)",
                    column_name, column_name
                ));
            }
            if column_name.contains("hash") {
                ddl.push_str(&format!(
                    ",\n    INDEX idx_{} (`{}`)",
                    column_name, column_name
                ));
            }
        }

        // Add FULLTEXT indexes for string fields marked with search option
        for field in &search_fields {
            // Only add FULLTEXT indexes for string fields
            if field.field_type == "string" {
                let column_name = field.name.to_snake_case();
                // For TiDB, use VARCHAR type for FULLTEXT search and STANDARD parser
                ddl.push_str(&format!(
                    ",\n    FULLTEXT INDEX ft_idx_{} (`{}`) WITH PARSER STANDARD",
                    column_name, column_name
                ));
            }
        }

        ddl.push_str("\n) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;\n\n");

        // ------------------------------------------------------------------
        // Child tables for each field with sequences option
        // ------------------------------------------------------------------
        for field in sequence_fields {
            // Shorten long field names to avoid MySQL constraint name length limits
            let field_suffix = if field.name == "merged_sequences_1" {
                "ms1".to_string()
            } else if field.name == "merged_sequences_2" {
                "ms2".to_string()
            } else {
                field.name.to_snake_case()
            };

            let child_table_name = format!("{}_{}", table_name, field_suffix);
            let parent_fk = format!("{}_id", table_name); // e.g. agent_message_event_id
            let value_col = field.name.to_singular().to_snake_case(); // e.g. sequences -> sequence

            let mysql_type = proto_type_to_mysql_with_options(
                &field.field_type,
                /* is_repeated = */ false,
                false,
                field.name.contains("metadata") || field.name.contains("message"),
            );

            ddl.push_str(&format!(
                "-- Child table for repeated field `{}`\n",
                field.name
            ));
            ddl.push_str(&format!(
                "CREATE TABLE IF NOT EXISTS {} (\n",
                if database.is_empty() {
                    child_table_name.clone()
                } else {
                    format!("`{}`.`{}`", database, child_table_name)
                }
            ));
            ddl.push_str("    `id` BIGINT AUTO_INCREMENT PRIMARY KEY,\n");
            ddl.push_str(&format!("    `{}` BIGINT NOT NULL,\n", parent_fk));
            ddl.push_str(&format!("    `{}` {} NOT NULL,\n", value_col, mysql_type));
            ddl.push_str("    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,\n");
            ddl.push_str("    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,\n");
            ddl.push_str(&format!(
                "    INDEX idx_{}_parent (`{}`),\n",
                child_table_name, parent_fk
            ));
            ddl.push_str(&format!(
                "    INDEX idx_{}_value (`{}`),\n",
                child_table_name, value_col
            ));
            ddl.push_str(&format!(
                "    CONSTRAINT fk_{}_{} FOREIGN KEY (`{}`) REFERENCES {} (`id`) ON DELETE CASCADE\n",
                child_table_name, parent_fk, parent_fk, table_name
            ));
            ddl.push_str(") ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;\n\n");
        }
    }

    Ok(ddl)
}

pub fn generate_entities(messages: &[ProtoMessage], output_dir: &str) -> Result<()> {
    // Create output directory
    std::fs::create_dir_all(output_dir)?;

    // Generate mod.rs
    generate_mod_file(messages, output_dir)?;

    // Generate each entity
    for message in messages {
        generate_entity_file(message, output_dir)?;

        // Also generate entities for each field with sequences option (child tables)
        for field in &message.fields {
            if field.has_sequences_option {
                generate_child_entity_file(message, field, output_dir)?;
            }
        }
    }

    Ok(())
}

fn generate_mod_file(messages: &[ProtoMessage], output_dir: &str) -> Result<()> {
    let mut content = String::new();

    content.push_str("//! Sea-ORM entities generated from proto file\n");
    content.push_str("//! This maintains the proto file as the single source of truth\n\n");

    // Module declarations
    for message in messages {
        let module_name = message.name.to_snake_case();
        content.push_str(&format!("pub mod {};\n", module_name));
    }

    // Child modules for fields with sequences option
    for message in messages {
        for field in &message.fields {
            if field.has_sequences_option {
                // Shorten long field names to avoid MySQL constraint name length limits
                let field_suffix = if field.name == "merged_sequences_1" {
                    "ms1".to_string()
                } else if field.name == "merged_sequences_2" {
                    "ms2".to_string()
                } else {
                    field.name.to_snake_case()
                };

                let child_mod = format!(
                    "{}_{}",
                    message.name.to_snake_case(),
                    field_suffix
                );
                content.push_str(&format!("pub mod {};\n", child_mod));
            }
        }
    }

    // content.push_str("\n// Re-export all entities\n");
    // for message in messages {
    //     let module_name = message.name.to_snake_case();
    //     let entity_name = message.name.clone();
    //     content.push_str(&format!(
    //         "pub use {}::Entity as {};\n",
    //         module_name, entity_name
    //     ));
    // }

    // for message in messages {
    //     for field in &message.fields {
    //         if field.is_repeated {
    //             let child_mod = format!(
    //                 "{}_{}",
    //                 message.name.to_snake_case(),
    //                 field.name.to_snake_case()
    //             );
    //             let entity_name = format!("{}{}", &message.name, field.name.to_class_case()); // e.g. AgentMessageEventSequences
    //             content.push_str(&format!(
    //                 "pub use {}::Entity as {};\n",
    //                 child_mod, entity_name
    //             ));
    //         }
    //     }
    // }

    fs::write(format!("{}/mod.rs", output_dir), content)?;
    Ok(())
}

fn generate_entity_file(message: &ProtoMessage, output_dir: &str) -> Result<()> {
    let module_name = message.name.to_snake_case();
    let table_name = message.name.to_snake_case();

    let mut content = String::new();

    // File header
    content.push_str(&format!(
        "//! {} entity\n//! Generated from proto definition: {}\n\n",
        message.name, message.name
    ));

    // Imports
    content.push_str("use sea_orm::entity::prelude::*;\n");
    //content.push_str("use serde::{Deserialize, Serialize};\n\n");

    // Model struct
    content.push_str(
        "#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]\n", //, Serialize, Deserialize
    );
    content.push_str(&format!("#[sea_orm(table_name = \"{}\")]\n", table_name));
    content.push_str("pub struct Model {\n");

    // Primary key
    content.push_str("    #[sea_orm(primary_key)]\n");
    content.push_str("    pub id: i64,\n");

    // Proto fields
    for field in &message.fields {
        // Skip fields with sequences option - they are handled by child tables
        if field.has_sequences_option {
            continue;
        }

        let field_name = field.name.to_snake_case();
        let rust_type = proto_type_to_rust(&field.field_type, field.is_repeated, field.is_optional);
        content.push_str(&format!("    pub {}: {},\n", field_name, rust_type));
    }

    // Metadata fields
    content.push_str("    pub created_at: Option<DateTimeUtc>,\n");
    content.push_str("    pub updated_at: Option<DateTimeUtc>,\n");

    content.push_str("}\n\n");

    // Relations
    content.push_str("#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]\n");
    content.push_str("pub enum Relation {}\n\n");

    // Active model behavior
    content.push_str("impl ActiveModelBehavior for ActiveModel {}\n");

    fs::write(format!("{}/{}.rs", output_dir, module_name), content)?;
    Ok(())
}

fn generate_child_entity_file(
    parent: &ProtoMessage,
    field: &ProtoField,
    output_dir: &str,
) -> Result<()> {
    // Shorten long field names to avoid MySQL constraint name length limits
    let field_suffix = if field.name == "merged_sequences_1" {
        "ms1".to_string()
    } else if field.name == "merged_sequences_2" {
        "ms2".to_string()
    } else {
        field.name.to_snake_case()
    };

    let module_name = format!(
        "{}_{}",
        parent.name.to_snake_case(),
        field_suffix
    );
    let table_name = module_name.clone();
    let parent_fk = format!("{}_id", parent.name.to_snake_case());
    let value_col = field.name.to_singular().to_snake_case();
    let rust_type = proto_type_to_rust(
        &field.field_type,
        /* is_repeated = */ false,
        /* is_optional = */ false,
    );

    let mut content = String::new();
    content.push_str(&format!(
        "//! Child entity for `{}`. `{}` -> `{}`\n\n",
        field.name, parent.name, module_name
    ));
    content.push_str("use sea_orm::entity::prelude::*;\n");
    //content.push_str("use serde::{Deserialize, Serialize};\n\n");

    // Model struct
    content.push_str(
        "#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]\n", //, Serialize, Deserialize
    );
    content.push_str(&format!("#[sea_orm(table_name = \"{}\")]\n", table_name));
    content.push_str("pub struct Model {\n");
    content.push_str("    #[sea_orm(primary_key)]\n");
    content.push_str("    pub id: i64,\n");
    content.push_str(&format!("    pub {}: i64,\n", parent_fk));
    content.push_str(&format!("    pub {}: {},\n", value_col, rust_type));
    content.push_str("    pub created_at: Option<DateTimeUtc>,\n");
    content.push_str("    pub updated_at: Option<DateTimeUtc>,\n");
    content.push_str("}\n\n");

    // Relations
    content.push_str("#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]\n");
    content.push_str("pub enum Relation {}\n\n");

    content.push_str("impl ActiveModelBehavior for ActiveModel {}\n");

    std::fs::write(format!("{}/{}.rs", output_dir, module_name), content)?;
    Ok(())
}

pub fn proto_type_to_mysql_with_options(
    proto_type: &str,
    is_repeated: bool,
    has_search_option: bool,
    long_text: bool,
) -> String {
    if is_repeated {
        return "JSON".to_string();
    }

    match proto_type {
        "string" => {
            if has_search_option {
                if long_text {
                    "TEXT".to_string()
                } else {
                    "VARCHAR(255)".to_string()
                } // Use VARCHAR for searchable fields - supports both regular and FULLTEXT indexes
            } else {
                "VARCHAR(255)".to_string()
            }
        }
        "bytes" => "BLOB".to_string(),
        "int32" | "sint32" | "sfixed32" => "INT".to_string(),
        "int64" | "sint64" | "sfixed64" => "BIGINT".to_string(),
        "uint32" | "fixed32" => "INT UNSIGNED".to_string(),
        "uint64" | "fixed64" => "BIGINT".to_string(),
        "float" => "FLOAT".to_string(),
        "double" => "DOUBLE".to_string(),
        "bool" => "BOOLEAN".to_string(),

        // Handle known enum types
        "LogLevel" => "TINYINT".to_string(),

        // Handle custom message types as JSON for now
        _ if proto_type.chars().next().unwrap().is_uppercase() => "JSON".to_string(),
        _ => "TEXT".to_string(),
    }
}

pub fn proto_type_to_rust(proto_type: &str, _is_repeated: bool, is_optional: bool) -> String {
    let base_type = match proto_type {
        "string" => "String".to_string(),
        "bytes" => "Vec<u8>".to_string(),
        "int32" | "sint32" | "sfixed32" => "i32".to_string(),
        "int64" | "sint64" | "sfixed64" => "i64".to_string(),
        "uint32" | "fixed32" => "u32".to_string(),
        "uint64" | "fixed64" => "i64".to_string(), // Use i64 for compatibility
        "float" => "f32".to_string(),
        "double" => "f64".to_string(),
        "bool" => "bool".to_string(),

        // Handle known enum types
        "LogLevel" => "i32".to_string(), // Protobuf enums are typically i32
        _ => "String".to_string(),
    };

    if is_optional {
        format!("Option<{}>", base_type)
    } else {
        base_type
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proto_type_conversion() {
        assert_eq!(
            proto_type_to_mysql_with_options("string", false, false, false),
            "VARCHAR(255)"
        );
        assert_eq!(
            proto_type_to_mysql_with_options("int64", false, false, false),
            "BIGINT"
        );
        assert_eq!(
            proto_type_to_mysql_with_options("uint64", false, false, false),
            "BIGINT"
        );
        assert_eq!(
            proto_type_to_mysql_with_options("bool", false, false, false),
            "BOOLEAN"
        );
        assert_eq!(
            proto_type_to_mysql_with_options("bytes", false, false, false),
            "BLOB"
        );
        assert_eq!(
            proto_type_to_mysql_with_options("string", true, false, false),
            "JSON"
        );
    }

    #[test]
    fn test_rust_type_conversion() {
        assert_eq!(proto_type_to_rust("string", false, false), "String");
        assert_eq!(proto_type_to_rust("string", false, true), "Option<String>");
        assert_eq!(proto_type_to_rust("uint64", false, false), "i64");
        assert_eq!(proto_type_to_rust("string", true, false), "Option<String>");
    }

    #[test]
    fn test_field_parsing() {
        let message_body = r#"
            string coordinator_id = 1 [ (silvana.options.v1.search) = true];
            uint64 timestamp = 2;
            repeated uint64 sequences = 3 [ (silvana.options.v1.sequences) = true];
            optional string description = 4;
        "#;

        let fields = parse_message_fields(message_body).unwrap();
        assert_eq!(fields.len(), 4);
        assert_eq!(fields[0].name, "coordinator_id");
        assert_eq!(fields[0].field_type, "string");
        assert!(!fields[0].is_repeated);
        assert!(!fields[0].is_optional);
        assert!(fields[0].has_search_option);
        assert!(!fields[0].has_sequences_option);

        assert!(fields[2].is_repeated);
        assert!(fields[2].has_sequences_option);
        assert!(!fields[2].has_search_option);

        assert!(fields[3].is_optional);
        assert!(!fields[3].has_search_option);
        assert!(!fields[3].has_sequences_option);
    }
}
