use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use dotenvy::dotenv;
use proto_to_ddl::parser::{generate_entities, generate_mysql_ddl, parse_proto_file};
use proto_to_ddl::SchemaValidator;
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
