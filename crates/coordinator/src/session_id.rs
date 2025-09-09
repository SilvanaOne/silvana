use crate::error::{CoordinatorError, Result};

/// Session information for Docker containers
#[derive(Debug, Clone)]
pub struct DockerSession {
    pub session_id: String,
    pub session_private_key: String,
}

/// Generate a new Docker session with keypair
/// Uses Ed25519 keypair generation from the sui crate
pub fn generate_docker_session() -> Result<DockerSession> {
    let keypair = sui::keypair::generate_ed25519()
        .map_err(|e| CoordinatorError::ConfigError(format!("Failed to generate keypair: {}", e)))?;
    
    Ok(DockerSession {
        session_id: keypair.address.to_string(),
        session_private_key: keypair.sui_private_key,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_generate_docker_session() {
        let session = generate_docker_session().unwrap();
        
        // Session ID should start with 0x (Sui address format)
        assert!(session.session_id.starts_with("0x"));
        
        // Session ID should be 66 characters (0x + 64 hex chars)
        assert_eq!(session.session_id.len(), 66);
        
        // Session private key should start with suiprivkey
        assert!(session.session_private_key.starts_with("suiprivkey"));
        
        // Should be able to generate multiple different sessions
        let session2 = generate_docker_session().unwrap();
        assert_ne!(session.session_id, session2.session_id);
        assert_ne!(session.session_private_key, session2.session_private_key);
    }
}