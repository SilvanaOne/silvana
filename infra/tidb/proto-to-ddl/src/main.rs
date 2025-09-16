use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use dotenvy::dotenv;
use proto_to_ddl::parser::{generate_entities, generate_mysql_ddl, parse_proto_file};
use proto_to_ddl::{SchemaValidator, generate_fulltext_index_list, generate_fulltext_index_metadata};
use std::fs;
use tracing::info;

#[derive(Parser)]
#[command(name = "proto-to-ddl")]
#[command(
    about = "Generate MySQL DDL, Sea-ORM entities, and validate schemas from Protocol Buffer files"
)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate MySQL DDL and optionally Sea-ORM entities from proto file
    Generate {
        /// Path to the protobuf file
        #[arg(short, long)]
        proto_file: String,

        /// Output SQL file path
        #[arg(short, long)]
        output: String,

        /// Database name prefix for tables
        #[arg(short, long, default_value = "")]
        database: String,

        /// Generate Sea-ORM entities
        #[arg(long)]
        entities: bool,

        /// Entity output directory
        #[arg(long, default_value = "src/entity")]
        entity_dir: String,
    },
    /// Validate database schema against proto definitions
    Validate {
        /// Path to the protobuf file
        #[arg(short, long)]
        proto_file: String,

        /// Database URL (can also be set via DATABASE_URL env var)
        #[arg(short, long)]
        database_url: Option<String>,
    },
    /// List all tables in the database
    ListTables {
        /// Database URL (can also be set via DATABASE_URL env var)
        #[arg(short, long)]
        database_url: Option<String>,
    },
    /// Search a specific table using fulltext search
    Search {
        /// Database URL (can also be set via DATABASE_URL env var)
        #[arg(short, long)]
        database_url: Option<String>,

        /// Table name to search
        #[arg(short, long)]
        table: String,

        /// Field to search in
        #[arg(short, long)]
        field: String,

        /// Search query
        #[arg(short, long)]
        query: String,

        /// Limit results
        #[arg(short, long, default_value = "10")]
        limit: u32,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Load environment variables
    dotenv().ok();

    let args = Args::parse();

    match args.command {
        Commands::Generate {
            proto_file,
            output,
            database,
            entities,
            entity_dir,
        } => generate_command(proto_file, output, database, entities, entity_dir).await,
        Commands::Validate {
            proto_file,
            database_url,
        } => validate_command(proto_file, database_url).await,
        Commands::ListTables { database_url } => list_tables_command(database_url).await,
        Commands::Search {
            database_url,
            table,
            field,
            query,
            limit,
        } => search_command(database_url, table, field, query, limit).await,
    }
}

async fn generate_command(
    proto_file: String,
    output: String,
    database: String,
    entities: bool,
    entity_dir: String,
) -> Result<()> {
    let proto_content = fs::read_to_string(&proto_file)
        .with_context(|| format!("Failed to read proto file: {}", proto_file))?;

    let messages = parse_proto_file(&proto_content)?;

    // Generate DDL
    let ddl = generate_mysql_ddl(&messages, &database)?;
    fs::write(&output, ddl).with_context(|| format!("Failed to write output file: {}", output))?;

    println!("✅ Generated DDL from {} to {}", proto_file, output);

    // Generate fulltext index list
    let index_list = generate_fulltext_index_list(&messages)?;
    let index_list_path = output.replace(".sql", "_fulltext_indexes.txt");
    fs::write(&index_list_path, index_list)
        .with_context(|| format!("Failed to write fulltext index list: {}", index_list_path))?;
    println!("✅ Generated fulltext index list to {}", index_list_path);

    // Generate fulltext index Rust code
    let index_code = generate_fulltext_index_metadata(&messages)?;
    let index_code_path = "crates/rpc/src/fulltext_indexes.rs";
    fs::write(&index_code_path, index_code)
        .with_context(|| format!("Failed to write fulltext index code: {}", index_code_path))?;
    println!("✅ Generated fulltext index code to {}", index_code_path);

    // Generate entities if requested
    if entities {
        generate_entities(&messages, &entity_dir)?;
        println!("✅ Generated entities to {}", entity_dir);
    }

    Ok(())
}

async fn validate_command(proto_file: String, database_url: Option<String>) -> Result<()> {
    // Get database URL from argument or environment
    let database_url = database_url
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .ok_or_else(|| {
            anyhow::anyhow!("DATABASE_URL must be provided via --database-url argument or DATABASE_URL environment variable")
        })?;

    info!("🔍 Starting schema validation against database...");
    info!("Database: {}", mask_password(&database_url));
    info!("Proto file: {}", proto_file);

    // Create validator
    let validator = SchemaValidator::new(&database_url, &proto_file).await?;

    // Run validation
    let results = validator.validate_schema().await?;

    // Print detailed report
    validator.print_validation_report(&results);

    // Exit with appropriate code
    let has_errors = results.iter().any(|r| !r.is_valid);
    if has_errors {
        std::process::exit(1);
    } else {
        info!("🎉 Schema validation passed!");
        Ok(())
    }
}

async fn list_tables_command(database_url: Option<String>) -> Result<()> {
    use sea_orm::{ConnectionTrait, Database, Statement};

    let db_url = database_url
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .context("Database URL not provided and DATABASE_URL env var not set")?;

    // Trim any whitespace from the URL
    let db_url = db_url.trim().to_string();

    let connection = Database::connect(&db_url)
        .await
        .context("Failed to connect to database")?;

    let stmt = Statement::from_string(
        sea_orm::DatabaseBackend::MySql,
        "SHOW TABLES".to_string(),
    );

    let result = connection.query_all(stmt).await?;

    println!("Tables in database:");
    println!("------------------");
    for row in result {
        if let Ok(table_name) = row.try_get::<String>("", "Tables_in_test") {
            println!("  - {}", table_name);
        }
    }

    Ok(())
}

async fn search_command(
    database_url: Option<String>,
    table: String,
    field: String,
    query: String,
    limit: u32,
) -> Result<()> {
    use sea_orm::{ConnectionTrait, Database, Statement};

    let db_url = database_url
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .context("Database URL not provided and DATABASE_URL env var not set")?;

    let db_url = db_url.trim().to_string();

    let connection = Database::connect(&db_url)
        .await
        .context("Failed to connect to database")?;

    // Build the search query using LIKE for compatibility
    // Use LIKE instead of fulltext search since TiDB fulltext doesn't work well with hex strings
    let sql = format!(
        "SELECT *,
         CASE WHEN {} LIKE '%{}%' THEN 1.0 ELSE 0 END as relevance_score
         FROM {}
         WHERE {} LIKE '%{}%'
         ORDER BY relevance_score DESC
         LIMIT {}",
        field, query, table, field, query, limit
    );

    println!("Executing search query:");
    println!("  Table: {}", table);
    println!("  Field: {}", field);
    println!("  Query: {}", query);
    println!("  Limit: {}", limit);
    println!();

    let stmt = Statement::from_string(sea_orm::DatabaseBackend::MySql, sql);

    match connection.query_all(stmt).await {
        Ok(results) => {
            println!("Found {} results:", results.len());
            println!("{}", "=".repeat(80));

            for (idx, row) in results.iter().enumerate() {
                println!("\nResult #{}:", idx + 1);
                println!("{}", "-".repeat(40));

                // Try to get common fields
                if let Ok(id) = row.try_get::<String>("", "id") {
                    println!("  id: {}", id);
                }
                if let Ok(coordinator_id) = row.try_get::<String>("", "coordinator_id") {
                    println!("  coordinator_id: {}", coordinator_id);
                }
                if let Ok(session_id) = row.try_get::<String>("", "session_id") {
                    println!("  session_id: {}", session_id);
                }
                if let Ok(job_id) = row.try_get::<String>("", "job_id") {
                    println!("  job_id: {}", job_id);
                }
                if let Ok(message) = row.try_get::<String>("", "message") {
                    let truncated = if message.len() > 200 {
                        format!("{}...", &message[..200])
                    } else {
                        message
                    };
                    println!("  message: {}", truncated);
                }
                if let Ok(timestamp) = row.try_get::<i64>("", "event_timestamp") {
                    println!("  event_timestamp: {}", timestamp);
                }
                if let Ok(score) = row.try_get::<f64>("", "relevance_score") {
                    println!("  relevance_score: {:.4}", score);
                }
            }

            if results.is_empty() {
                println!("No matching records found.");
            }
        }
        Err(e) => {
            eprintln!("Search failed: {}", e);
            eprintln!("\nPossible issues:");
            eprintln!("  - Table '{}' might not exist", table);
            eprintln!("  - Field '{}' might not have a fulltext index", field);
            eprintln!("  - Syntax error in search query");
            return Err(e.into());
        }
    }

    Ok(())
}

/// Mask password in database URL for safe logging
fn mask_password(url: &str) -> String {
    if let Some(at_pos) = url.find('@') {
        if let Some(colon_pos) = url[..at_pos].rfind(':') {
            let mut masked = url.to_string();
            let password_start = colon_pos + 1;
            let password_end = at_pos;
            masked.replace_range(password_start..password_end, "***");
            return masked;
        }
    }
    url.to_string()
}
