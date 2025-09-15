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

/// Calculate cost based on duration and resource requirements
///
/// # Arguments
/// * `duration_ms` - Duration in milliseconds
/// * `min_memory_gb` - Minimum memory requirement in GB
/// * `min_cpu_cores` - Minimum CPU cores requirement
/// * `requires_tee` - Whether TEE is required
///
/// # Returns
/// Cost in units using formula: duration_in_sec * min_memory_gb * min_cpu_cores * (requires_tee? 10 : 1)
/// Duration is rounded up to the nearest second
pub fn calculate_cost(duration_ms: u64, min_memory_gb: u16, min_cpu_cores: u16, requires_tee: bool) -> u64 {
    // Round up to nearest second: (duration_ms + 999) / 1000
    let duration_sec = (duration_ms + 999) / 1000;
    let tee_multiplier = if requires_tee { 10 } else { 1 };

    duration_sec * (min_memory_gb as u64) * (min_cpu_cores as u64) * tee_multiplier
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

    #[test]
    fn test_calculate_cost() {
        // Test basic cost calculation without TEE
        // 10 seconds (10000ms) * 2GB * 4 cores * 1 (no TEE) = 80 units
        assert_eq!(calculate_cost(10000, 2, 4, false), 80);

        // Test with TEE enabled
        // 10 seconds * 2GB * 4 cores * 10 (TEE multiplier) = 800 units
        assert_eq!(calculate_cost(10000, 2, 4, true), 800);

        // Test with different resources
        assert_eq!(calculate_cost(60000, 1, 1, false), 60);

        // Test with fractional seconds (should round up)
        // 5.5 seconds (5500ms) -> 6 seconds * 1GB * 1 core * 1 = 6 units
        assert_eq!(calculate_cost(5500, 1, 1, false), 6);

        // Test exact second boundary
        // 5 seconds (5000ms) -> 5 seconds * 1GB * 1 core * 1 = 5 units
        assert_eq!(calculate_cost(5000, 1, 1, false), 5);

        // Test minimal duration (1ms) rounds up to 1 second
        // 0.001 seconds (1ms) -> 1 second * 1GB * 1 core * 1 = 1 unit
        assert_eq!(calculate_cost(1, 1, 1, false), 1);
    }
}