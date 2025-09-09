use mina::{generate_mina_keypair, keypair::{keypair_from_private_key, verify_keypair}};

#[test]
fn test_generate_keypair() {
    // Generate a new keypair
    let keypair = generate_mina_keypair().expect("Failed to generate keypair");
    
    // Check that private key is not empty
    assert!(!keypair.private_key.is_empty());
    println!("Generated private key: {}", keypair.private_key);
    
    // Check that public key is not empty and starts with B62 (Mina address prefix)
    assert!(!keypair.public_key.is_empty());
    assert!(keypair.public_key.starts_with("B62"));
    println!("Generated public key: {}", keypair.public_key);
    
    // Verify that the private key matches the public key
    let is_valid = verify_keypair(&keypair.private_key, &keypair.public_key)
        .expect("Failed to verify keypair");
    assert!(is_valid, "Generated keypair should be valid");
}

#[test]
fn test_keypair_from_private_key() {
    // Generate a keypair first
    let generated = generate_mina_keypair().expect("Failed to generate keypair");
    
    // Recreate keypair from private key
    let keypair = keypair_from_private_key(&generated.private_key)
        .expect("Failed to create keypair from private key");
    
    // Verify the public key matches
    let address = keypair.public.into_address();
    assert_eq!(address, generated.public_key);
}

#[test]
fn test_multiple_keypairs_are_different() {
    // Generate multiple keypairs
    let keypair1 = generate_mina_keypair().expect("Failed to generate keypair 1");
    let keypair2 = generate_mina_keypair().expect("Failed to generate keypair 2");
    let keypair3 = generate_mina_keypair().expect("Failed to generate keypair 3");
    
    // Ensure all private keys are different
    assert_ne!(keypair1.private_key, keypair2.private_key);
    assert_ne!(keypair1.private_key, keypair3.private_key);
    assert_ne!(keypair2.private_key, keypair3.private_key);
    
    // Ensure all public keys are different
    assert_ne!(keypair1.public_key, keypair2.public_key);
    assert_ne!(keypair1.public_key, keypair3.public_key);
    assert_ne!(keypair2.public_key, keypair3.public_key);
}

#[test]
fn test_verify_keypair_with_wrong_address() {
    // Generate two different keypairs
    let keypair1 = generate_mina_keypair().expect("Failed to generate keypair 1");
    let keypair2 = generate_mina_keypair().expect("Failed to generate keypair 2");
    
    // Verify that keypair1's private key doesn't match keypair2's public key
    let is_valid = verify_keypair(&keypair1.private_key, &keypair2.public_key)
        .expect("Failed to verify keypair");
    assert!(!is_valid, "Different keypair should not validate");
}