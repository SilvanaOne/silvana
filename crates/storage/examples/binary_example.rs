use anyhow::Result;
use storage::s3::S3Client;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize S3 client
    let s3_client = S3Client::new("your-bucket-name".to_string()).await?;
    
    // Example 1: Write binary data without expected hash
    let data = vec![0x48, 0x65, 0x6C, 0x6C, 0x6F]; // "Hello" in bytes
    let file_name = "test-file.bin";
    let mime_type = "application/octet-stream";
    
    println!("Writing binary data to S3...");
    let stored_hash = s3_client
        .write_binary(data.clone(), file_name, mime_type, None, None)
        .await?;
    println!("File stored with SHA256: {}", stored_hash);

    // Example 2: Write binary data with expected hash verification
    let expected_hash = stored_hash.clone(); // Use the hash from previous write
    let result = s3_client
        .write_binary(data.clone(), "test-file-2.bin", mime_type, None, Some(expected_hash))
        .await;
    
    match result {
        Ok(hash) => println!("File stored with verified SHA256: {}", hash),
        Err(e) => println!("Hash verification failed: {}", e),
    }
    
    // Example 3: Read binary data back
    println!("\nReading binary data from S3...");
    let result = s3_client.read_binary(file_name).await?;
    println!("Read {} bytes with SHA256: {}", result.data.len(), result.sha256);

    // Verify the data matches
    assert_eq!(data, result.data);
    assert_eq!(stored_hash, result.sha256);
    println!("Data integrity verified!");

    // Example 4: Working with image files
    let image_data = std::fs::read("path/to/image.png")?;
    let image_hash = s3_client
        .write_binary(image_data, "images/test.png", "image/png", None, None)
        .await?;
    println!("Image stored with SHA256: {}", image_hash);

    // Read the image back
    let image_result = s3_client.read_binary("images/test.png").await?;
    std::fs::write("downloaded_image.png", image_result.data)?;
    println!("Image downloaded and saved locally with SHA256: {}", image_result.sha256);
    
    Ok(())
}