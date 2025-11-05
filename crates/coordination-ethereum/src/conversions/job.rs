//! Job type conversions between Solidity and Rust

use silvana_coordination_trait::JobStatus;

/// Convert Solidity JobStatus (u8) to Rust JobStatus
pub fn convert_job_status(status: u8) -> JobStatus {
    match status {
        0 => JobStatus::Pending,
        1 => JobStatus::Running,
        2 => JobStatus::Completed,
        3 => JobStatus::Failed(String::new()), // Error message should be provided separately
        _ => JobStatus::Pending, // Default to pending for unknown status
    }
}

/// Convert Rust JobStatus to Solidity JobStatus (u8)
pub fn job_status_to_u8(status: &JobStatus) -> u8 {
    match status {
        JobStatus::Pending => 0,
        JobStatus::Running => 1,
        JobStatus::Completed => 2,
        JobStatus::Failed(_) => 3,
    }
}

/// Convert u64 array from Vec to Option
pub fn optional_u64_vec(vec: Vec<u64>) -> Option<Vec<u64>> {
    if vec.is_empty() {
        None
    } else {
        Some(vec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_job_status() {
        assert_eq!(convert_job_status(0), JobStatus::Pending);
        assert_eq!(convert_job_status(1), JobStatus::Running);
        assert_eq!(convert_job_status(2), JobStatus::Completed);
        assert!(matches!(convert_job_status(3), JobStatus::Failed(_)));
        assert_eq!(convert_job_status(99), JobStatus::Pending); // Unknown defaults to Pending
    }

    #[test]
    fn test_job_status_to_u8() {
        assert_eq!(job_status_to_u8(&JobStatus::Pending), 0);
        assert_eq!(job_status_to_u8(&JobStatus::Running), 1);
        assert_eq!(job_status_to_u8(&JobStatus::Completed), 2);
        assert_eq!(
            job_status_to_u8(&JobStatus::Failed("error".to_string())),
            3
        );
    }

    #[test]
    fn test_optional_u64_vec() {
        assert_eq!(optional_u64_vec(vec![]), None);
        assert_eq!(optional_u64_vec(vec![1, 2, 3]), Some(vec![1, 2, 3]));
    }
}
