//! Centralized constants for the storage crate
//!
//! This module contains all configurable constants for storage operations,
//! including archive size limits, compression settings, and S3 configuration.

// =============================================================================
// Archive Size Limits
// =============================================================================

/// Maximum size for uncompressed tar archives (10 MB).
/// Archives exceeding this size before compression will be rejected.
pub const MAX_TAR_SIZE: usize = 10_485_760; // 10 MB

/// Maximum size for compressed archives (1 MB).
/// This is the default limit for the final compressed archive size.
pub const MAX_COMPRESSED_ARCHIVE_SIZE: usize = 1_048_576; // 1 MB

// =============================================================================
// Compression Settings
// =============================================================================

/// Default compression level for zstd (range: 1-22).
/// Level 3 provides a good balance between compression ratio and speed.
pub const DEFAULT_COMPRESSION_LEVEL: i32 = 3;

// =============================================================================
// S3 Configuration
// =============================================================================

/// Default S3 bucket name for binary distribution.
pub const DEFAULT_S3_BUCKET: &str = "silvana-distribution";

/// Default MIME type for compressed tar archives.
pub const ARCHIVE_MIME_TYPE: &str = "application/x-tar+zstd";

/// Maximum number of retries when checking S3 object availability.
pub const S3_AVAILABILITY_MAX_RETRIES: u32 = 10;

/// Delay between S3 availability check retries (in milliseconds).
pub const S3_AVAILABILITY_RETRY_DELAY_MS: u64 = 500;

// =============================================================================
// File System Settings
// =============================================================================

/// Whether to follow symbolic links when archiving by default.
pub const DEFAULT_FOLLOW_SYMLINKS: bool = false;

/// Default patterns to ignore when creating archives.
/// These patterns are always applied in addition to .gitignore rules.
pub const DEFAULT_IGNORE_PATTERNS: &[&str] = &[
    ".git/",
    ".svn/",
    ".hg/",
    ".DS_Store",
    "Thumbs.db",
    "*.swp",
    "*.swo",
    "*~",
    "node_modules/",
    "target/",
    "dist/",
    "build/",
    ".env",
    ".env.local",
    "*.log",
];