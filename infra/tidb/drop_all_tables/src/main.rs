use anyhow::Result;
use dotenvy;
use sqlx::mysql::MySqlPoolOptions;
use std::env;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let database_url =
        env::var("DATABASE_URL").expect("DATABASE_URL environment variable must be set");

    println!("🔗 Connecting to database...");

    let pool = MySqlPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await?;

    println!("📋 Querying all tables in database...");

    // Get all tables in the database
    let tables: Vec<(String,)> = sqlx::query_as("SHOW TABLES").fetch_all(&pool).await?;

    if tables.is_empty() {
        println!("📭 No tables found in database (database is already empty)");
        return Ok(());
    }

    println!("🗑️  Found {} tables to drop:", tables.len());
    for (table_name,) in &tables {
        println!("  - {}", table_name);
    }

    println!("\n🗑️  Dropping all tables...");

    // Disable foreign key checks
    sqlx::query("SET FOREIGN_KEY_CHECKS = 0")
        .execute(&pool)
        .await?;

    // Drop each table
    for (table_name,) in &tables {
        print!("  - Dropping table: {} ... ", table_name);

        let query = format!("DROP TABLE IF EXISTS `{}`", table_name);
        match sqlx::query(&query).execute(&pool).await {
            Ok(_) => println!("✅ Dropped"),
            Err(e) => println!("❌ Failed: {}", e),
        }
    }

    // Re-enable foreign key checks
    sqlx::query("SET FOREIGN_KEY_CHECKS = 1")
        .execute(&pool)
        .await?;

    println!("✅ All tables dropped successfully");

    Ok(())
}
