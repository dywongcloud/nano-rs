//! Sliver Format Constants and Types
//!
//! Defines the sliver archive format structure and version constants.
//! The format is designed to be simple, portable, and evolvable.

/// Current sliver format version
///
/// This is a semantic version string that identifies the format specification.
/// Future versions may add features but must maintain backward compatibility
/// for basic structure (meta.json, heap.bin, vfs/).
pub const FORMAT_VERSION: &str = "1.0";

/// NANO runtime version for metadata
/// This should be set at build time or from crate version
pub const NANO_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Filename for the V8 heap snapshot blob
///
/// This is an opaque binary blob created by V8's SnapshotCreator API.
/// The contents are version-specific to V8 and should not be parsed.
pub const HEAP_FILENAME: &str = "heap.bin";

/// Filename for JSON metadata
///
/// Contains structured information about the snapshot including
/// hostname, creation time, format version, and description.
pub const METADATA_FILENAME: &str = "meta.json";

/// Filename for human-readable manifest
///
/// A plain text file listing the archive contents for inspection.
/// This is informational only and not used during loading.
pub const MANIFEST_FILENAME: &str = "manifest.txt";

/// Prefix for VFS entries in the archive
///
/// All VFS files are stored under this path prefix in the tar archive.
/// Example: vfs/data/config.json
pub const VFS_PREFIX: &str = "vfs/";

/// The sliver file extension
pub const SLIVER_EXTENSION: &str = ".sliver";

/// Format specification and capabilities
#[derive(Debug, Clone)]
pub struct SliverFormat;

impl SliverFormat {
    /// Get the current format version
    pub fn version() -> &'static str {
        FORMAT_VERSION
    }

    /// Get the NANO runtime version
    pub fn nano_version() -> &'static str {
        NANO_VERSION
    }

    /// Check if a format version is supported
    ///
    /// Currently only "1.0" is supported. Future versions will implement
    /// backward compatibility for reading older formats.
    pub fn is_supported_version(version: &str) -> bool {
        version == FORMAT_VERSION
    }

    /// Get the list of required files in a valid sliver archive
    pub fn required_files() -> &'static [&'static str] {
        &[METADATA_FILENAME, HEAP_FILENAME]
    }

    /// Get the complete archive structure as a string
    ///
    /// This is used for documentation and manifest generation.
    pub fn structure_documentation() -> &'static str {
        r#"Sliver Archive Structure (v1.0)
================================

app-v1.sliver (tar archive)
├── meta.json          # Metadata: hostname, created_at, version
├── heap.bin           # V8 heap snapshot (opaque blob)
├── vfs/               # Virtual filesystem contents
│   ├── data/
│   │   └── config.json
│   └── assets/
│       └── logo.png
└── manifest.txt       # Human-readable manifest

Required Files:
- meta.json: JSON metadata with hostname, timestamps, version info
- heap.bin: Opaque V8 heap snapshot blob

Optional Files:
- manifest.txt: Human-readable listing (generated)
- vfs/*: Virtual filesystem contents

Format Notes:
- Archive is a standard tar file (ustar or GNU format)
- heap.bin is an opaque blob - do not parse its contents
- VFS entries preserve directory structure under vfs/ prefix
- All paths use forward slashes (even on Windows)
- Binary files stored as-is without encoding

Future Extensions:
- Delta/differential snapshots (additive format)
- Compression layer (gzip/brotli)
- Checksum verification per entry
"#
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_version() {
        assert_eq!(SliverFormat::version(), "1.0");
        assert!(SliverFormat::is_supported_version("1.0"));
        assert!(!SliverFormat::is_supported_version("0.9"));
        assert!(!SliverFormat::is_supported_version("2.0"));
    }

    #[test]
    fn test_required_files() {
        let required = SliverFormat::required_files();
        assert!(required.contains(&"meta.json"));
        assert!(required.contains(&"heap.bin"));
    }

    #[test]
    fn test_constants() {
        assert_eq!(HEAP_FILENAME, "heap.bin");
        assert_eq!(METADATA_FILENAME, "meta.json");
        assert_eq!(MANIFEST_FILENAME, "manifest.txt");
        assert_eq!(VFS_PREFIX, "vfs/");
        assert_eq!(SLIVER_EXTENSION, ".sliver");
    }

    #[test]
    fn test_nano_version() {
        // Should match cargo package version
        assert!(!SliverFormat::nano_version().is_empty());
    }
}
