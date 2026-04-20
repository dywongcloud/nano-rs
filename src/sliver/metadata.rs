//! Sliver Metadata Structure
//!
//! Defines the metadata for a sliver snapshot including
//! hostname, timestamps, versioning, and optional description.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::sliver::format::FORMAT_VERSION;

/// Metadata for a sliver snapshot
///
/// This structure is serialized to JSON and stored as meta.json
/// in the sliver archive. It provides identifying information and
/// context about the snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SliverMetadata {
    /// Hostname of the app this snapshot represents
    ///
    /// This corresponds to the virtual host configured in NANO
    /// and is used for routing requests to the correct isolate.
    pub hostname: String,

    /// UTC timestamp when the snapshot was created
    ///
    /// Format: ISO 8601 (e.g., "2026-04-20T12:34:56Z")
    pub created_at: DateTime<Utc>,

    /// Sliver format version
    ///
    /// Indicates which version of the sliver specification was used.
    /// Current version is "1.0".
    pub format_version: String,

    /// NANO runtime version that created this snapshot
    ///
    /// The runtime version can affect snapshot compatibility due to
    /// V8 version differences, though heap.bin is treated as opaque.
    pub nano_version: String,

    /// Optional human-readable description
    ///
    /// Can include deployment tags, version notes, or other context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Optional custom metadata fields
    ///
    /// Additional key-value pairs for application-specific metadata.
    /// These are preserved during roundtrip but not interpreted by NANO.
    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub custom: HashMap<String, String>,
}

impl SliverMetadata {
    /// Create new metadata with current timestamp
    ///
    /// # Arguments
    /// * `hostname` - The virtual hostname for this app
    /// * `nano_version` - The NANO runtime version string
    pub fn new(hostname: impl Into<String>, nano_version: impl Into<String>) -> Self {
        Self {
            hostname: hostname.into(),
            created_at: Utc::now(),
            format_version: FORMAT_VERSION.to_string(),
            nano_version: nano_version.into(),
            description: None,
            custom: HashMap::new(),
        }
    }

    /// Create metadata with a description
    pub fn with_description(
        hostname: impl Into<String>,
        nano_version: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        let mut meta = Self::new(hostname, nano_version);
        meta.description = Some(description.into());
        meta
    }

    /// Add a custom metadata field
    pub fn with_custom(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.custom.insert(key.into(), value.into());
        self
    }

    /// Serialize to JSON bytes
    pub fn to_json(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec_pretty(self)
    }

    /// Deserialize from JSON bytes
    pub fn from_json(data: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(data)
    }

    /// Generate a human-readable summary
    pub fn summary(&self) -> String {
        let mut lines = vec![
            format!("Hostname: {}", self.hostname),
            format!("Created: {}", self.created_at.to_rfc3339()),
            format!("Format Version: {}", self.format_version),
            format!("NANO Version: {}", self.nano_version),
        ];

        if let Some(desc) = &self.description {
            lines.push(format!("Description: {}", desc));
        }

        if !self.custom.is_empty() {
            lines.push("Custom Metadata:".to_string());
            for (k, v) in &self.custom {
                lines.push(format!("  {}: {}", k, v));
            }
        }

        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata_creation() {
        let meta = SliverMetadata::new("app.example.com", "1.1.0");
        assert_eq!(meta.hostname, "app.example.com");
        assert_eq!(meta.format_version, "1.0");
        assert_eq!(meta.nano_version, "1.1.0");
        assert!(meta.description.is_none());
    }

    #[test]
    fn test_metadata_with_description() {
        let meta = SliverMetadata::with_description("app.example.com", "1.1.0", "Test snapshot");
        assert_eq!(meta.description, Some("Test snapshot".to_string()));
    }

    #[test]
    fn test_metadata_custom_fields() {
        let meta = SliverMetadata::new("app.example.com", "1.1.0")
            .with_custom("deployment", "production")
            .with_custom("version", "v2.3.1");

        assert_eq!(meta.custom.get("deployment"), Some(&"production".to_string()));
        assert_eq!(meta.custom.get("version"), Some(&"v2.3.1".to_string()));
    }

    #[test]
    fn test_metadata_json_roundtrip() {
        let original = SliverMetadata::with_description("app.example.com", "1.1.0", "Test")
            .with_custom("key", "value");

        let json = original.to_json().unwrap();
        let restored = SliverMetadata::from_json(&json).unwrap();

        assert_eq!(original.hostname, restored.hostname);
        assert_eq!(original.format_version, restored.format_version);
        assert_eq!(original.nano_version, restored.nano_version);
        assert_eq!(original.description, restored.description);
        assert_eq!(original.custom, restored.custom);
        // Timestamps should match exactly
        assert_eq!(original.created_at, restored.created_at);
    }

    #[test]
    fn test_metadata_summary() {
        let meta = SliverMetadata::with_description("app.example.com", "1.1.0", "Test snapshot")
            .with_custom("env", "staging");

        let summary = meta.summary();
        assert!(summary.contains("Hostname: app.example.com"));
        assert!(summary.contains("NANO Version: 1.1.0"));
        assert!(summary.contains("Description: Test snapshot"));
        assert!(summary.contains("env: staging"));
    }

    #[test]
    fn test_metadata_minimal_serialization() {
        // Test that optional fields are skipped when empty
        let meta = SliverMetadata::new("app.example.com", "1.1.0");
        let json = serde_json::to_string(&meta).unwrap();
        
        // Should not contain description or custom fields when empty
        assert!(!json.contains("description"));
        assert!(!json.contains("custom"));
    }
}
