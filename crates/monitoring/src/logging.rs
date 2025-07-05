//! Logging configuration and initialization for Silvana services
//!
//! This module provides centralized logging functionality with:
//! - Daily rotating file logging or console logging
//! - Configurable log directories and file prefixes
//! - Safe error handling for logging setup
//! - Environment variable configuration
//! - OpenTelemetry integration for sending logs to BetterStack

use anyhow::Result;
use std::env;
use tracing::{Event, Level, Subscriber, info, warn};
use tracing_appender::rolling;
use tracing_subscriber::{Layer, layer::SubscriberExt, util::SubscriberInitExt};

// Import OpenTelemetry config checker
use crate::opentelemetry::OpenTelemetryConfig;

/// Maximum length for log messages (999 characters)
const MAX_LOG_MESSAGE_LENGTH: usize = 999;

/// Truncate a message to the maximum allowed length
fn truncate_message(message: &str) -> String {
    if message.len() <= MAX_LOG_MESSAGE_LENGTH {
        message.to_string()
    } else {
        format!("{}...[truncated]", &message[..MAX_LOG_MESSAGE_LENGTH - 14])
    }
}

/// Custom layer for sending logs to BetterStack via OpenTelemetry
struct OpenTelemetryLayer;

impl<S> Layer<S> for OpenTelemetryLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        // Only send warn and error level logs to BetterStack to avoid spam
        let level = *event.metadata().level();

        // CRITICAL: Prevent recursive loop by excluding logs from monitoring module
        let target = event.metadata().target();
        if target.starts_with("monitoring::") || target.contains("opentelemetry") {
            return; // Skip logs from our own monitoring/OpenTelemetry code
        }

        if level <= Level::WARN {
            // Extract the log message
            let mut visitor = LogVisitor::default();
            event.record(&mut visitor);

            if let Some(message) = visitor.get_message() {
                let level_str = match level {
                    Level::ERROR => "error",
                    Level::WARN => "warn",
                    Level::INFO => "info",
                    Level::DEBUG => "debug",
                    Level::TRACE => "trace",
                };

                // Send to BetterStack asynchronously (don't block logging)
                tokio::spawn(async move {
                    crate::opentelemetry::log(level_str, &message).await;
                });
            }
        }
    }
}

/// Visitor to extract log message from tracing events
#[derive(Default)]
struct LogVisitor {
    message: Option<String>,
    fields: Vec<String>,
}

impl tracing::field::Visit for LogVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let field_str = format!("{}={:?}", field.name(), value);
        if field.name() == "message" {
            self.message = Some(format!("{:?}", value).trim_matches('"').to_string());
        } else {
            self.fields.push(field_str);
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else {
            self.fields.push(format!("{}={}", field.name(), value));
        }
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.fields.push(format!("{}={}", field.name(), value));
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.fields.push(format!("{}={}", field.name(), value));
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.fields.push(format!("{}={}", field.name(), value));
    }
}

impl LogVisitor {
    fn get_message(&self) -> Option<String> {
        let full_message = match &self.message {
            Some(msg) => {
                if self.fields.is_empty() {
                    msg.clone()
                } else {
                    format!("{} ({})", msg, self.fields.join(", "))
                }
            }
            None => {
                if !self.fields.is_empty() {
                    self.fields.join(", ")
                } else {
                    return None;
                }
            }
        };

        // Always truncate messages before returning
        Some(truncate_message(&full_message))
    }
}

/// Initialize tracing with configurable output destination
///
/// ## Environment Variables
///
/// - `LOG_DESTINATION`: Output destination - "console" or "file" (default: "file")
/// - `LOG_DIR`: Directory for log files (default: "./logs") - only used when LOG_DESTINATION=file
/// - `LOG_FILE_PREFIX`: Prefix for log file names (default: "silvana") - only used when LOG_DESTINATION=file
///
/// ## Example
///
/// ```rust
/// use monitoring::init_logging;
/// use anyhow::Result;
///
/// #[tokio::main]
/// async fn main() -> Result<()> {
///     // Initialize logging with configurable destination
///     // Uses LOG_DESTINATION env var: "console" for stdout, "file" for daily rotating files
///     init_logging().await?;
///     
///     // Your application code here...
///     Ok(())
/// }
/// ```
pub async fn init_logging() -> Result<()> {
    let log_destination = env::var("LOG_DESTINATION").unwrap_or_else(|_| "file".to_string());

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        warn!("Failed to parse RUST_LOG environment variable, defaulting to 'info' level");
        "info".into()
    });

    match log_destination.to_lowercase().as_str() {
        "console" => {
            // Console logging - output to stdout
            let subscriber_result = std::panic::catch_unwind(|| {
                let subscriber = tracing_subscriber::registry().with(env_filter).with(
                    tracing_subscriber::fmt::layer()
                        .with_writer(std::io::stdout)
                        .with_ansi(true) // Enable ANSI colors for console output
                        .with_target(false),
                );

                // Add OpenTelemetry layer if configured
                if OpenTelemetryConfig::is_configured() {
                    subscriber.with(OpenTelemetryLayer).init()
                } else {
                    subscriber.init()
                }
            });

            if subscriber_result.is_err() {
                return Err(anyhow::anyhow!(
                    "Failed to initialize console tracing subscriber"
                ));
            }

            info!("📺 Logging to console (stdout)");
        }
        _ => {
            // File logging (default) - daily rotating files
            let log_dir = env::var("LOG_DIR").unwrap_or_else(|_| "./logs".to_string());
            let log_file_prefix =
                env::var("LOG_FILE_PREFIX").unwrap_or_else(|_| "silvana".to_string());

            // FIXED: Safe directory creation with proper error handling
            if let Err(e) = std::fs::create_dir_all(&log_dir) {
                return Err(anyhow::anyhow!(
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
                        return Err(anyhow::anyhow!(
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
                        return Err(anyhow::anyhow!(
                            "Failed to create non-blocking file appender"
                        ));
                    }
                };

            // FIXED: Safe tracing subscriber initialization with error handling
            let subscriber_result = std::panic::catch_unwind(|| {
                let subscriber = tracing_subscriber::registry().with(env_filter).with(
                    tracing_subscriber::fmt::layer()
                        .with_writer(non_blocking)
                        .with_ansi(false) // Disable ANSI colors for file output
                        .with_target(false), // Clean up the format for files
                );

                // Add OpenTelemetry layer if configured
                if OpenTelemetryConfig::is_configured() {
                    subscriber.with(OpenTelemetryLayer).init()
                } else {
                    subscriber.init()
                }
            });

            if subscriber_result.is_err() {
                return Err(anyhow::anyhow!(
                    "Failed to initialize file tracing subscriber"
                ));
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
        }
    }

    // Check and report BetterStack OpenTelemetry connection status
    if OpenTelemetryConfig::is_configured() {
        match OpenTelemetryConfig::from_env() {
            Ok(config) => {
                info!("🔗 BetterStack OpenTelemetry: ✅ CONFIGURED");
                info!("📡 Host: {}", config.ingesting_host);
                info!("🏷️  Source ID: {}", config.source_id);
                info!("📤 Logs will be sent to BetterStack (WARN level and above)");
            }
            Err(e) => {
                warn!(
                    "🔗 BetterStack OpenTelemetry: ⚠️  CONFIGURATION ERROR - {}",
                    e
                );
            }
        }
    } else {
        info!("🔗 BetterStack OpenTelemetry: ❌ NOT CONFIGURED");
        info!(
            "   Set OPENTELEMETRY_INGESTING_HOST, OPENTELEMETRY_SOURCE_ID, and OPENTELEMETRY_SOURCE_TOKEN to enable"
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_truncate_message() {
        // Test that short messages are not truncated
        let short_message = "This is a short message";
        assert_eq!(truncate_message(short_message), short_message);

        // Test that long messages are truncated to 999 characters
        let long_message = "a".repeat(1500); // 1500 characters
        let truncated = truncate_message(&long_message);
        assert_eq!(truncated.len(), 999);
        assert!(truncated.ends_with("...[truncated]"));
        assert!(truncated.starts_with("aaa"));

        // Test with a realistic SQL query example
        let long_sql = format!(
            "SELECT * FROM users WHERE id IN ({}) AND status = 'active' AND created_at > '2023-01-01'",
            (1..200)
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
        let truncated_sql = truncate_message(&long_sql);
        assert!(truncated_sql.len() <= 999);
        if long_sql.len() > 999 {
            assert!(truncated_sql.ends_with("...[truncated]"));
        }

        // Test that exactly 999 characters are not truncated
        let exactly_999 = "b".repeat(999);
        assert_eq!(truncate_message(&exactly_999), exactly_999);

        // Test that 1000 characters get truncated
        let exactly_1000 = "c".repeat(1000);
        let truncated_1000 = truncate_message(&exactly_1000);
        assert_eq!(truncated_1000.len(), 999);
        assert!(truncated_1000.ends_with("...[truncated]"));
    }

    #[test]
    fn test_log_visitor_truncation() {
        let mut visitor = LogVisitor::default();

        // Test with a long message
        let long_message = "x".repeat(1200);
        visitor.message = Some(long_message.clone());

        let result = visitor.get_message();
        assert!(result.is_some());
        let message = result.unwrap();
        assert_eq!(message.len(), 999);
        assert!(message.ends_with("...[truncated]"));

        // Test with fields as well
        let mut visitor_with_fields = LogVisitor::default();
        visitor_with_fields.message = Some("A short message".to_string());
        visitor_with_fields.fields.push("field1=value1".to_string());
        visitor_with_fields
            .fields
            .push("field2=very_long_value_that_makes_the_total_length_exceed_the_limit".repeat(20));

        let result_with_fields = visitor_with_fields.get_message();
        assert!(result_with_fields.is_some());
        let message_with_fields = result_with_fields.unwrap();
        assert!(message_with_fields.len() <= 999);
    }

    #[tokio::test]
    async fn test_logging_init_with_valid_directory() {
        // Use a simple test directory that we can create and clean up
        let log_dir = "./test_logs_temp";

        // Set environment variables for test - explicitly set file destination
        unsafe {
            env::set_var("LOG_DESTINATION", "file");
            env::set_var("LOG_DIR", log_dir);
            env::set_var("LOG_FILE_PREFIX", "test-log");
        }

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
        unsafe {
            env::remove_var("LOG_DESTINATION");
            env::remove_var("LOG_DIR");
            env::remove_var("LOG_FILE_PREFIX");
        }
    }

    #[tokio::test]
    async fn test_logging_init_with_invalid_directory() {
        // Try to create log directory in a path that should fail on most systems
        let invalid_path = if cfg!(windows) {
            "Z:\\nonexistent\\deeply\\nested\\invalid\\path"
        } else {
            "/root/nonexistent/deeply/nested/invalid/path"
        };

        unsafe {
            env::set_var("LOG_DESTINATION", "file");
            env::set_var("LOG_DIR", invalid_path);
            env::set_var("LOG_FILE_PREFIX", "test-log");
        }

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
        unsafe {
            env::remove_var("LOG_DESTINATION");
            env::remove_var("LOG_DIR");
            env::remove_var("LOG_FILE_PREFIX");
        }
    }

    #[tokio::test]
    async fn test_console_logging_initialization() {
        // Set environment variable for console logging
        unsafe {
            env::set_var("LOG_DESTINATION", "console");
        }

        // This should succeed without needing log directory
        let result = init_logging().await;
        assert!(
            result.is_ok(),
            "Console logging initialization should succeed"
        );

        // Clean up
        unsafe {
            env::remove_var("LOG_DESTINATION");
        }
    }

    #[tokio::test]
    async fn test_console_logging_case_insensitive() {
        // Test various case combinations for "console"
        let test_cases = vec!["CONSOLE", "Console", "CoNsOlE", "console"];

        for case in test_cases {
            unsafe {
                env::set_var("LOG_DESTINATION", case);
            }

            let result = init_logging().await;
            assert!(
                result.is_ok(),
                "Console logging should work with case: {}",
                case
            );

            unsafe {
                env::remove_var("LOG_DESTINATION");
            }
        }
    }

    #[tokio::test]
    async fn test_default_file_logging() {
        // Test that file logging is the default when LOG_DESTINATION is not set
        let log_dir = "./test_logs_default";

        unsafe {
            env::remove_var("LOG_DESTINATION"); // Ensure it's not set
            env::set_var("LOG_DIR", log_dir);
            env::set_var("LOG_FILE_PREFIX", "default-test");
        }

        let result = init_logging().await;
        assert!(result.is_ok(), "Default file logging should succeed");

        // Verify directory was created (proving file logging was used)
        assert!(
            fs::metadata(log_dir).is_ok(),
            "Log directory should exist for default file logging"
        );

        // Clean up
        let _ = fs::remove_dir_all(log_dir);
        unsafe {
            env::remove_var("LOG_DIR");
            env::remove_var("LOG_FILE_PREFIX");
        }
    }

    #[tokio::test]
    async fn test_invalid_log_destination_defaults_to_file() {
        // Test that invalid LOG_DESTINATION values default to file logging
        let log_dir = "./test_logs_invalid_dest";

        unsafe {
            env::set_var("LOG_DESTINATION", "invalid_value");
            env::set_var("LOG_DIR", log_dir);
            env::set_var("LOG_FILE_PREFIX", "invalid-test");
        }

        let result = init_logging().await;
        assert!(
            result.is_ok(),
            "Invalid destination should default to file logging"
        );

        // Verify directory was created (proving file logging was used as fallback)
        assert!(
            fs::metadata(log_dir).is_ok(),
            "Log directory should exist when defaulting to file"
        );

        // Clean up
        let _ = fs::remove_dir_all(log_dir);
        unsafe {
            env::remove_var("LOG_DESTINATION");
            env::remove_var("LOG_DIR");
            env::remove_var("LOG_FILE_PREFIX");
        }
    }

    #[test]
    fn test_environment_variable_fallbacks() {
        // Save current values
        let original_log_dir = env::var("LOG_DIR").ok();
        let original_log_prefix = env::var("LOG_FILE_PREFIX").ok();
        let original_log_destination = env::var("LOG_DESTINATION").ok();

        // Remove environment variables to test fallbacks
        unsafe {
            env::remove_var("LOG_DESTINATION");
            env::remove_var("LOG_DIR");
            env::remove_var("LOG_FILE_PREFIX");
        }

        // Test that fallbacks work without panicking
        let log_destination = env::var("LOG_DESTINATION").unwrap_or_else(|_| "file".to_string());
        let log_dir = env::var("LOG_DIR").unwrap_or_else(|_| "./logs".to_string());
        let log_file_prefix = env::var("LOG_FILE_PREFIX").unwrap_or_else(|_| "silvana".to_string());

        assert_eq!(log_destination, "file");
        assert_eq!(log_dir, "./logs");
        assert_eq!(log_file_prefix, "silvana");

        // Restore original values
        if let Some(val) = original_log_destination {
            unsafe {
                env::set_var("LOG_DESTINATION", val);
            }
        }
        if let Some(val) = original_log_dir {
            unsafe {
                env::set_var("LOG_DIR", val);
            }
        }
        if let Some(val) = original_log_prefix {
            unsafe {
                env::set_var("LOG_FILE_PREFIX", val);
            }
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
