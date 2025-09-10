use anyhow::{Result, anyhow};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use tar::{Archive, Builder};
use tracing::{debug, info, warn};
use walkdir::WalkDir;
use zstd::stream::{decode_all, encode_all};

use crate::constants::*;
use crate::s3::S3Client;

/// Archive configuration
pub struct ArchiveConfig {
    /// Compression level for zstd (1-22, default 3)
    pub compression_level: i32,
    /// Whether to follow symlinks
    pub follow_symlinks: bool,
    /// Maximum compressed archive size in bytes (default 1 MB, 0 = unlimited)
    pub max_size: u64,
}

impl Default for ArchiveConfig {
    fn default() -> Self {
        Self {
            compression_level: DEFAULT_COMPRESSION_LEVEL,
            follow_symlinks: DEFAULT_FOLLOW_SYMLINKS,
            max_size: MAX_COMPRESSED_ARCHIVE_SIZE as u64,
        }
    }
}

/// Pack a folder into a compressed tar archive and upload to S3
///
/// # Arguments
/// * `folder_path` - Path to the folder to archive
/// * `s3_client` - S3 client for uploading
/// * `s3_key` - S3 object key for the archive
/// * `config` - Optional archive configuration
///
/// # Returns
/// The SHA256 hash of the compressed archive
pub async fn pack_folder_to_s3(
    folder_path: &Path,
    s3_client: &S3Client,
    s3_key: &str,
    config: Option<ArchiveConfig>,
) -> Result<String> {
    let config = config.unwrap_or_default();

    info!("Packing folder: {:?} to S3 key: {}", folder_path, s3_key);

    // Create compressed archive
    let archive_data = create_compressed_archive(folder_path, &config)?;

    // Calculate SHA256
    let mut hasher = Sha256::new();
    hasher.update(&archive_data);
    let archive_hash = format!("{:x}", hasher.finalize());

    info!(
        "Created archive: size={} bytes, hash={}",
        archive_data.len(),
        archive_hash
    );

    // Upload to S3
    let stored_hash = s3_client
        .write_binary(
            archive_data,
            s3_key,
            ARCHIVE_MIME_TYPE,
            Some(archive_hash.clone()),
        )
        .await?;

    info!("Successfully uploaded archive to S3: {}", s3_key);

    Ok(stored_hash)
}

/// Download archive from S3 and unpack to a folder
///
/// # Arguments
/// * `s3_client` - S3 client for downloading
/// * `s3_key` - S3 object key of the archive
/// * `target_folder` - Path to extract the archive contents
/// * `verify_hash` - Optional expected SHA256 hash for verification
///
/// # Returns
/// The SHA256 hash of the downloaded archive
pub async fn unpack_from_s3(
    s3_client: &S3Client,
    s3_key: &str,
    target_folder: &Path,
    verify_hash: Option<String>,
) -> Result<String> {
    debug!("Downloading archive: {}", s3_key);

    // Download from S3
    let (archive_data, downloaded_hash) = s3_client.read_binary(s3_key).await?;

    // Verify hash if provided
    if let Some(expected_hash) = verify_hash {
        if downloaded_hash != expected_hash {
            return Err(anyhow!(
                "Hash verification failed: expected {}, got {}",
                expected_hash,
                downloaded_hash
            ));
        }
    }

    debug!(
        "Downloaded archive: size={} bytes, hash={}",
        archive_data.len(),
        downloaded_hash
    );

    // Extract archive
    extract_compressed_archive(&archive_data, target_folder)?;

    debug!("Successfully extracted archive to: {:?}", target_folder);

    Ok(downloaded_hash)
}

/// Create a compressed tar archive from a folder
fn create_compressed_archive(folder_path: &Path, config: &ArchiveConfig) -> Result<Vec<u8>> {
    if !folder_path.exists() {
        return Err(anyhow!("Folder does not exist: {:?}", folder_path));
    }

    if !folder_path.is_dir() {
        return Err(anyhow!("Path is not a directory: {:?}", folder_path));
    }

    // Load all gitignore rules (including from subdirectories)
    let all_gitignores = load_all_gitignore_rules(folder_path)?;

    // Create tar archive in memory
    let mut tar_buffer = Vec::new();
    {
        let mut tar_builder = Builder::new(&mut tar_buffer);

        // Use a custom walker that respects gitignore
        let walker = WalkDir::new(folder_path)
            .follow_links(config.follow_symlinks)
            .into_iter()
            .filter_entry(|entry| {
                // Filter out directories that should be ignored BEFORE descending
                let path = entry.path();

                // Always include the root
                if path == folder_path {
                    return true;
                }

                // Check all gitignore rules
                for (gitignore_dir, gitignore) in &all_gitignores {
                    if path.starts_with(gitignore_dir) {
                        if let Ok(rel_to_gitignore) = path.strip_prefix(gitignore_dir) {
                            if gitignore
                                .matched(rel_to_gitignore, entry.file_type().is_dir())
                                .is_ignore()
                            {
                                debug!(
                                    "Filtering out (via {:?}): {:?}",
                                    gitignore_dir, rel_to_gitignore
                                );
                                return false;
                            }
                        }
                    }
                }
                true
            });

        // Walk through filtered entries
        for entry in walker.filter_map(|e| e.ok()) {
            let path = entry.path();
            let relative_path = path.strip_prefix(folder_path)?;

            // Skip the root directory itself
            if relative_path.as_os_str().is_empty() {
                continue;
            }

            debug!("Adding to archive: {:?}", relative_path);

            if entry.file_type().is_file() {
                let mut file = File::open(path)?;
                tar_builder.append_file(relative_path, &mut file)?;
            } else if entry.file_type().is_dir() {
                tar_builder.append_dir(relative_path, path)?;
            } else if config.follow_symlinks && entry.file_type().is_symlink() {
                // If following symlinks, they're already resolved by WalkDir
                // This branch shouldn't be reached, but included for completeness
                warn!("Unexpected symlink encountered: {:?}", path);
            }
        }

        tar_builder.finish()?;
    }

    // Check tar size before compression
    if tar_buffer.len() > MAX_TAR_SIZE {
        return Err(anyhow!(
            "Tar archive size {} bytes exceeds maximum allowed size of {} bytes (10 MB) before compression",
            tar_buffer.len(),
            MAX_TAR_SIZE
        ));
    }

    // Compress with zstd
    let compressed = encode_all(&tar_buffer[..], config.compression_level)?;

    // Check compressed size limit (1 MB by default)
    if config.max_size > 0 && compressed.len() > config.max_size as usize {
        return Err(anyhow!(
            "Compressed archive size {} bytes exceeds maximum allowed size of {} bytes (1 MB)",
            compressed.len(),
            config.max_size
        ));
    }

    println!(
        "Archive created: uncompressed={} bytes ({}KB), compressed={} bytes ({}KB), ratio={:.2}%",
        tar_buffer.len(),
        tar_buffer.len() / 1024,
        compressed.len(),
        compressed.len() / 1024,
        (compressed.len() as f64 / tar_buffer.len() as f64) * 100.0
    );

    Ok(compressed)
}

/// Extract a compressed tar archive to a folder
fn extract_compressed_archive(archive_data: &[u8], target_folder: &Path) -> Result<()> {
    // Create target directory if it doesn't exist
    if !target_folder.exists() {
        fs::create_dir_all(target_folder)?;
    }

    if !target_folder.is_dir() {
        return Err(anyhow!(
            "Target path is not a directory: {:?}",
            target_folder
        ));
    }

    // Decompress with zstd
    let decompressed = decode_all(archive_data)?;

    debug!(
        "Decompressed archive: compressed={} bytes, uncompressed={} bytes",
        archive_data.len(),
        decompressed.len()
    );

    // Extract tar archive
    let mut archive = Archive::new(&decompressed[..]);

    // Set options for extraction
    archive.set_preserve_permissions(true);
    archive.set_preserve_mtime(true);
    archive.set_unpack_xattrs(false); // Don't unpack extended attributes by default

    // Extract to target folder
    archive.unpack(target_folder)?;

    Ok(())
}

/// Load all gitignore rules from a folder tree (including subdirectories)
fn load_all_gitignore_rules(folder_path: &Path) -> Result<Vec<(PathBuf, Gitignore)>> {
    let mut gitignores = Vec::new();
    let mut found_root_gitignore = false;

    // Walk through all directories to find .gitignore files
    for entry in WalkDir::new(folder_path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_dir())
    {
        let dir_path = entry.path();
        let gitignore_path = dir_path.join(".gitignore");

        if dir_path == folder_path {
            found_root_gitignore = gitignore_path.exists();
        }

        if gitignore_path.exists() {
            debug!("Found .gitignore in: {:?}", dir_path);
            let mut builder = GitignoreBuilder::new(dir_path);

            // Add the gitignore file
            if let Some(e) = builder.add(&gitignore_path) {
                warn!("Failed to load .gitignore from {:?}: {}", gitignore_path, e);
                continue;
            }

            // Also add default patterns
            add_default_ignore_patterns(&mut builder);

            if let Ok(gitignore) = builder.build() {
                gitignores.push((dir_path.to_path_buf(), gitignore));
            }
        }
    }

    // Always add default patterns for the root directory if no .gitignore exists there
    if !found_root_gitignore {
        let mut builder = GitignoreBuilder::new(folder_path);
        add_default_ignore_patterns(&mut builder);
        if let Ok(gitignore) = builder.build() {
            gitignores.push((folder_path.to_path_buf(), gitignore));
        }
    }

    Ok(gitignores)
}

/// Add default ignore patterns (common build artifacts, OS files, etc.)
fn add_default_ignore_patterns(builder: &mut GitignoreBuilder) {
    for pattern in DEFAULT_IGNORE_PATTERNS {
        let _ = builder.add_line(None, pattern);
    }
}

/// Helper function to pack a folder with default configuration
pub async fn pack_folder(folder_path: &Path, s3_client: &S3Client, s3_key: &str) -> Result<String> {
    pack_folder_to_s3(folder_path, s3_client, s3_key, None).await
}

/// Helper function to unpack an archive without hash verification
pub async fn unpack_archive(
    s3_client: &S3Client,
    s3_key: &str,
    target_folder: &Path,
) -> Result<String> {
    unpack_from_s3(s3_client, s3_key, target_folder, None).await
}

/// Unpack a local compressed tar archive file to a folder
pub fn unpack_local_archive(archive_path: &Path, target_folder: &Path) -> Result<()> {
    // Read the archive file
    let archive_data = fs::read(archive_path)?;
    
    // Extract the compressed archive
    extract_compressed_archive(&archive_data, target_folder)?;
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_default_config() {
        let config = ArchiveConfig::default();
        assert_eq!(config.compression_level, DEFAULT_COMPRESSION_LEVEL);
        assert_eq!(config.follow_symlinks, DEFAULT_FOLLOW_SYMLINKS);
        assert_eq!(config.max_size, MAX_COMPRESSED_ARCHIVE_SIZE as u64);
    }

    #[tokio::test]
    async fn test_archive_and_extract() -> Result<()> {
        // Create a temporary directory with some files
        let temp_dir = TempDir::new()?;
        let source_dir = temp_dir.path().join("source");
        fs::create_dir(&source_dir)?;

        // Create test files
        fs::write(source_dir.join("file1.txt"), "Hello, World!")?;
        fs::write(source_dir.join("file2.txt"), "Test content")?;

        // Create subdirectory with files
        let sub_dir = source_dir.join("subdir");
        fs::create_dir(&sub_dir)?;
        fs::write(sub_dir.join("file3.txt"), "Nested file")?;

        // Create .gitignore file
        fs::write(source_dir.join(".gitignore"), "*.log\ntemp/\n")?;

        // Create files that should be ignored
        fs::write(source_dir.join("test.log"), "Should be ignored")?;
        let temp_dir_path = source_dir.join("temp");
        fs::create_dir(&temp_dir_path)?;
        fs::write(temp_dir_path.join("ignored.txt"), "Should be ignored")?;

        // Create archive
        let config = ArchiveConfig::default();
        let archive_data = create_compressed_archive(&source_dir, &config)?;

        // Extract to a new directory
        let target_dir = temp_dir.path().join("target");
        extract_compressed_archive(&archive_data, &target_dir)?;

        // Verify extracted files
        assert!(target_dir.join("file1.txt").exists());
        assert!(target_dir.join("file2.txt").exists());
        assert!(target_dir.join("subdir/file3.txt").exists());

        // Verify ignored files are not extracted
        assert!(!target_dir.join("test.log").exists());
        assert!(!target_dir.join("temp").exists());

        // Verify content
        let content1 = fs::read_to_string(target_dir.join("file1.txt"))?;
        assert_eq!(content1, "Hello, World!");

        let content2 = fs::read_to_string(target_dir.join("file2.txt"))?;
        assert_eq!(content2, "Test content");

        let content3 = fs::read_to_string(target_dir.join("subdir/file3.txt"))?;
        assert_eq!(content3, "Nested file");

        Ok(())
    }

    #[test]
    fn test_gitignore_patterns() {
        let temp_dir = TempDir::new().unwrap();
        let mut builder = GitignoreBuilder::new(temp_dir.path());

        add_default_ignore_patterns(&mut builder);
        let gitignore = builder.build().unwrap();

        // Test default patterns
        assert!(gitignore.matched(Path::new(".git"), true).is_ignore());
        assert!(
            gitignore
                .matched(Path::new("node_modules"), true)
                .is_ignore()
        );
        assert!(gitignore.matched(Path::new("file.swp"), false).is_ignore());
        assert!(gitignore.matched(Path::new(".DS_Store"), false).is_ignore());
        assert!(gitignore.matched(Path::new(".env"), false).is_ignore());

        // Test non-ignored files
        assert!(!gitignore.matched(Path::new("main.rs"), false).is_ignore());
        assert!(
            !gitignore
                .matched(Path::new("Cargo.toml"), false)
                .is_ignore()
        );
    }
}
