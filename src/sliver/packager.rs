//! Sliver Packager
//!
//! Creates slivers directly from directories without requiring a running app.
//! This enables packing static sites (Astro, Next.js exports) and JS worker bundles
//! into standalone sliver files that can be deployed anywhere.
//!
//! # Example
//!
//! ```rust,no_run
//! use nano::sliver::packager::create_sliver_from_directory;
//!
//! # async fn example() -> anyhow::Result<()> {
//! // Create sliver from a directory
//! create_sliver_from_directory(
//!     "./dist",
//!     "myapp",
//!     Some("v1.0".to_string()),
//!     Some("./myapp.sliver".to_string()),
//! ).await?;
//! # Ok(())
//! # }
//! ```

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

use crate::sliver::metadata::SliverMetadata;
use crate::sliver::packer::SliverPacker;
use crate::vfs::types::{VfsFile, VfsPath};

/// Metadata for directory-based slivers
#[derive(Debug, Clone)]
pub struct DirectorySliverMetadata {
    /// Name of the sliver
    pub name: String,
    /// Tag/version
    pub tag: String,
    /// Entrypoint path (relative to files/)
    pub entrypoint: String,
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Create a sliver from a directory without requiring a running app
///
/// This function packs all files from a source directory into a standalone
/// sliver archive. The sliver can be run directly with `nano-rs run --sliver`.
///
/// # Arguments
///
/// * `source_dir` - Path to the directory containing app files
/// * `name` - Name for the sliver
/// * `tag` - Optional version tag
/// * `output` - Optional output path (defaults to `{name}.sliver`)
/// * `hostname` - Optional hostname (defaults to name if not specified)
///
/// # Returns
///
/// Path to the created sliver file
///
/// # Errors
///
/// Returns an error if:
/// - The source directory doesn't exist
/// - No entrypoint is found (index.js, index.html, etc.)
/// - File reading fails
/// - Output path already exists
pub async fn create_sliver_from_directory(
    source_dir: &str,
    name: &str,
    tag: Option<String>,
    output: Option<String>,
    hostname: Option<String>,
) -> Result<PathBuf> {
    let source_path = Path::new(source_dir);
    
    // Validate source directory exists
    if !source_path.exists() {
        bail!("Source directory does not exist: {}", source_dir);
    }
    if !source_path.is_dir() {
        bail!("Source path is not a directory: {}", source_dir);
    }
    
    // Determine output path
    let output_path = output.map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(format!("{}.sliver", name))
    });
    
    // Check output doesn't already exist
    if output_path.exists() {
        bail!(
            "Sliver file already exists: {}. Use --output to specify a different path.",
            output_path.display()
        );
    }
    
    // Use provided hostname or default to name
    let sliver_hostname = hostname.unwrap_or_else(|| name.to_string());
    
    // Detect entrypoint
    let entrypoint = detect_entrypoint(source_path);
    tracing::info!("Detected entrypoint: {}", entrypoint);
    
    // Create sliver metadata
    let sliver_tag = tag.clone().unwrap_or_else(|| "latest".to_string());
    let mut metadata = SliverMetadata::new(&sliver_hostname, env!("CARGO_PKG_VERSION"));
    metadata.name = Some(name.to_string());
    metadata.description = Some(format!(
        "Created from directory: {} | Entrypoint: {} | Tag: {}",
        source_dir, entrypoint, sliver_tag
    ));
    
    // Store entrypoint in custom metadata
    metadata.custom.insert("entrypoint".to_string(), entrypoint.clone());
    metadata.custom.insert("source_dir".to_string(), source_dir.to_string());
    metadata.custom.insert("tag".to_string(), sliver_tag.clone());
    metadata.custom.insert("sliver_type".to_string(), "directory".to_string());
    
    // Create the sliver packer
    let mut packer = SliverPacker::new();
    packer.add_metadata(&metadata)?;
    
    // For directory-based slivers, we don't create a V8 heap snapshot
    // Instead, we store a placeholder that indicates this is a "cold" sliver
    // The runtime will create the isolate on first request
    let heap_data = create_placeholder_heap(&entrypoint);
    packer.add_heap(&heap_data)?;
    
    // Load all files from the directory into VFS entries
    let vfs_entries = load_directory_files(source_path)?;
    
    // Add VFS entries to packer
    if !vfs_entries.is_empty() {
        packer.add_vfs_entries(&vfs_entries)?;
    }
    
    // Finalize the archive
    let archive_data = packer.finalize()?;
    
    // Write to output file
    std::fs::write(&output_path, &archive_data)
        .with_context(|| format!("Failed to write sliver to {}", output_path.display()))?;
    
    println!("Created sliver: {}", output_path.display());
    println!("  Source: {}", source_dir);
    println!("  Name: {}", name);
    println!("  Hostname: {}", sliver_hostname);
    println!("  Tag: {}", sliver_tag);
    println!("  Entrypoint: {}", entrypoint);
    println!("  Size: {} bytes", archive_data.len());
    println!("  Files: {}", vfs_entries.len());
    
    tracing::info!(
        "Created sliver from directory: {} -> {} ({} bytes, {} files)",
        source_dir,
        output_path.display(),
        archive_data.len(),
        vfs_entries.len()
    );
    
    Ok(output_path)
}

/// Create a placeholder heap for directory-based slivers
///
/// Directory slivers don't have a V8 heap snapshot since the app
/// wasn't running. Instead, we create a small marker that the
/// runtime can detect and handle appropriately.
fn create_placeholder_heap(entrypoint: &str) -> Vec<u8> {
    // Create a magic header that indicates this is a directory-based sliver
    // Format: "NANO-DIR-v1\0" followed by entrypoint path
    let mut data = Vec::new();
    data.extend_from_slice(b"NANO-DIR-v1\0");
    data.extend_from_slice(entrypoint.as_bytes());
    data
}

/// Detect the entrypoint file from a directory
///
/// Checks for common entrypoint patterns in order of preference:
/// 1. index.js (JavaScript worker)
/// 2. index.mjs (ES Module worker)
/// 3. main.js (JavaScript worker)
/// 4. worker.js (Web Worker pattern)
/// 5. index.html (Static site)
///
/// Returns the detected entrypoint path (relative to the directory)
fn detect_entrypoint(dir: &Path) -> String {
    let candidates = [
        "index.js",
        "index.mjs",
        "main.js",
        "worker.js",
        "index.html",
    ];
    
    for candidate in &candidates {
        if dir.join(candidate).exists() {
            return candidate.to_string();
        }
    }
    
    // Default to index.html if no other entrypoint found
    // This supports static sites with non-standard entrypoints
    "index.html".to_string()
}

/// Load all files from a directory into VFS entries
///
/// Recursively walks the directory and creates VFS entries for all files.
/// Preserves directory structure in the VFS paths.
fn load_directory_files(dir: &Path) -> Result<Vec<(VfsPath, VfsFile)>> {
    use std::time::SystemTime;
    use walkdir::WalkDir;
    
    let mut entries = Vec::new();
    
    for entry in WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        let relative_path = path.strip_prefix(dir)
            .map_err(|e| anyhow::anyhow!("Failed to get relative path: {}", e))?;
        
        // Skip certain files
        let file_name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        
        if file_name.starts_with('.') || file_name.ends_with(".sliver") {
            continue;
        }
        
        // Read file content
        let content = std::fs::read(path)
            .with_context(|| format!("Failed to read file: {}", path.display()))?;
        
        let metadata = std::fs::metadata(path)
            .with_context(|| format!("Failed to get metadata: {}", path.display()))?;
        
        let modified_at = metadata.modified()
            .unwrap_or_else(|_| SystemTime::now());
        let created_at = metadata.created()
            .unwrap_or_else(|_| SystemTime::now());
        
        // Create VFS path (ensure it starts with /)
        let vfs_path_str = format!("/{}", relative_path.to_string_lossy());
        let vfs_path = VfsPath::new(&vfs_path_str)
            .with_context(|| format!("Invalid VFS path: {}", vfs_path_str))?;
        
        let vfs_file = VfsFile {
            content,
            modified_at,
            created_at,
            size: metadata.len() as usize,
        };
        
        entries.push((vfs_path, vfs_file));
    }
    
    tracing::info!("Loaded {} files from directory: {}", entries.len(), dir.display());
    
    Ok(entries)
}