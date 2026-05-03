//! CLI Error Types
//!
//! Defines human-readable error types for CLI operations with helpful
//! suggestions for fixing common issues.

use std::io;
use std::path::PathBuf;
use thiserror::Error;

/// CLI-specific errors with human-readable messages
#[derive(Error, Debug)]
pub enum CliError {
    /// Sliver file not found with search path information
    #[error("✗ Sliver '{}' not found\n\nSearched in:\n{}\n\nCreate it with: nano-rs sliver create <hostname> --name {}", 
        name, 
        searched_paths.iter().map(|p| format!("  • {}", p.display())).collect::<Vec<_>>().join("\n"),
        name)]
    SliverNotFound {
        name: String,
        searched_paths: Vec<PathBuf>,
    },

    /// Sliver not found with suggestion for similar name
    #[error("✗ Sliver '{}' not found\n\nDid you mean: {}?\n\nSearched in:\n{}\n\nList all slivers with: nano-rs sliver list", 
        name,
        suggestion,
        searched_paths.iter().map(|p| format!("  • {}", p.display())).collect::<Vec<_>>().join("\n"))]
    SliverNotFoundWithSuggestion {
        name: String,
        suggestion: String,
        searched_paths: Vec<PathBuf>,
    },

    /// Corrupted or invalid sliver file
    #[error("✗ Corrupted sliver file: {path}\n\nReason: {reason}\n\nThe file may be incomplete or damaged. Try recreating the sliver.")]
    CorruptedSliver {
        path: PathBuf,
        reason: String,
    },

    /// Version mismatch between sliver and runtime
    #[error("✗ Version mismatch in sliver '{sliver}'\n\nSliver format version: {found}\nSupported versions: {expected}\n\n{}", 
        if *too_new { 
            "This sliver was created with a newer version of nano-rs.\nPlease upgrade nano-rs to use this sliver."
        } else {
            "This sliver format is outdated.\nPlease recreate this sliver with the current version."
        })]
    VersionMismatch {
        sliver: String,
        expected: String,
        found: String,
        too_new: bool,
    },

    /// Snapshot restoration failed
    #[error("✗ Failed to restore snapshot for sliver '{sliver}'\n\nReason: {reason}")]
    SnapshotRestoreFailed {
        sliver: String,
        reason: String,
    },

    /// Invalid hostname provided
    #[error("✗ Invalid hostname: '{input}'\n{}\n\nHostnames must be valid domain names (e.g., 'api.example.com')", 
        suggestion.as_ref().map(|s| format!("\nDid you mean: {}?", s)).unwrap_or_default())]
    InvalidHostname {
        input: String,
        suggestion: Option<String>,
    },

    /// Invalid sliver name
    #[error("✗ Invalid sliver name: '{name}'\n\nReason: {reason}\n\nSliver names must:\n  • Use only letters, numbers, hyphens, and underscores\n  • Be 1-64 characters long\n  • Start with a letter or number")]
    InvalidSliverName {
        name: String,
        reason: String,
    },

    /// Configuration file error
    #[error("✗ Configuration error: {path}\n\nDetails: {details}\n\nCheck the configuration file format and required fields.")]
    ConfigError {
        path: PathBuf,
        details: String,
    },

    /// IO error with context
    #[error("✗ {operation} failed: {path}\n\nReason: {source}")]
    IoError {
        operation: String,
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// V8 snapshot compatibility error
    #[error("✗ V8 snapshot compatibility error\n\nRuntime V8: {runtime_version}\nSliver V8: {snapshot_version}\n\n{}",
        if *major_mismatch {
            "Major V8 version mismatch. The sliver must be recreated."
        } else {
            "Minor version difference detected. The sliver may work but could have subtle issues."
        })]
    V8VersionMismatch {
        runtime_version: String,
        snapshot_version: String,
        major_mismatch: bool,
    },

    /// Hostname not found in configuration
    #[error("✗ Hostname '{hostname}' not found in configuration\n\nConfiguration file: {config_path}\n\nAdd this hostname to your configuration or check for typos.\n\nAvailable hostnames:\n{}",
        available.join("\n").lines().map(|l| format!("  • {}", l)).collect::<Vec<_>>().join("\n"))]
    HostnameNotFound {
        hostname: String,
        config_path: PathBuf,
        available: Vec<String>,
    },

    /// Sliver already exists
    #[error("✗ Sliver '{name}' already exists\n\nUse --force to overwrite or choose a different name.")]
    SliverAlreadyExists {
        name: String,
    },

    /// Generic operation error with suggestion
    #[error("✗ {operation} failed\n\nReason: {reason}\n\n{}", 
        suggestion.as_ref().unwrap_or(&"Check the error details and try again.".to_string()))]
    OperationFailed {
        operation: String,
        reason: String,
        suggestion: Option<String>,
    },
}

impl CliError {
    // TODO: Re-enable helper constructors when CLI integration is complete
    // These helpers are intentionally disabled to avoid unused code warnings.
    // They will be restored when the full CLI validation flow is implemented.

    /*
    /// Create a sliver not found error with automatic path searching
    pub fn sliver_not_found(name: impl Into<String>, search_paths: Vec<PathBuf>) -> Self {
        let name = name.into();
        CliError::SliverNotFound {
            name,
            searched_paths: search_paths,
        }
    }

    /// Create a corrupted sliver error
    pub fn corrupted_sliver(path: impl Into<PathBuf>, reason: impl Into<String>) -> Self {
        CliError::CorruptedSliver {
            path: path.into(),
            reason: reason.into(),
        }
    }

    /// Create an IO error with context
    pub fn io_error(operation: impl Into<String>, path: impl Into<PathBuf>, source: io::Error) -> Self {
        CliError::IoError {
            operation: operation.into(),
            path: path.into(),
            source,
        }
    }

    /// Returns true if this error has a suggestion for the user
    pub fn has_suggestion(&self) -> bool {
        matches!(self,
            CliError::SliverNotFoundWithSuggestion { .. }
            | CliError::InvalidHostname { suggestion: Some(_), .. }
            | CliError::OperationFailed { suggestion: Some(_), .. }
            | CliError::HostnameNotFound { .. }
        )
    }
    */
}

/// Result type alias for CLI operations
pub type CliResult<T> = Result<T, CliError>;

// TODO: Re-enable similarity search when CLI typo suggestions are implemented
// These functions are disabled to avoid unused code warnings.
// They provide Levenshtein distance for typo correction suggestions.

/*
/// Helper to find similar strings using Levenshtein distance
pub fn find_similar(target: &str, candidates: &[String], threshold: usize) -> Option<String> {
    let mut best_match: Option<(String, usize)> = None;

    for candidate in candidates {
        let distance = levenshtein_distance(target, candidate);
        if distance <= threshold && distance < target.len() {
            if best_match.as_ref().map_or(true, |(_, d)| distance < *d) {
                best_match = Some((candidate.clone(), distance));
            }
        }
    }

    best_match.map(|(s, _)| s)
}

/// Calculate Levenshtein distance between two strings
fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_len = a.chars().count();
    let b_len = b.chars().count();

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut matrix = vec![vec![0; b_len + 1]; a_len + 1];

    for i in 0..=a_len {
        matrix[i][0] = i;
    }
    for j in 0..=b_len {
        matrix[0][j] = j;
    }

    for (i, a_char) in a.chars().enumerate() {
        for (j, b_char) in b.chars().enumerate() {
            let cost = if a_char == b_char { 0 } else { 1 };
            matrix[i + 1][j + 1] = (matrix[i][j + 1] + 1)
                .min(matrix[i + 1][j] + 1)
                .min(matrix[i][j] + cost);
        }
    }

    matrix[a_len][b_len]
}
*/

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = CliError::sliver_not_found("my-app", vec![
            PathBuf::from("./my-app.sliver"),
            PathBuf::from("/home/user/.nano/slivers/my-app.sliver"),
        ]);
        let display = format!("{}", err);
        assert!(display.contains("Sliver 'my-app' not found"));
        assert!(display.contains("Searched in:"));
        assert!(display.contains("Create it with:"));
    }

    #[test]
    fn test_corrupted_sliver_error() {
        let err = CliError::corrupted_sliver("./bad.sliver", "Invalid tar header");
        let display = format!("{}", err);
        assert!(display.contains("Corrupted sliver file"));
        assert!(display.contains("Invalid tar header"));
        assert!(display.contains("Try recreating the sliver"));
    }

    #[test]
    fn test_version_mismatch_error() {
        let err = CliError::VersionMismatch {
            sliver: "test.sliver".to_string(),
            expected: "1.0".to_string(),
            found: "2.0".to_string(),
            too_new: true,
        };
        let display = format!("{}", err);
        assert!(display.contains("Version mismatch"));
        assert!(display.contains("newer version"));
    }

    #[test]
    fn test_levenshtein_distance() {
        assert_eq!(levenshtein_distance("", ""), 0);
        assert_eq!(levenshtein_distance("abc", ""), 3);
        assert_eq!(levenshtein_distance("", "abc"), 3);
        assert_eq!(levenshtein_distance("kitten", "sitting"), 3);
        assert_eq!(levenshtein_distance("saturday", "sunday"), 3);
    }

    #[test]
    fn test_find_similar() {
        let candidates = vec![
            "api-prod".to_string(),
            "api-staging".to_string(),
            "web-app".to_string(),
        ];

        assert_eq!(
            find_similar("api-pro", &candidates, 2),
            Some("api-prod".to_string())
        );

        assert_eq!(
            find_similar("api-stag", &candidates, 3),
            Some("api-staging".to_string())
        );

        assert_eq!(find_similar("completely-different", &candidates, 2), None);
    }

    #[test]
    fn test_has_suggestion() {
        let with_suggestion = CliError::SliverNotFoundWithSuggestion {
            name: "test".to_string(),
            suggestion: "test-app".to_string(),
            searched_paths: vec![],
        };
        assert!(with_suggestion.has_suggestion());

        let without_suggestion = CliError::sliver_not_found("test", vec![]);
        assert!(!without_suggestion.has_suggestion());
    }
}
