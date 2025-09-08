use mina::{
    generate_mina_keypair,
    keypair::keypair_from_private_key,
    create_signature,
    signature_to_base58,
    signature_from_base58,
    signature_to_strings,
    verify_signature,
};
use mina_signer::BaseField;

#[test]
fn test_sign_and_verify_fields() {
    // Generate a new keypair
    let generated = generate_mina_keypair().expect("Failed to generate keypair");
    
    // Create keypair object for signing
    let keypair = keypair_from_private_key(&generated.private_key)
        .expect("Failed to create keypair");
    
    // Create test fields
    let fields = vec![
        BaseField::from(1u64),
        BaseField::from(2u64),
        BaseField::from(3u64),
    ];
    
    // Create signature
    let sig = create_signature(&keypair, &fields);
    
    // Convert to base58
    let sig_b58 = signature_to_base58(&sig);
    println!("Signature base58: {}", sig_b58);
    assert!(!sig_b58.is_empty());
    
    // Verify the signature
    let is_valid = verify_signature(&sig_b58, &generated.public_key, &fields);
    assert!(is_valid, "Signature should be valid");
}

#[test]
fn test_signature_to_from_base58() {
    // Generate a keypair and create a signature
    let generated = generate_mina_keypair().expect("Failed to generate keypair");
    let keypair = keypair_from_private_key(&generated.private_key)
        .expect("Failed to create keypair");
    
    let fields = vec![BaseField::from(42u64)];
    let sig = create_signature(&keypair, &fields);
    
    // Convert to base58 and back
    let sig_b58 = signature_to_base58(&sig);
    let sig_decoded = signature_from_base58(&sig_b58)
        .expect("Failed to decode signature");
    
    // The signature components should match
    assert_eq!(sig.rx, sig_decoded.rx);
    assert_eq!(sig.s, sig_decoded.s);
}

#[test]
fn test_signature_to_strings() {
    // Generate a keypair and create a signature
    let generated = generate_mina_keypair().expect("Failed to generate keypair");
    let keypair = keypair_from_private_key(&generated.private_key)
        .expect("Failed to create keypair");
    
    let fields = vec![BaseField::from(123u64), BaseField::from(456u64)];
    let sig = create_signature(&keypair, &fields);
    
    // Convert to strings
    let sig_strings = signature_to_strings(&sig);
    
    // Check that r and s are not empty
    assert!(!sig_strings.r.is_empty());
    assert!(!sig_strings.s.is_empty());
    
    println!("Signature r: {}", sig_strings.r);
    println!("Signature s: {}", sig_strings.s);
}

#[test]
fn test_verify_signature_with_wrong_public_key() {
    // Generate two different keypairs
    let keypair1_gen = generate_mina_keypair().expect("Failed to generate keypair 1");
    let keypair2_gen = generate_mina_keypair().expect("Failed to generate keypair 2");
    
    let keypair1 = keypair_from_private_key(&keypair1_gen.private_key)
        .expect("Failed to create keypair 1");
    
    // Sign with keypair1
    let fields = vec![BaseField::from(999u64)];
    let sig = create_signature(&keypair1, &fields);
    let sig_b58 = signature_to_base58(&sig);
    
    // Try to verify with keypair2's public key - should fail
    let is_valid = verify_signature(&sig_b58, &keypair2_gen.public_key, &fields);
    assert!(!is_valid, "Signature should not verify with wrong public key");
}

#[test]
fn test_verify_signature_with_wrong_fields() {
    // Generate a keypair
    let generated = generate_mina_keypair().expect("Failed to generate keypair");
    let keypair = keypair_from_private_key(&generated.private_key)
        .expect("Failed to create keypair");
    
    // Sign some fields
    let original_fields = vec![BaseField::from(1u64), BaseField::from(2u64)];
    let sig = create_signature(&keypair, &original_fields);
    let sig_b58 = signature_to_base58(&sig);
    
    // Try to verify with different fields - should fail
    let wrong_fields = vec![BaseField::from(3u64), BaseField::from(4u64)];
    let is_valid = verify_signature(&sig_b58, &generated.public_key, &wrong_fields);
    assert!(!is_valid, "Signature should not verify with wrong fields");
}

#[test]
fn test_invalid_base58_signature() {
    // Try to decode an invalid base58 string
    let invalid_sig = signature_from_base58("invalid_base58_signature");
    assert!(invalid_sig.is_none(), "Invalid base58 should return None");
}

#[test]
fn test_sign_empty_fields() {
    // Generate a keypair
    let generated = generate_mina_keypair().expect("Failed to generate keypair");
    let keypair = keypair_from_private_key(&generated.private_key)
        .expect("Failed to create keypair");
    
    // Sign empty fields array
    let fields: Vec<BaseField> = vec![];
    let sig = create_signature(&keypair, &fields);
    let sig_b58 = signature_to_base58(&sig);
    
    // Should be able to verify empty fields
    let is_valid = verify_signature(&sig_b58, &generated.public_key, &fields);
    assert!(is_valid, "Should be able to sign and verify empty fields");
}

#[test]
fn test_sign_large_field_values() {
    // Generate a keypair
    let generated = generate_mina_keypair().expect("Failed to generate keypair");
    let keypair = keypair_from_private_key(&generated.private_key)
        .expect("Failed to create keypair");
    
    // Sign with large field values
    let fields = vec![
        BaseField::from(u64::MAX),
        BaseField::from(u64::MAX / 2),
        BaseField::from(1234567890123456789u64),
    ];
    
    let sig = create_signature(&keypair, &fields);
    let sig_b58 = signature_to_base58(&sig);
    
    // Should verify correctly
    let is_valid = verify_signature(&sig_b58, &generated.public_key, &fields);
    assert!(is_valid, "Should handle large field values");
}