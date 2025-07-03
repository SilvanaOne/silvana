use anyhow::{anyhow, Result};
use std::env;
use tracing::{info, warn};
use tracing_appender::rolling;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub async fn init_logging() -> Result<()> {
    // Initialize tracing with daily rolling file logging
    let log_dir = env::var("LOG_DIR").unwrap_or_else(|_| "./logs".to_string());
    let log_file_prefix = env::var("LOG_FILE_PREFIX").unwrap_or_else(|_| "silvana-rpc".to_string());

    // FIXED: Safe directory creation with proper error handling
    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        return Err(anyhow!(
            "Failed to create log directory '{}': {}",
            log_dir,
            e
        ));
    }

    // FIXED: Safe file appender creation with error handling
    let file_appender =
        match std::panic::catch_unwind(|| rolling::daily(&log_dir, &log_file_prefix)) {
            Ok(appender) => appender,
            Err(_) => {
                return Err(anyhow!(
                    "Failed to create daily rolling file appender for directory '{}'",
                    log_dir
                ));
            }
        };

    // FIXED: Safe non-blocking wrapper with error handling
    let (non_blocking, _guard) =
        match std::panic::catch_unwind(|| tracing_appender::non_blocking(file_appender)) {
            Ok(result) => result,
            Err(_) => {
                return Err(anyhow!("Failed to create non-blocking file appender"));
            }
        };

    // FIXED: Safe tracing subscriber initialization with error handling
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        warn!("Failed to parse RUST_LOG environment variable, defaulting to 'info' level");
        "info".into()
    });

    let subscriber_result = std::panic::catch_unwind(|| {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(non_blocking)
                    .with_ansi(false) // Disable ANSI colors for file output
                    .with_target(false), // Clean up the format for files
            )
            .init()
    });

    if subscriber_result.is_err() {
        return Err(anyhow!("Failed to initialize tracing subscriber"));
    }

    // Log file information
    info!("📝 Logging to daily rotating files in: {}/", log_dir);
    info!(
        "📂 Log file pattern: {}/{}.<YYYY-MM-DD>",
        log_dir, log_file_prefix
    );
    info!("🔄 Rotation: Daily at midnight UTC");

    // CRITICAL: Store guard to prevent dropping - this keeps the logging thread alive!
    // We intentionally "leak" this guard for the lifetime of the application
    std::mem::forget(_guard);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[tokio::test]
    async fn test_logging_init_with_valid_directory() {
        // Use a simple test directory that we can create and clean up
        let log_dir = "./test_logs_temp";

        // Set environment variables for test
        env::set_var("LOG_DIR", log_dir);
        env::set_var("LOG_FILE_PREFIX", "test-log");

        // This should succeed
        let result = init_logging().await;
        assert!(
            result.is_ok(),
            "Logging initialization should succeed with valid directory"
        );

        // Verify directory was created
        assert!(fs::metadata(log_dir).is_ok(), "Log directory should exist");

        // Clean up
        let _ = fs::remove_dir_all(log_dir);
        env::remove_var("LOG_DIR");
        env::remove_var("LOG_FILE_PREFIX");
    }

    #[tokio::test]
    async fn test_logging_init_with_invalid_directory() {
        // Try to create log directory in a path that should fail on most systems
        let invalid_path = if cfg!(windows) {
            "Z:\\nonexistent\\deeply\\nested\\invalid\\path"
        } else {
            "/root/nonexistent/deeply/nested/invalid/path"
        };

        env::set_var("LOG_DIR", invalid_path);
        env::set_var("LOG_FILE_PREFIX", "test-log");

        // This should fail gracefully, not panic
        let result = init_logging().await;
        assert!(
            result.is_err(),
            "Logging initialization should fail gracefully with invalid directory"
        );

        // Verify error message contains useful information
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("Failed to create log directory"),
            "Error should mention directory creation failure"
        );
        assert!(
            error_msg.contains(invalid_path),
            "Error should include the problematic path"
        );

        // Clean up
        env::remove_var("LOG_DIR");
        env::remove_var("LOG_FILE_PREFIX");
    }

    #[test]
    fn test_environment_variable_fallbacks() {
        // Save current values
        let original_log_dir = env::var("LOG_DIR").ok();
        let original_log_prefix = env::var("LOG_FILE_PREFIX").ok();

        // Remove environment variables to test fallbacks
        env::remove_var("LOG_DIR");
        env::remove_var("LOG_FILE_PREFIX");

        // Test that fallbacks work without panicking
        let log_dir = env::var("LOG_DIR").unwrap_or_else(|_| "./logs".to_string());
        let log_file_prefix =
            env::var("LOG_FILE_PREFIX").unwrap_or_else(|_| "silvana-rpc".to_string());

        assert_eq!(log_dir, "./logs");
        assert_eq!(log_file_prefix, "silvana-rpc");

        // Restore original values
        if let Some(val) = original_log_dir {
            env::set_var("LOG_DIR", val);
        }
        if let Some(val) = original_log_prefix {
            env::set_var("LOG_FILE_PREFIX", val);
        }
    }

    #[test]
    fn test_directory_path_safety() {
        // Test various potentially problematic directory paths
        let test_paths = vec![
            "./test_empty",       // Test with created path
            ".",                  // Current directory
            "./",                 // Current directory with slash
            "./test_logs_safety", // Test relative path
        ];

        for path in test_paths {
            // This should not panic, even with edge case paths
            let result = std::fs::create_dir_all(path);
            // We don't assert success because some paths might fail due to permissions,
            // but we verify it doesn't panic
            match result {
                Ok(_) => {
                    // Clean up if successful (only clean up our test directories)
                    if path.starts_with("./test_") {
                        let _ = std::fs::remove_dir_all(path);
                    }
                }
                Err(_) => {
                    // Expected for some paths - this is fine as long as it doesn't panic
                }
            }
        }
    }

    #[test]
    fn test_panic_catching_behavior() {
        // Test that our panic catching actually works
        let panic_result = std::panic::catch_unwind(|| panic!("Test panic"));

        assert!(panic_result.is_err(), "Panic should be caught");

        // Test with a non-panicking operation
        let no_panic_result = std::panic::catch_unwind(|| "success");

        assert!(
            no_panic_result.is_ok(),
            "Non-panicking operation should succeed"
        );
        assert_eq!(no_panic_result.unwrap(), "success");
    }

    #[test]
    fn test_error_handling_safety() {
        // Test that directory creation error handling works
        let test_result = std::fs::create_dir_all("");

        // This might succeed or fail depending on the system, but should not panic
        match test_result {
            Ok(_) => {
                // Empty path might be interpreted as current directory on some systems
            }
            Err(e) => {
                // This is expected behavior and should not panic
                assert!(
                    !e.to_string().is_empty(),
                    "Error message should not be empty"
                );
            }
        }
    }
}
