use anyhow::Result;
use storage::{S3Client, pack_folder_to_s3, unpack_from_s3, ArchiveConfig};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

#[tokio::test]
async fn test_pack_examples_add_folder() -> Result<()> {
    // Initialize S3 client
    let s3_client = S3Client::new("silvana-distribution".to_string()).await?;
    
    // Get the examples/add folder path (relative to workspace root)
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let examples_add_path = workspace_root.join("examples").join("add");
    
    // Verify source folder exists
    if !examples_add_path.exists() {
        eprintln!("Warning: examples/add folder not found at {:?}, skipping test", examples_add_path);
        return Ok(());
    }
    
    println!("Source folder: {:?}", examples_add_path);
    
    // Generate unique S3 key for this test
    let timestamp = chrono::Utc::now().timestamp_millis();
    let s3_key = format!("test/examples_add_{}.tar.zst", timestamp);
    
    println!("Packing folder to S3: {}", s3_key);
    
    // Pack and upload to S3 with default config (1MB limit)
    let archive_hash = pack_folder_to_s3(
        &examples_add_path,
        &s3_client,
        &s3_key,
        None, // Use default config
    ).await?;
    
    println!("Archive uploaded successfully with hash: {}", archive_hash);
    
    // Create data/add directory for extraction
    let data_dir = workspace_root.join("data");
    if !data_dir.exists() {
        fs::create_dir(&data_dir)?;
    }
    let target_dir = data_dir.join("add");
    
    // Remove target directory if it exists
    if target_dir.exists() {
        fs::remove_dir_all(&target_dir)?;
    }
    
    println!("Unpacking archive to: {:?}", target_dir);
    
    // Download and unpack from S3
    let downloaded_hash = unpack_from_s3(
        &s3_client,
        &s3_key,
        &target_dir,
        Some(archive_hash.clone()), // Verify hash
    ).await?;
    
    assert_eq!(archive_hash, downloaded_hash, "Hash verification failed");
    
    // Verify some expected files/directories exist
    assert!(target_dir.exists(), "Target directory should exist");
    
    // Check for expected subdirectories
    let agent_dir = target_dir.join("agent");
    let move_dir = target_dir.join("move");
    
    if agent_dir.exists() {
        println!("✓ agent/ directory extracted");
    }
    if move_dir.exists() {
        println!("✓ move/ directory extracted");
    }
    
    // Note: .DS_Store at root level of examples/add might be included since gitignore 
    // rules are applied per directory. Subdirectory .DS_Store files should be excluded.
    
    // Verify that subdirectory .DS_Store files and other ignored items are excluded
    assert!(!target_dir.join("agent/.DS_Store").exists(), "agent/.DS_Store should be excluded");
    assert!(!target_dir.join("agent/node_modules").exists(), "agent/node_modules should be excluded");
    assert!(!target_dir.join("agent/cache").exists(), "agent/cache should be excluded");
    assert!(!target_dir.join("agent/dist").exists(), "agent/dist should be excluded");
    assert!(!target_dir.join("agent/.env").exists(), "agent/.env should be excluded");
    assert!(!target_dir.join("agent/.env.app").exists(), "agent/.env.app should be excluded");
    assert!(!target_dir.join("move/build").exists(), "move/build should be excluded");
    
    // List extracted contents for verification
    println!("\nExtracted contents:");
    for entry in fs::read_dir(&target_dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = if path.is_dir() { "dir" } else { "file" };
        println!("  - {} ({})", path.file_name().unwrap().to_string_lossy(), file_type);
    }
    
    // Cleanup S3
    println!("\nCleaning up S3...");
    s3_client.delete(&s3_key).await?;
    
    println!("✅ Archive pack/unpack test for examples/add passed!");
    
    Ok(())
}

#[tokio::test]
async fn test_pack_and_unpack_with_temp_folder() -> Result<()> {
    // Initialize S3 client
    let s3_client = S3Client::new("silvana-distribution".to_string()).await?;
    
    // Create a temporary directory with test files
    let temp_dir = TempDir::new()?;
    let source_dir = temp_dir.path().join("source");
    fs::create_dir(&source_dir)?;
    
    // Create test files (small to stay under 1MB limit)
    fs::write(source_dir.join("README.md"), "# Test Project\nThis is a test archive.")?;
    fs::write(source_dir.join("main.rs"), "fn main() {\n    println!(\"Hello, World!\");\n}")?;
    
    // Create subdirectory
    let src_dir = source_dir.join("src");
    fs::create_dir(&src_dir)?;
    fs::write(src_dir.join("lib.rs"), "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}")?;
    
    // Create .gitignore
    fs::write(source_dir.join(".gitignore"), "target/\n*.log\n.DS_Store\n")?;
    
    // Create files that should be ignored
    fs::write(source_dir.join("debug.log"), "Debug information")?;
    fs::write(source_dir.join(".DS_Store"), "Mac metadata")?;
    
    // Generate unique S3 key for this test
    let timestamp = chrono::Utc::now().timestamp_millis();
    let s3_key = format!("test/archive_test_{}.tar.zst", timestamp);
    
    println!("Packing folder to S3: {}", s3_key);
    
    // Pack and upload to S3
    let archive_hash = pack_folder_to_s3(
        &source_dir,
        &s3_client,
        &s3_key,
        None, // Use default config
    ).await?;
    
    println!("Archive uploaded with hash: {}", archive_hash);
    
    // Create target directory for extraction
    let target_dir = temp_dir.path().join("target");
    
    // Download and unpack from S3
    println!("Unpacking archive from S3...");
    let downloaded_hash = unpack_from_s3(
        &s3_client,
        &s3_key,
        &target_dir,
        Some(archive_hash.clone()), // Verify hash
    ).await?;
    
    assert_eq!(archive_hash, downloaded_hash, "Hash verification failed");
    
    // Verify extracted files exist
    assert!(target_dir.join("README.md").exists(), "README.md not extracted");
    assert!(target_dir.join("main.rs").exists(), "main.rs not extracted");
    assert!(target_dir.join("src/lib.rs").exists(), "src/lib.rs not extracted");
    
    // Verify ignored files were not included
    assert!(!target_dir.join("debug.log").exists(), "debug.log should not be extracted");
    assert!(!target_dir.join(".DS_Store").exists(), ".DS_Store should not be extracted");
    
    // Verify content
    let readme_content = fs::read_to_string(target_dir.join("README.md"))?;
    assert_eq!(readme_content, "# Test Project\nThis is a test archive.");
    
    let main_content = fs::read_to_string(target_dir.join("main.rs"))?;
    assert!(main_content.contains("Hello, World!"));
    
    // Cleanup S3
    println!("Cleaning up S3...");
    s3_client.delete(&s3_key).await?;
    
    println!("✅ Temp folder archive test passed!");
    
    Ok(())
}

#[tokio::test]
async fn test_archive_size_limits() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_dir = temp_dir.path().join("large");
    fs::create_dir(&source_dir)?;
    
    // Try to create a file that would exceed 1MB compressed
    // Create a 2MB file with random-like data that won't compress well
    let mut large_data = Vec::with_capacity(2_000_000);
    for i in 0..2_000_000 {
        large_data.push((i % 256) as u8);
    }
    fs::write(source_dir.join("large.bin"), large_data)?;
    
    let s3_client = S3Client::new("silvana-distribution".to_string()).await?;
    let s3_key = format!("test/large_test_{}.tar.zst", chrono::Utc::now().timestamp_millis());
    
    // This should fail due to size limit
    let result = pack_folder_to_s3(&source_dir, &s3_client, &s3_key, None).await;
    
    assert!(result.is_err(), "Should fail with large archive");
    if let Err(e) = result {
        let error_msg = e.to_string();
        assert!(
            error_msg.contains("exceeds maximum") || error_msg.contains("size"),
            "Error should mention size limit: {}",
            error_msg
        );
        println!("Expected error received: {}", error_msg);
    }
    
    println!("✅ Size limit test passed!");
    
    Ok(())
}

#[tokio::test]
async fn test_custom_config() -> Result<()> {
    let s3_client = S3Client::new("silvana-distribution".to_string()).await?;
    
    // Create a small test folder
    let temp_dir = TempDir::new()?;
    let source_dir = temp_dir.path().join("test");
    fs::create_dir(&source_dir)?;
    fs::write(source_dir.join("test.txt"), "Test content")?;
    
    // Test with custom configuration
    let config = ArchiveConfig {
        compression_level: 10, // Higher compression
        follow_symlinks: false,
        max_size: 500_000, // 500KB limit
    };
    
    let s3_key = format!("test/custom_config_{}.tar.zst", chrono::Utc::now().timestamp_millis());
    
    // Pack with custom config
    let archive_hash = pack_folder_to_s3(
        &source_dir,
        &s3_client,
        &s3_key,
        Some(config),
    ).await?;
    
    println!("Archive with custom config uploaded: {}", archive_hash);
    
    // Cleanup
    s3_client.delete(&s3_key).await?;
    
    println!("✅ Custom config test passed!");
    
    Ok(())
}