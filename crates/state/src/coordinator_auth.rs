//! Coordinator authentication and authorization
//!
//! This module handles Ed25519 signature verification for coordinators
//! and access control checks based on coordinator groups and whitelists.

use anyhow::Result;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sea_orm::{ConnectionTrait, FromQueryResult};
use std::time::{SystemTime, UNIX_EPOCH};
use tonic::Status;
use tracing::{debug, warn};

use crate::proto::CoordinatorAuth;

/// Maximum age of a coordinator request in seconds (5 minutes)
const MAX_REQUEST_AGE_SECS: i64 = 300;

/// Verify coordinator Ed25519 signature
///
/// Signs message format: "{public_key}:{app_instance_id}:{operation}:{timestamp}:{nonce}"
pub fn verify_coordinator_signature(
    auth: &CoordinatorAuth,
    app_instance_id: &str,
    operation: &str,
) -> Result<(), Status> {
    // Construct the message that was signed
    let message = format!(
        "{}:{}:{}:{}:{}",
        auth.coordinator_public_key, app_instance_id, operation, auth.timestamp, auth.nonce
    );

    // Decode public key from hex
    let public_key_bytes = hex::decode(&auth.coordinator_public_key).map_err(|e| {
        Status::invalid_argument(format!("Invalid coordinator public key hex: {}", e))
    })?;

    if public_key_bytes.len() != 32 {
        return Err(Status::invalid_argument(format!(
            "Invalid coordinator public key length: expected 32 bytes, got {}",
            public_key_bytes.len()
        )));
    }

    let mut key_array = [0u8; 32];
    key_array.copy_from_slice(&public_key_bytes);

    let verifying_key = VerifyingKey::from_bytes(&key_array).map_err(|e| {
        Status::invalid_argument(format!("Invalid coordinator public key format: {}", e))
    })?;

    // Decode signature from hex
    let signature_bytes = hex::decode(&auth.signature).map_err(|e| {
        Status::invalid_argument(format!("Invalid coordinator signature hex: {}", e))
    })?;

    if signature_bytes.len() != 64 {
        return Err(Status::invalid_argument(format!(
            "Invalid coordinator signature length: expected 64 bytes, got {}",
            signature_bytes.len()
        )));
    }

    let mut sig_array = [0u8; 64];
    sig_array.copy_from_slice(&signature_bytes);

    let signature = Signature::from_bytes(&sig_array);

    // Verify signature
    verifying_key
        .verify(message.as_bytes(), &signature)
        .map_err(|e| {
            warn!(
                "Coordinator signature verification failed for {}: {}",
                auth.coordinator_public_key, e
            );
            Status::unauthenticated("Invalid coordinator signature")
        })?;

    // Verify timestamp to prevent replay attacks
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let age = (now - auth.timestamp).abs();
    if age > MAX_REQUEST_AGE_SECS {
        return Err(Status::unauthenticated(format!(
            "Request timestamp too old or in future: {} seconds (max {} seconds)",
            age, MAX_REQUEST_AGE_SECS
        )));
    }

    debug!(
        "Coordinator {} signature verified for operation {} on app_instance {}",
        auth.coordinator_public_key, operation, app_instance_id
    );

    Ok(())
}

/// Access level for coordinators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessLevel {
    ReadOnly,
    JobManage,
    Full,
}

impl AccessLevel {
    /// Parse access level from string
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "READ_ONLY" => Some(AccessLevel::ReadOnly),
            "JOB_MANAGE" => Some(AccessLevel::JobManage),
            "FULL" => Some(AccessLevel::Full),
            _ => None,
        }
    }

    /// Check if this access level permits a given operation
    pub fn permits(&self, operation: &str) -> bool {
        match self {
            AccessLevel::Full => true,
            AccessLevel::JobManage => matches!(
                operation,
                "create_job"
                    | "start_job"
                    | "complete_job"
                    | "fail_job"
                    | "terminate_job"
                    | "get_job"
                    | "get_pending_sequences"
                    | "get_pending_count"
                    | "get_failed_count"
                    | "get_total_count"
                    | "get_pending_jobs"
                    | "get_failed_jobs"
                    | "get_jobs_batch"
                    | "restart_failed_jobs"
                    | "remove_failed_jobs"
            ),
            AccessLevel::ReadOnly => matches!(
                operation,
                "get_job"
                    | "get_pending_sequences"
                    | "get_pending_count"
                    | "get_failed_count"
                    | "get_total_count"
                    | "get_pending_jobs"
                    | "get_failed_jobs"
                    | "get_jobs_batch"
            ),
        }
    }
}

/// Verify coordinator has access to app instance through a whitelisted group
pub async fn verify_coordinator_access<C>(
    db: &C,
    coordinator_public_key: &str,
    app_instance_id: &str,
    required_operation: &str,
) -> Result<(), Status>
where
    C: ConnectionTrait,
{
    use sea_orm::Statement;

    // Result from coordinator access check
    #[derive(Debug, FromQueryResult)]
    struct CoordinatorAccessResult {
        access_level: String,
    }

    // Query to check if coordinator has access through any group
    let query_result = CoordinatorAccessResult::find_by_statement(Statement::from_sql_and_values(
        sea_orm::DbBackend::MySql,
        r#"
            SELECT aca.access_level
            FROM app_instance_coordinator_access aca
            JOIN coordinator_group_members cgm ON aca.group_id = cgm.group_id
            WHERE aca.app_instance_id = ?
              AND cgm.coordinator_public_key = ?
              AND (aca.expires_at IS NULL OR aca.expires_at > NOW())
            LIMIT 1
        "#,
        vec![
            app_instance_id.into(),
            coordinator_public_key.into(),
        ],
    ))
    .one(db)
    .await
    .map_err(|e| {
        Status::internal(format!("Database error checking coordinator access: {}", e))
    })?;

    match query_result {
        None => {
            warn!(
                "Coordinator {} denied access to app_instance {}: not in any whitelisted group",
                coordinator_public_key, app_instance_id
            );
            Err(Status::permission_denied(format!(
                "Coordinator {} does not have access to app instance {}",
                coordinator_public_key, app_instance_id
            )))
        }
        Some(result) => {
            let access_level = AccessLevel::from_str(&result.access_level).ok_or_else(|| {
                Status::internal(format!("Invalid access level in database: {}", result.access_level))
            })?;

            if !access_level.permits(required_operation) {
                warn!(
                    "Coordinator {} denied operation {} on app_instance {}: insufficient permissions ({:?})",
                    coordinator_public_key, required_operation, app_instance_id, access_level
                );
                return Err(Status::permission_denied(format!(
                    "Coordinator {} has insufficient permissions for operation {}",
                    coordinator_public_key, required_operation
                )));
            }

            debug!(
                "Coordinator {} granted access to operation {} on app_instance {} with access level {:?}",
                coordinator_public_key, required_operation, app_instance_id, access_level
            );

            Ok(())
        }
    }
}

/// Combined verification: signature + access control
pub async fn verify_and_authorize<C>(
    db: &C,
    auth: &CoordinatorAuth,
    app_instance_id: &str,
    operation: &str,
) -> Result<(), Status>
where
    C: ConnectionTrait,
{
    // First verify the signature
    verify_coordinator_signature(auth, app_instance_id, operation)?;

    // Then check access permissions
    verify_coordinator_access(db, &auth.coordinator_public_key, app_instance_id, operation).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_access_level_permissions() {
        // Full access permits everything
        assert!(AccessLevel::Full.permits("create_job"));
        assert!(AccessLevel::Full.permits("get_job"));
        assert!(AccessLevel::Full.permits("anything"));

        // JobManage permits job operations
        assert!(AccessLevel::JobManage.permits("create_job"));
        assert!(AccessLevel::JobManage.permits("start_job"));
        assert!(AccessLevel::JobManage.permits("complete_job"));
        assert!(AccessLevel::JobManage.permits("fail_job"));
        assert!(AccessLevel::JobManage.permits("get_job"));
        assert!(!AccessLevel::JobManage.permits("grant_access"));

        // ReadOnly only permits read operations
        assert!(!AccessLevel::ReadOnly.permits("create_job"));
        assert!(!AccessLevel::ReadOnly.permits("start_job"));
        assert!(AccessLevel::ReadOnly.permits("get_job"));
        assert!(AccessLevel::ReadOnly.permits("get_pending_count"));
    }

    #[test]
    fn test_access_level_from_str() {
        assert_eq!(AccessLevel::from_str("READ_ONLY"), Some(AccessLevel::ReadOnly));
        assert_eq!(AccessLevel::from_str("JOB_MANAGE"), Some(AccessLevel::JobManage));
        assert_eq!(AccessLevel::from_str("FULL"), Some(AccessLevel::Full));
        assert_eq!(AccessLevel::from_str("invalid"), None);
    }
}
