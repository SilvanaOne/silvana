use anyhow::Result;
use inflector::Inflector;

#[derive(Debug, Clone)]
pub struct FulltextIndex {
    pub table_name: String,
    pub column_name: String,
    pub index_name: String,
}

impl FulltextIndex {
    pub fn new(table_name: String, column_name: String) -> Self {
        let index_name = format!("ft_idx_{}", column_name);
        Self {
            table_name,
            column_name,
            index_name,
        }
    }
}

/// Generate a Rust module with all fulltext indexes for use in search
pub fn generate_fulltext_index_metadata(messages: &[crate::parser::ProtoMessage]) -> Result<String> {
    let mut indexes = Vec::new();

    // Collect all fulltext indexes from messages
    for message in messages {
        let table_name = if message.name == "JobCreatedEvent" {
            "jobs".to_string()
        } else if message.name == "AgentSessionStartedEvent" {
            "agent_session".to_string()
        } else {
            message.name.to_snake_case()
        };

        // Find fields with search option
        for field in &message.fields {
            if field.has_search_option && field.field_type == "string" && !field.has_sequences_option {
                let column_name = field.name.to_snake_case();
                indexes.push(FulltextIndex::new(table_name.clone(), column_name));
            }
        }
    }

    // Generate Rust code with all fulltext indexes
    let mut code = String::new();

    code.push_str("// Auto-generated fulltext index metadata\n");
    code.push_str("// DO NOT EDIT - Generated from protobuf definitions\n\n");

    code.push_str("use lazy_static::lazy_static;\n\n");

    code.push_str("#[derive(Debug, Clone)]\n");
    code.push_str("pub struct FulltextIndex {\n");
    code.push_str("    pub table_name: &'static str,\n");
    code.push_str("    pub column_name: &'static str,\n");
    code.push_str("    pub index_name: &'static str,\n");
    code.push_str("}\n\n");

    code.push_str("lazy_static! {\n");
    code.push_str("    pub static ref FULLTEXT_INDEXES: Vec<FulltextIndex> = vec![\n");

    for index in &indexes {
        code.push_str(&format!(
            "        FulltextIndex {{\n            table_name: \"{}\",\n            column_name: \"{}\",\n            index_name: \"{}\",\n        }},\n",
            index.table_name, index.column_name, index.index_name
        ));
    }

    code.push_str("    ];\n");
    code.push_str("}\n\n");

    // Add helper function to get indexes by table
    code.push_str("pub fn get_indexes_for_table(table_name: &str) -> Vec<&'static FulltextIndex> {\n");
    code.push_str("    FULLTEXT_INDEXES\n");
    code.push_str("        .iter()\n");
    code.push_str("        .filter(|idx| idx.table_name == table_name)\n");
    code.push_str("        .collect()\n");
    code.push_str("}\n\n");

    // Add function to get all unique tables with fulltext indexes
    code.push_str("pub fn get_tables_with_fulltext() -> Vec<&'static str> {\n");
    code.push_str("    let mut tables: Vec<&str> = FULLTEXT_INDEXES\n");
    code.push_str("        .iter()\n");
    code.push_str("        .map(|idx| idx.table_name)\n");
    code.push_str("        .collect();\n");
    code.push_str("    tables.sort();\n");
    code.push_str("    tables.dedup();\n");
    code.push_str("    tables\n");
    code.push_str("}\n");

    Ok(code)
}

/// Generate a simple text list of all fulltext indexes for documentation
pub fn generate_fulltext_index_list(messages: &[crate::parser::ProtoMessage]) -> Result<String> {
    let mut output = String::new();
    output.push_str("# Fulltext Indexes Generated from Protobuf\n\n");

    let mut current_table = String::new();

    for message in messages {
        let table_name = if message.name == "JobCreatedEvent" {
            "jobs".to_string()
        } else if message.name == "AgentSessionStartedEvent" {
            "agent_session".to_string()
        } else {
            message.name.to_snake_case()
        };

        let mut table_indexes = Vec::new();

        // Find fields with search option
        for field in &message.fields {
            if field.has_search_option && field.field_type == "string" && !field.has_sequences_option {
                let column_name = field.name.to_snake_case();
                let index_name = format!("ft_idx_{}", column_name);
                table_indexes.push((column_name, index_name));
            }
        }

        if !table_indexes.is_empty() {
            if table_name != current_table {
                output.push_str(&format!("\n## Table: {}\n", table_name));
                current_table = table_name.clone();
            }

            for (column, index) in table_indexes {
                output.push_str(&format!(
                    "- FULLTEXT INDEX {} (`{}`) WITH PARSER STANDARD\n",
                    index, column
                ));
            }
        }
    }

    Ok(output)
}