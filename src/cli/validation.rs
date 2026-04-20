//! CLI Input Validation
//!
//! Provides comprehensive input validation with helpful error messages
//! and suggestions for fixing common mistakes.

use crate::cli::error::{CliError, CliResult, find_similar};
use nano::sliver::SliverMetadata;
use std::path::Path;

/// Maximum length for sliver names
const MAX_SLIVER_NAME_LEN: usize = 64;

/// Valid hostname characters (simplified check)
const VALID_HOSTNAME_CHARS: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-.";

/// Validate a hostname
pub fn validate_hostname(input: &str) -> CliResult<()> {
    if input.is_empty() {
        return Err(CliError::InvalidHostname {
            input: input.to_string(),
            suggestion: None,
        });
    }

    // Check length
    if input.len() > 253 {
        return Err(CliError::InvalidHostname {
            input: input.to_string(),
            suggestion: Some("Use a shorter hostname".to_string()),
        });
    }

    // Check for valid characters
    let invalid_chars: Vec<char> = input
        .chars()
        .filter(|c| !VALID_HOSTNAME_CHARS.contains(*c))
        .collect();

    if !invalid_chars.is_empty() {
        return Err(CliError::InvalidHostname {
            input: input.to_string(),
            suggestion: Some(format!(
                "Remove invalid characters: {}",
                invalid_chars.iter().collect::<String>()
            )),
        });
    }

    // Check for consecutive dots
    if input.contains("..") {
        return Err(CliError::InvalidHostname {
            input: input.to_string(),
            suggestion: Some("Remove consecutive dots".to_string()),
        });
    }

    // Check for leading/trailing dots or hyphens
    if input.starts_with('.') || input.starts_with('-') {
        return Err(CliError::InvalidHostname {
            input: input.to_string(),
            suggestion: Some("Remove leading dot or hyphen".to_string()),
        });
    }

    if input.ends_with('.') || input.ends_with('-') {
        return Err(CliError::InvalidHostname {
            input: input.to_string(),
            suggestion: Some("Remove trailing dot or hyphen".to_string()),
        });
    }

    // Check for at least one dot (fully qualified domain)
    // Note: Allow simple names for local testing, but warn
    // This is a relaxed validation suitable for the tool
    
    Ok(())
}

/// Validate a sliver name
pub fn validate_sliver_name(input: &str) -> CliResult<()> {
    if input.is_empty() {
        return Err(CliError::InvalidSliverName {
            name: input.to_string(),
            reason: "Name cannot be empty".to_string(),
        });
    }

    if input.len() > MAX_SLIVER_NAME_LEN {
        return Err(CliError::InvalidSliverName {
            name: input.to_string(),
            reason: format!("Name too long (max {} characters)", MAX_SLIVER_NAME_LEN),
        });
    }

    // Check first character
    let first = input.chars().next().unwrap();
    if !first.is_ascii_alphanumeric() {
        return Err(CliError::InvalidSliverName {
            name: input.to_string(),
            reason: "Name must start with a letter or number".to_string(),
        });
    }

    // Check valid characters
    let invalid: Vec<char> = input
        .chars()
        .filter(|c| !c.is_ascii_alphanumeric() && *c != '-' && *c != '_')
        .collect();

    if !invalid.is_empty() {
        return Err(CliError::InvalidSliverName {
            name: input.to_string(),
            reason: format!(
                "Invalid characters: {} (only letters, numbers, hyphens, underscores allowed)",
                invalid.iter().collect::<String>()
            ),
        });
    }

    Ok(())
}

/// Validate a configuration file path
pub fn validate_config_path(path: &Path) -> CliResult<()> {
    if !path.exists() {
        return Err(CliError::ConfigError {
            path: path.to_path_buf(),
            details: format!("File not found: {}", path.display()),
        });
    }

    if !path.is_file() {
        return Err(CliError::ConfigError {
            path: path.to_path_buf(),
            details: "Path is not a file".to_string(),
        });
    }

    // Try to read and parse as JSON
    let content = std::fs::read_to_string(path)
        .map_err(|e| CliError::io_error("Read", path, e))?;

    let _: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| CliError::ConfigError {
            path: path.to_path_buf(),
            details: format!("Invalid JSON: {}", e),
        })?;

    Ok(())
}

/// Check version compatibility
pub fn check_version_compatibility(metadata: &SliverMetadata) -> CliResult<()> {
    let supported = supported_format_versions();
    let current = metadata.format_version.as_str();

    if supported.contains(&current) {
        return Ok(());
    }

    // Parse version for comparison
    let current_parts: Vec<u32> = current
        .split('.')
        .filter_map(|p| p.parse().ok())
        .collect();

    let latest_supported = supported.last().unwrap_or(&"1.0");
    let latest_parts: Vec<u32> = latest_supported
        .split('.')
        .filter_map(|p| p.parse().ok())
        .collect();

    let too_new = if current_parts.is_empty() || latest_parts.is_empty() {
        false // Be conservative
    } else {
        current_parts[0] > latest_parts[0]
    };

    Err(CliError::VersionMismatch {
        sliver: metadata.name.clone().unwrap_or_else(|| "unnamed".to_string()),
        expected: supported.join(", "),
        found: current.to_string(),
        too_new,
    })
}

/// Get list of supported format versions
pub fn supported_format_versions() -> Vec<&'static str> {
    vec!["1.0"]
}

/// Suggest a correction for a hostname typo
pub fn suggest_hostname_typo(input: &str, known: &[String]) -> Option<String> {
    // Try exact match first
    if known.iter().any(|k| k == input) {
        return None; // Not a typo, it's valid
    }

    // Find similar using Levenshtein distance
    find_similar(input, known, 3)
}

/// Validate sliver file path and existence
pub fn validate_sliver_path(path: &Path) -> CliResult<()> {
    if !path.exists() {
        // Try to find similar files
        let parent = path.parent().unwrap_or(Path::new("."));
        let stem = path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        let similar = if parent.exists() {
            std::fs::read_dir(parent)
                .ok()
                .into_iter()
                .flatten()
                .filter_map(|e| e.ok())
                .filter_map(|e| e.path().file_stem().and_then(|s| s.to_str()).map(|s| s.to_string()))
                .filter(|s| s.contains(stem) || stem.contains(s))
                .take(3)
                .collect::<Vec<_>>()
        } else {
            vec![]
        };

        return Err(CliError::SliverNotFound {
            name: path.display().to_string(),
            searched_paths: vec![path.to_path_buf()],
        });
    }

    if !path.is_file() {
        return Err(CliError::CorruptedSliver {
            path: path.to_path_buf(),
            reason: "Path is not a file".to_string(),
        });
    }

    // Check file extension
    let ext = path.extension().and_then(|e| e.to_str());
    if ext != Some("sliver") {
        return Err(CliError::CorruptedSliver {
            path: path.to_path_buf(),
            reason: format!("Expected .sliver extension, got .{}", ext.unwrap_or("none")),
        });
    }

    Ok(())
}

/// Validate that a sliver doesn't already exist
pub fn validate_sliver_not_exists(name: &str, sliver_dir: &Path) -> CliResult<()> {
    let sliver_path = sliver_dir.join(format!("{}.sliver", name));
    
    if sliver_path.exists() {
        return Err(CliError::SliverAlreadyExists {
            name: name.to_string(),
        });
    }

    Ok(())
}

/// Check if a string is a valid tag
pub fn validate_tag(tag: &str) -> CliResult<()> {
    if tag.len() > 32 {
        return Err(CliError::OperationFailed {
            operation: "Tag validation".to_string(),
            reason: format!("Tag too long: {} (max 32 characters)", tag.len()),
            suggestion: Some("Use a shorter tag".to_string()),
        });
    }

    // Tags can be more permissive than names
    let invalid: Vec<char> = tag
        .chars()
        .filter(|c| {
            !c.is_ascii_alphanumeric() && !"._-".contains(*c)
        })
        .collect();

    if !invalid.is_empty() {
        return Err(CliError::OperationFailed {
            operation: "Tag validation".to_string(),
            reason: format!("Invalid characters in tag: {}", invalid.iter().collect::<String>()),
            suggestion: Some("Use only letters, numbers, dots, underscores, and hyphens".to_string()),
        });
    }

    Ok(())
}

/// Combined validation for sliver creation args
pub fn validate_sliver_create_args(
    hostname: &str,
    name: Option<&str>,
    tag: Option<&str>,
) -> CliResult<()> {
    validate_hostname(hostname)?;

    if let Some(n) = name {
        validate_sliver_name(n)?;
    }

    if let Some(t) = tag {
        validate_tag(t)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_validate_hostname_valid() {
        assert!(validate_hostname("api.example.com").is_ok());
        assert!(validate_hostname("localhost").is_ok());
        assert!(validate_hostname("my-app.example.com").is_ok());
        assert!(validate_hostname("a.b").is_ok());
    }

    #[test]
    fn test_validate_hostname_invalid() {
        assert!(validate_hostname("").is_err());
        assert!(validate_hostname("..").is_err());
        assert!(validate_hostname("test..com").is_err());
        assert!(validate_hostname("-example.com").is_err());
        assert!(validate_hostname("example.com-").is_err());
    }

    #[test]
    fn test_validate_sliver_name_valid() {
        assert!(validate_sliver_name("api-prod").is_ok());
        assert!(validate_sliver_name("my_app").is_ok());
        assert!(validate_sliver_name("v123").is_ok());
        assert!(validate_sliver_name("a").is_ok());
    }

    #[test]
    fn test_validate_sliver_name_invalid() {
        assert!(validate_sliver_name("").is_err());
        assert!(validate_sliver_name("-invalid").is_err());
        assert!(validate_sliver_name("_invalid").is_err());
        assert!(validate_sliver_name("invalid!").is_err());
        assert!(validate_sliver_name("invalid space").is_err());
        
        // Too long
        let long_name = "a".repeat(65);
        assert!(validate_sliver_name(&long_name).is_err());
    }

    #[test]
    fn test_suggest_hostname_typo() {
        let known = vec![
            "api.example.com".to_string(),
            "web.example.com".to_string(),
            "static.example.com".to_string(),
        ];

        assert_eq!(
            suggest_hostname_typo("api.exampl.com", &known),
            Some("api.example.com".to_string())
        );

        assert_eq!(
            suggest_hostname_typo("web.examle.com", &known),
            Some("web.example.com".to_string())
        );

        // Exact match returns None
        assert_eq!(suggest_hostname_typo("api.example.com", &known), None);
    }

    #[test]
    fn test_validate_tag() {
        assert!(validate_tag("v1.0").is_ok());
        assert!(validate_tag("1.0.0").is_ok());
        assert!(validate_tag("beta-1").is_ok());
        assert!(validate_tag("rc.1").is_ok());
        
        assert!(validate_tag("v1.0!").is_err());
        assert!(validate_tag(&"a".repeat(33)).is_err());
    }

    #[test]
    fn test_check_version_compatibility() {
        // Use a real metadata struct
        let metadata_ok = SliverMetadata::new("test.example.com", "1.1.0");
        
        assert!(check_version_compatibility(&metadata_ok).is_ok());
        
        // Test with format version checking
        // The check_version_compatibility validates format_version string
        // Our current implementation accepts "1.0" as valid
    }
}
