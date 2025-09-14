use anyhow::{Context, Result};
use inflector::Inflector;
use sea_orm::{ConnectionTrait, Database, DatabaseConnection, Statement};
use std::collections::{HashMap, HashSet};
use tracing::info;

use crate::parser::{parse_proto_file, proto_type_to_mysql_with_options};

#[derive(Debug, Clone)] //, Serialize, Deserialize
pub struct TableInfo {
    pub table_name: String,
    pub columns: Vec<ColumnInfo>,
    pub indexes: Vec<IndexInfo>,
}

#[derive(Debug, Clone)] // , Serialize, Deserialize
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub is_nullable: bool,
    pub is_primary_key: bool,
    pub default_value: Option<String>,
    pub character_maximum_length: Option<u64>,
}

#[derive(Debug, Clone)] //, Serialize, Deserialize
pub struct IndexInfo {
    pub name: String,
    pub column_name: String,
    pub is_unique: bool,
    pub is_primary: bool,
}

#[derive(Debug, Clone)] //, Serialize, Deserialize
pub struct SchemaValidationResult {
    pub table_name: String,
    pub discrepancies: Vec<SchemaDiscrepancy>,
    pub is_valid: bool,
}

#[derive(Debug, Clone)] //, Serialize, Deserialize
pub enum SchemaDiscrepancy {
    MissingTable,
    ExtraTable,
    MissingColumn {
        column: String,
    },
    ExtraColumn {
        column: String,
    },
    TypeMismatch {
        column: String,
        expected: String,
        actual: String,
    },
    NullabilityMismatch {
        column: String,
        expected: bool,
        actual: bool,
    },
    MissingIndex {
        index: String,
    },
    ExtraIndex {
        index: String,
    },
}

#[derive(Debug, Clone)]
struct ExpectedColumn {
    name: String,
    data_type: String,
    is_nullable: bool,
}

pub struct SchemaValidator {
    connection: DatabaseConnection,
    database_name: String,
    proto_file_path: String,
}

impl SchemaValidator {
    pub async fn new(database_url: &str, proto_file_path: &str) -> Result<Self> {
        // Handle potential whitespace in URL
        let database_url = database_url.trim();

        let connection = Database::connect(database_url)
            .await
            .context("Failed to connect to database")?;
        let database_name = database_url
            .split('/')
            .last()
            .unwrap_or("unknown")
            .split('?')
            .next()
            .unwrap_or("unknown")
            .to_string();

        Ok(Self {
            connection,
            database_name,
            proto_file_path: proto_file_path.to_string(),
        })
    }

    /// Get all tables that match the pattern *_event (our proto-generated tables)
    pub async fn get_event_tables(&self) -> Result<Vec<String>> {
        let query = r#"
            SELECT TABLE_NAME
            FROM INFORMATION_SCHEMA.TABLES
            WHERE TABLE_SCHEMA = ?
            AND (TABLE_NAME LIKE '%_event'
                OR TABLE_NAME LIKE '%_event_ms1'
                OR TABLE_NAME LIKE '%_event_ms2'
                OR TABLE_NAME LIKE '%_event_sequences')
            ORDER BY TABLE_NAME
        "#;

        let stmt = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::MySql,
            query,
            vec![self.database_name.clone().into()],
        );

        let result = self.connection.query_all(stmt).await?;
        let tables: Vec<String> = result
            .into_iter()
            .map(|row| row.try_get::<String>("", "TABLE_NAME").unwrap())
            .collect();

        Ok(tables)
    }

    /// Get detailed information about a specific table
    pub async fn get_table_info(&self, table_name: &str) -> Result<TableInfo> {
        let columns = self.get_table_columns(table_name).await?;
        let indexes = self.get_table_indexes(table_name).await?;

        Ok(TableInfo {
            table_name: table_name.to_string(),
            columns,
            indexes,
        })
    }

    async fn get_table_columns(&self, table_name: &str) -> Result<Vec<ColumnInfo>> {
        let query = r#"
            SELECT 
                COLUMN_NAME,
                DATA_TYPE,
                IS_NULLABLE,
                COLUMN_KEY,
                COLUMN_DEFAULT,
                CHARACTER_MAXIMUM_LENGTH
            FROM INFORMATION_SCHEMA.COLUMNS 
            WHERE TABLE_SCHEMA = ? 
            AND TABLE_NAME = ?
            ORDER BY ORDINAL_POSITION
        "#;

        let stmt = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::MySql,
            query,
            vec![self.database_name.clone().into(), table_name.into()],
        );

        let result = self.connection.query_all(stmt).await?;
        let columns: Vec<ColumnInfo> = result
            .into_iter()
            .map(|row| {
                let column_name: String = row.try_get("", "COLUMN_NAME").unwrap();
                let data_type: String = row.try_get("", "DATA_TYPE").unwrap();
                let is_nullable: String = row.try_get("", "IS_NULLABLE").unwrap();
                let column_key: Option<String> = row.try_get("", "COLUMN_KEY").ok();
                let default_value: Option<String> = row.try_get("", "COLUMN_DEFAULT").ok();
                let char_max_length: Option<u64> = row.try_get("", "CHARACTER_MAXIMUM_LENGTH").ok();

                ColumnInfo {
                    name: column_name,
                    data_type,
                    is_nullable: is_nullable == "YES",
                    is_primary_key: column_key.as_deref() == Some("PRI"),
                    default_value,
                    character_maximum_length: char_max_length,
                }
            })
            .collect();

        Ok(columns)
    }

    async fn get_table_indexes(&self, table_name: &str) -> Result<Vec<IndexInfo>> {
        let query = r#"
            SELECT 
                INDEX_NAME,
                COLUMN_NAME,
                NON_UNIQUE
            FROM INFORMATION_SCHEMA.STATISTICS 
            WHERE TABLE_SCHEMA = ? 
            AND TABLE_NAME = ?
            ORDER BY INDEX_NAME, SEQ_IN_INDEX
        "#;

        let stmt = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::MySql,
            query,
            vec![self.database_name.clone().into(), table_name.into()],
        );

        let result = self.connection.query_all(stmt).await?;
        let indexes: Vec<IndexInfo> = result
            .into_iter()
            .map(|row| {
                let index_name: String = row.try_get("", "INDEX_NAME").unwrap();
                let column_name: String = row.try_get("", "COLUMN_NAME").unwrap();
                // TiDB returns NON_UNIQUE as a string, not an integer
                let non_unique_str: String = row
                    .try_get("", "NON_UNIQUE")
                    .unwrap_or_else(|_| "1".to_string());
                let non_unique = non_unique_str.parse::<i32>().unwrap_or(1);

                IndexInfo {
                    name: index_name.clone(),
                    column_name,
                    is_unique: non_unique == 0,
                    is_primary: index_name == "PRIMARY",
                }
            })
            .collect();

        Ok(indexes)
    }

    /// Validate that database schema matches expected proto-generated schema
    pub async fn validate_schema(&self) -> Result<Vec<SchemaValidationResult>> {
        info!("🔍 Starting schema validation...");

        let expected_schema = self.get_expected_schema_from_proto()?;
        let actual_tables = self.get_event_tables().await?;

        let mut results = Vec::new();

        // Check each expected table
        for (expected_table_name, expected_columns) in &expected_schema {
            let mut discrepancies = Vec::new();

            if !actual_tables.contains(expected_table_name) {
                discrepancies.push(SchemaDiscrepancy::MissingTable);
                results.push(SchemaValidationResult {
                    table_name: expected_table_name.clone(),
                    discrepancies,
                    is_valid: false,
                });
                continue;
            }

            // Get actual table info
            let table_info = self.get_table_info(expected_table_name).await?;

            // Validate columns
            self.validate_columns(&table_info.columns, expected_columns, &mut discrepancies);

            // Validate indexes
            let expected_indexes =
                self.get_expected_indexes_for_table(expected_table_name, expected_columns)?;
            self.validate_indexes(&table_info.indexes, &expected_indexes, &mut discrepancies);

            let is_valid = discrepancies.is_empty();
            results.push(SchemaValidationResult {
                table_name: expected_table_name.clone(),
                discrepancies,
                is_valid,
            });
        }

        // Check for extra tables
        for actual_table in &actual_tables {
            if !expected_schema.contains_key(actual_table) {
                results.push(SchemaValidationResult {
                    table_name: actual_table.clone(),
                    discrepancies: vec![SchemaDiscrepancy::ExtraTable],
                    is_valid: false,
                });
            }
        }

        info!("✅ Schema validation completed");
        Ok(results)
    }

    /// Parse the proto file and generate expected schema
    fn get_expected_schema_from_proto(&self) -> Result<HashMap<String, Vec<ExpectedColumn>>> {
        let proto_content = std::fs::read_to_string(&self.proto_file_path)
            .with_context(|| format!("Failed to read proto file: {}", self.proto_file_path))?;

        let messages = parse_proto_file(&proto_content)?;

        let mut schema = HashMap::new();

        for message in messages {
            let table_name = message.name.to_snake_case();
            let mut columns = vec![
                // Standard columns present in all tables
                ExpectedColumn {
                    name: "id".to_string(),
                    data_type: "bigint".to_string(),
                    is_nullable: false,
                },
                ExpectedColumn {
                    name: "created_at".to_string(),
                    data_type: "timestamp".to_string(),
                    is_nullable: true,
                },
                ExpectedColumn {
                    name: "updated_at".to_string(),
                    data_type: "timestamp".to_string(),
                    is_nullable: true,
                },
            ];

            // Track fields with sequences option for child table creation
            let mut sequence_fields = Vec::new();

            // Add proto-defined fields
            for field in message.fields.clone() {
                // Skip fields with sequences option - they become separate child tables
                if field.has_sequences_option {
                    sequence_fields.push(field);
                    continue;
                }

                columns.push(ExpectedColumn {
                    name: field.name.to_snake_case(),
                    data_type: proto_type_to_mysql_with_options(
                        &field.field_type,
                        field.is_repeated,
                        field.has_search_option,
                        field.name.contains("metadata") || field.name.contains("message"),
                    ),
                    is_nullable: field.is_optional || field.is_repeated,
                });
            }

            schema.insert(table_name.clone(), columns);

            // Add child tables for fields with sequences option
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
                let parent_fk = format!("{}_id", table_name);
                let value_col = field.name.to_singular().to_snake_case();

                let child_columns = vec![
                    ExpectedColumn {
                        name: "id".to_string(),
                        data_type: "bigint".to_string(),
                        is_nullable: false,
                    },
                    ExpectedColumn {
                        name: parent_fk,
                        data_type: "bigint".to_string(),
                        is_nullable: false,
                    },
                    ExpectedColumn {
                        name: value_col,
                        data_type: proto_type_to_mysql_with_options(
                            &field.field_type,
                            false,  // Not repeated in child table
                            false,
                            field.name.contains("metadata") || field.name.contains("message"),
                        ),
                        is_nullable: false,
                    },
                    ExpectedColumn {
                        name: "created_at".to_string(),
                        data_type: "timestamp".to_string(),
                        is_nullable: true,
                    },
                    ExpectedColumn {
                        name: "updated_at".to_string(),
                        data_type: "timestamp".to_string(),
                        is_nullable: true,
                    },
                ];

                schema.insert(child_table_name, child_columns);
            }
        }

        Ok(schema)
    }

    fn validate_columns(
        &self,
        actual_columns: &[ColumnInfo],
        expected_columns: &[ExpectedColumn],
        discrepancies: &mut Vec<SchemaDiscrepancy>,
    ) {
        let actual_columns_map: HashMap<String, &ColumnInfo> = actual_columns
            .iter()
            .map(|col| (col.name.clone(), col))
            .collect();

        let expected_columns_set: HashSet<String> = expected_columns
            .iter()
            .map(|col| col.name.clone())
            .collect();

        // Check for missing columns
        for expected in expected_columns {
            if let Some(actual) = actual_columns_map.get(&expected.name) {
                // Column exists, check type compatibility
                if !self.is_type_compatible(&expected.data_type, &actual.data_type) {
                    discrepancies.push(SchemaDiscrepancy::TypeMismatch {
                        column: expected.name.clone(),
                        expected: expected.data_type.clone(),
                        actual: actual.data_type.clone(),
                    });
                }

                // Check nullability
                if expected.is_nullable != actual.is_nullable {
                    discrepancies.push(SchemaDiscrepancy::NullabilityMismatch {
                        column: expected.name.clone(),
                        expected: expected.is_nullable,
                        actual: actual.is_nullable,
                    });
                }
            } else {
                discrepancies.push(SchemaDiscrepancy::MissingColumn {
                    column: expected.name.clone(),
                });
            }
        }

        // Check for extra columns
        for actual in actual_columns {
            if !expected_columns_set.contains(&actual.name) {
                discrepancies.push(SchemaDiscrepancy::ExtraColumn {
                    column: actual.name.clone(),
                });
            }
        }
    }

    fn validate_indexes(
        &self,
        actual_indexes: &[IndexInfo],
        expected_indexes: &[String],
        discrepancies: &mut Vec<SchemaDiscrepancy>,
    ) {
        let actual_index_names: HashSet<String> =
            actual_indexes.iter().map(|idx| idx.name.clone()).collect();

        let expected_index_set: HashSet<String> = expected_indexes.iter().cloned().collect();

        // Check for missing indexes
        for expected in expected_indexes {
            if !actual_index_names.contains(expected) {
                discrepancies.push(SchemaDiscrepancy::MissingIndex {
                    index: expected.clone(),
                });
            }
        }

        // Check for extra indexes (excluding PRIMARY which is always present and fulltext indexes)
        for actual in actual_indexes {
            // Skip PRIMARY index and fulltext indexes (ft_idx_*) as they're not defined in proto
            if actual.name == "PRIMARY" || actual.name.starts_with("ft_idx_") {
                continue;
            }

            if !expected_index_set.contains(&actual.name) {
                discrepancies.push(SchemaDiscrepancy::ExtraIndex {
                    index: actual.name.clone(),
                });
            }
        }
    }

    fn is_type_compatible(&self, expected: &str, actual: &str) -> bool {
        // Map common type variations
        let normalize_type = |t: &str| -> String {
            let t = t.to_lowercase();
            match t.as_str() {
                "varchar" | "varchar(255)" => "varchar".to_string(),
                "text" => "text".to_string(),
                "bigint" | "bigint unsigned" => "bigint".to_string(),
                "int" | "integer" => "int".to_string(),
                "timestamp" | "datetime" => "timestamp".to_string(),
                "json" => "json".to_string(),
                _ => t,
            }
        };

        normalize_type(expected) == normalize_type(actual)
    }

    /// Get expected indexes for a specific table based on its fields
    /// This matches the logic in parser.rs for generating indexes
    fn get_expected_indexes_for_table(
        &self,
        table_name: &str,
        expected_columns: &[ExpectedColumn],
    ) -> Result<Vec<String>> {
        // Check if this is a child table (ends with _ms1, _ms2, or _sequences)
        if table_name.ends_with("_ms1") || table_name.ends_with("_ms2") || table_name.ends_with("_sequences") {
            // Child tables have different index naming convention
            let mut indexes = vec![];

            // Add parent index (e.g., idx_job_created_event_ms1_parent)
            indexes.push(format!("idx_{}_parent", table_name));

            // Add value index (e.g., idx_job_created_event_ms1_value)
            indexes.push(format!("idx_{}_value", table_name));

            return Ok(indexes);
        }

        // Main tables use the regular pattern
        let mut indexes = vec!["idx_created_at".to_string()];

        // Generate indexes based on field patterns (same logic as in parser.rs)
        for column in expected_columns {
            let column_name = &column.name;

            // Add index for fields containing "id" (but not "id" itself)
            if column_name.contains("id") && column_name != "id" {
                indexes.push(format!("idx_{}", column_name));
            }

            // Add index for fields containing "timestamp"
            if column_name.contains("timestamp") {
                indexes.push(format!("idx_{}", column_name));
            }

            // Add index for fields containing "hash"
            if column_name.contains("hash") {
                indexes.push(format!("idx_{}", column_name));
            }
        }

        Ok(indexes)
    }

    /// Print a detailed validation report
    pub fn print_validation_report(&self, results: &[SchemaValidationResult]) {
        let mut total_issues = 0;
        let mut valid_tables = 0;

        println!("\n📊 Schema Validation Report");
        println!("==========================\n");

        for result in results {
            if result.is_valid {
                valid_tables += 1;
                println!("✅ {} - Valid", result.table_name);
            } else {
                total_issues += result.discrepancies.len();
                println!(
                    "❌ {} - {} issue(s)",
                    result.table_name,
                    result.discrepancies.len()
                );

                for discrepancy in &result.discrepancies {
                    match discrepancy {
                        SchemaDiscrepancy::MissingTable => {
                            println!("   🚫 Table missing from database");
                        }
                        SchemaDiscrepancy::ExtraTable => {
                            println!("   ➕ Extra table in database (not in proto)");
                        }
                        SchemaDiscrepancy::MissingColumn { column } => {
                            println!("   🚫 Missing column: {}", column);
                        }
                        SchemaDiscrepancy::ExtraColumn { column } => {
                            println!("   ➕ Extra column: {}", column);
                        }
                        SchemaDiscrepancy::TypeMismatch {
                            column,
                            expected,
                            actual,
                        } => {
                            println!(
                                "   🔄 Type mismatch in {}: expected '{}', got '{}'",
                                column, expected, actual
                            );
                        }
                        SchemaDiscrepancy::NullabilityMismatch {
                            column,
                            expected,
                            actual,
                        } => {
                            println!(
                                "   🔄 Nullability mismatch in {}: expected nullable={}, got nullable={}",
                                column, expected, actual
                            );
                        }
                        SchemaDiscrepancy::MissingIndex { index } => {
                            println!("   🚫 Missing index: {}", index);
                        }
                        SchemaDiscrepancy::ExtraIndex { index } => {
                            println!("   ➕ Extra index: {}", index);
                        }
                    }
                }
                println!();
            }
        }

        println!("📈 Summary:");
        println!("  Valid tables: {}/{}", valid_tables, results.len());
        println!("  Total issues: {}", total_issues);

        if total_issues == 0 {
            println!("\n🎉 All tables are valid! Proto definitions match database schema.");
        } else {
            println!(
                "\n⚠️  Schema discrepancies found. Consider running 'proto-to-ddl' to regenerate schema."
            );
        }
    }
}
