use crate::error::{CoordinatorError, Result};
use sui::fetch::AgentMethod;

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

/// Calculate session cost based on duration and agent method resources
///
/// # Arguments
/// * `duration_ms` - Session duration in milliseconds
/// * `agent_method` - Agent method containing resource requirements
///
/// # Returns
/// Cost in units using formula: duration_in_sec * min_memory_gb * min_cpu_cores * (requires_tee? 10 : 1)
/// Duration is rounded up to the nearest second
pub fn calculate_session_cost(duration_ms: u64, agent_method: &AgentMethod) -> u64 {
    // Round up to nearest second: (duration_ms + 999) / 1000
    let duration_sec = (duration_ms + 999) / 1000;
    let tee_multiplier = if agent_method.requires_tee { 10 } else { 1 };

    duration_sec * (agent_method.min_memory_gb as u64) * (agent_method.min_cpu_cores as u64) * tee_multiplier
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
    fn test_calculate_session_cost() {
        // Test basic cost calculation without TEE
        let agent_method = AgentMethod {
            docker_image: "test:latest".to_string(),
            docker_sha256: None,
            min_memory_gb: 2,
            min_cpu_cores: 4,
            requires_tee: false,
        };

        // 10 seconds (10000ms) * 2GB * 4 cores * 1 (no TEE) = 80 units
        assert_eq!(calculate_session_cost(10000, &agent_method), 80);

        // Test with TEE enabled
        let agent_method_tee = AgentMethod {
            docker_image: "test:latest".to_string(),
            docker_sha256: None,
            min_memory_gb: 2,
            min_cpu_cores: 4,
            requires_tee: true,
        };

        // 10 seconds * 2GB * 4 cores * 10 (TEE multiplier) = 800 units
        assert_eq!(calculate_session_cost(10000, &agent_method_tee), 800);

        // Test with different resources
        let agent_method_small = AgentMethod {
            docker_image: "test:latest".to_string(),
            docker_sha256: None,
            min_memory_gb: 1,
            min_cpu_cores: 1,
            requires_tee: false,
        };

        // 60 seconds (60000ms) * 1GB * 1 core * 1 = 60 units
        assert_eq!(calculate_session_cost(60000, &agent_method_small), 60);

        // Test with fractional seconds (should round up)
        // 5.5 seconds (5500ms) -> 6 seconds * 1GB * 1 core * 1 = 6 units
        assert_eq!(calculate_session_cost(5500, &agent_method_small), 6);

        // Test exact second boundary
        // 5 seconds (5000ms) -> 5 seconds * 1GB * 1 core * 1 = 5 units
        assert_eq!(calculate_session_cost(5000, &agent_method_small), 5);

        // Test minimal duration (1ms) rounds up to 1 second
        // 0.001 seconds (1ms) -> 1 second * 1GB * 1 core * 1 = 1 unit
        assert_eq!(calculate_session_cost(1, &agent_method_small), 1);
    }
}