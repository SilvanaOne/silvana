use anyhow::Result;
use storage::s3::S3Client;
use sha2::{Sha256, Digest};

#[tokio::test]
async fn test_write_and_read_binary() -> Result<()> {
    // Initialize S3 client with silvana-distribution bucket
    let s3_client = S3Client::new("silvana-distribution".to_string()).await?;
    
    // Create test binary data
    let test_data = b"This is a test binary file content for S3 storage testing.";
    let data = test_data.to_vec();
    
    // Calculate expected hash
    let mut hasher = Sha256::new();
    hasher.update(&data);
    let expected_hash = format!("{:x}", hasher.finalize());
    
    // Test file name with timestamp to avoid conflicts
    let timestamp = chrono::Utc::now().timestamp_millis();
    let file_name = format!("test/binary_test_{}.bin", timestamp);
    
    // Test 1: Write binary data without expected hash
    println!("Test 1: Writing binary data without hash verification...");
    let stored_hash = s3_client
        .write_binary(data.clone(), &file_name, "application/octet-stream", None, None)
        .await?;

    assert_eq!(stored_hash, expected_hash, "Stored hash should match calculated hash");
    println!("✓ File written successfully with SHA256: {}", stored_hash);

    // Test 2: Read binary data back
    println!("\nTest 2: Reading binary data back...");
    let result = s3_client.read_binary(&file_name).await?;

    assert_eq!(result.data, data, "Read data should match original data");
    assert_eq!(result.sha256, expected_hash, "Read hash should match expected hash");
    println!("✓ Data read successfully and integrity verified");
    
    // Test 3: Write with correct expected hash
    let file_name_2 = format!("test/binary_test_{}_verified.bin", timestamp);
    println!("\nTest 3: Writing with correct hash verification...");
    let verified_hash = s3_client
        .write_binary(
            data.clone(),
            &file_name_2,
            "application/octet-stream",
            None,
            Some(expected_hash.clone())
        )
        .await?;

    assert_eq!(verified_hash, expected_hash, "Verified hash should match expected");
    println!("✓ File written with hash verification successful");

    // Test 4: Attempt write with incorrect expected hash (should fail)
    let file_name_3 = format!("test/binary_test_{}_wrong.bin", timestamp);
    let wrong_hash = "0000000000000000000000000000000000000000000000000000000000000000".to_string();
    println!("\nTest 4: Testing hash mismatch detection...");

    let result = s3_client
        .write_binary(
            data.clone(),
            &file_name_3,
            "application/octet-stream",
            None,
            Some(wrong_hash)
        )
        .await;
    
    assert!(result.is_err(), "Should fail with wrong hash");
    if let Err(e) = result {
        assert!(e.to_string().contains("SHA256 hash mismatch"), "Error should mention hash mismatch");
        println!("✓ Hash mismatch correctly detected: {}", e);
    }
    
    // Test 5: Large binary data
    println!("\nTest 5: Testing with larger binary data...");
    let large_data: Vec<u8> = (0..1024 * 100).map(|i| (i % 256) as u8).collect(); // 100KB
    let file_name_large = format!("test/binary_test_{}_large.bin", timestamp);

    let large_hash = s3_client
        .write_binary(large_data.clone(), &file_name_large, "application/octet-stream", None, None)
        .await?;

    let large_result = s3_client.read_binary(&file_name_large).await?;
    assert_eq!(large_result.data.len(), large_data.len(), "Large data size should match");
    assert_eq!(large_result.sha256, large_hash, "Large data hash should match");
    println!("✓ Large binary data ({}KB) handled successfully", large_data.len() / 1024);
    
    // Cleanup: Delete test files
    println!("\nCleaning up test files...");
    s3_client.delete(&file_name).await?;
    s3_client.delete(&file_name_2).await?;
    s3_client.delete(&file_name_large).await?;
    // file_name_3 was never created due to hash mismatch
    println!("✓ Test files cleaned up");
    
    println!("\n✅ All S3 binary tests passed!");
    Ok(())
}

#[tokio::test]
async fn test_binary_with_different_mime_types() -> Result<()> {
    let s3_client = S3Client::new("silvana-distribution".to_string()).await?;
    
    let timestamp = chrono::Utc::now().timestamp_millis();
    
    // Test different MIME types
    let test_cases = vec![
        ("image/png", b"PNG_DATA_MOCK".to_vec(), "test_image.png"),
        ("image/jpeg", b"JPEG_DATA_MOCK".to_vec(), "test_image.jpg"),
        ("application/pdf", b"PDF_DATA_MOCK".to_vec(), "test_doc.pdf"),
        ("application/json", b"{\"test\": \"data\"}".to_vec(), "test_data.json"),
        ("text/plain", b"Plain text content".to_vec(), "test_text.txt"),
    ];
    
    for (mime_type, data, suffix) in test_cases {
        let file_name = format!("test/mime_test_{}_{}", timestamp, suffix);
        println!("Testing MIME type: {} with file: {}", mime_type, file_name);
        
        // Write with specific MIME type
        let hash = s3_client
            .write_binary(data.clone(), &file_name, mime_type, None, None)
            .await?;

        // Read back and verify
        let result = s3_client.read_binary(&file_name).await?;
        assert_eq!(result.data, data, "Data should match for MIME type {}", mime_type);
        assert_eq!(result.sha256, hash, "Hash should match for MIME type {}", mime_type);
        
        // Cleanup
        s3_client.delete(&file_name).await?;
        println!("✓ MIME type {} tested successfully", mime_type);
    }
    
    println!("\n✅ All MIME type tests passed!");
    Ok(())
}

#[tokio::test]
async fn test_binary_edge_cases() -> Result<()> {
    let s3_client = S3Client::new("silvana-distribution".to_string()).await?;
    
    let timestamp = chrono::Utc::now().timestamp_millis();
    
    // Test 1: Empty file
    println!("Test 1: Empty binary data...");
    let empty_data = Vec::new();
    let file_name_empty = format!("test/binary_test_{}_empty.bin", timestamp);

    let empty_hash = s3_client
        .write_binary(empty_data.clone(), &file_name_empty, "application/octet-stream", None, None)
        .await?;

    let empty_result = s3_client.read_binary(&file_name_empty).await?;
    assert_eq!(empty_result.data.len(), 0, "Empty file should have 0 bytes");
    assert_eq!(empty_result.sha256, empty_hash, "Empty file hash should match");
    s3_client.delete(&file_name_empty).await?;
    println!("✓ Empty file handled correctly");
    
    // Test 2: Binary data with special bytes
    println!("\nTest 2: Binary data with special bytes...");
    let special_data: Vec<u8> = vec![0x00, 0xFF, 0x01, 0xFE, 0x7F, 0x80, 0xAA, 0x55];
    let file_name_special = format!("test/binary_test_{}_special.bin", timestamp);

    let special_hash = s3_client
        .write_binary(special_data.clone(), &file_name_special, "application/octet-stream", None, None)
        .await?;

    let special_result = s3_client.read_binary(&file_name_special).await?;
    assert_eq!(special_result.data, special_data, "Special bytes should be preserved");
    assert_eq!(special_result.sha256, special_hash, "Special bytes hash should match");
    s3_client.delete(&file_name_special).await?;
    println!("✓ Special bytes handled correctly");
    
    // Test 3: File name with special characters
    println!("\nTest 3: File name with special characters...");
    let complex_data = b"test".to_vec();
    let file_name_complex = format!("test/binary-test_{}_file+name=test.bin", timestamp);

    let complex_hash = s3_client
        .write_binary(complex_data.clone(), &file_name_complex, "application/octet-stream", None, None)
        .await?;

    let complex_result = s3_client.read_binary(&file_name_complex).await?;
    assert_eq!(complex_result.data, complex_data, "Complex filename data should match");
    assert_eq!(complex_result.sha256, complex_hash, "Complex filename hash should match");
    s3_client.delete(&file_name_complex).await?;
    println!("✓ Complex file names handled correctly");
    
    println!("\n✅ All edge case tests passed!");
    Ok(())
}