//! Application configuration types
//!
//! Defines the core data structures for application configuration including
//! per-app limits, environment variables, and the main AppConfig struct.
//!
//! # Security Considerations
//!
//! - Environment variables are explicitly configured per-app (not entire host env)
//! - Entrypoint paths are validated to prevent directory traversal
//! - Memory limits are bounded between 16-2048 MB to prevent resource exhaustion
//! - Timeouts are bounded between 1-300 seconds
//!
//! # Threat Model Coverage
//!
//! - T-05-02: Only inject explicitly configured env vars, not entire host environment
//! - T-05-04: Entrypoint path validation prevents path traversal attacks

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Environment variables for an application
///
/// Type alias for per-app environment variables. Only variables explicitly
/// configured in the config file are injected into the JS global scope,
/// not the entire host environment (per T-05-02).
pub type AppEnv = HashMap<String, String>;

/// Resource limits for an application
///
/// Defines resource constraints for each hosted application to prevent
/// one app from consuming excessive resources.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AppLimits {
    /// Maximum memory in MB (16-2048, default: 128)
    #[serde(default = "default_memory_mb")]
    pub memory_mb: u32,

    /// Request timeout in seconds (1-300, default: 30)
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u32,

    /// Number of worker threads (1-32, default: 4)
    #[serde(default = "default_workers")]
    pub workers: usize,
}

impl Default for AppLimits {
    fn default() -> Self {
        Self {
            memory_mb: default_memory_mb(),
            timeout_secs: default_timeout_secs(),
            workers: default_workers(),
        }
    }
}

fn default_memory_mb() -> u32 {
    128
}

fn default_timeout_secs() -> u32 {
    30
}

fn default_workers() -> usize {
    4
}

/// VFS backend type selection
///
/// Determines which storage backend is used for this application's
/// virtual file system.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum VfsBackendType {
    /// In-memory storage (default, ephemeral)
    #[default]
    Memory,
    /// Local filesystem persistence
    Disk,
    /// S3-compatible object storage (requires vfs-s3 feature)
    S3,
}

/// Configuration for disk VFS backend
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VfsDiskConfig {
    /// Base directory for file storage
    pub base_path: String,
}

/// Configuration for S3 VFS backend
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VfsS3Config {
    /// S3 endpoint URL (e.g., "https://s3.amazonaws.com" or "http://localhost:9000")
    pub endpoint: String,
    /// S3 bucket name
    pub bucket: String,
    /// AWS region (e.g., "us-east-1")
    pub region: String,
    /// Access key ID
    pub access_key: String,
    /// Secret access key
    pub secret_key: String,
    /// Optional key prefix for all objects
    #[serde(default)]
    pub prefix: Option<String>,
    /// Use path-style URLs (true for MinIO, false for AWS)
    #[serde(default)]
    pub path_style: bool,
}

/// Application configuration for a single hosted app
///
/// Defines all configuration for one application including its hostname,
/// entry point script, environment variables, and resource limits.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    /// Hostname this app responds to (e.g., "api.example.com")
    pub hostname: String,

    /// Path to the entry point JavaScript file
    pub entrypoint: String,

    /// Environment variables to inject into JS global scope (per T-05-02)
    #[serde(default)]
    pub env_vars: AppEnv,

    /// Resource limits for this app
    #[serde(default)]
    pub limits: AppLimits,

    /// VFS backend type (default: memory)
    #[serde(default)]
    pub vfs_backend: VfsBackendType,

    /// Disk backend configuration (required when vfs_backend = disk)
    #[serde(default)]
    pub vfs_disk: Option<VfsDiskConfig>,

    /// S3 backend configuration (required when vfs_backend = s3)
    #[serde(default)]
    pub vfs_s3: Option<VfsS3Config>,
}

/// Server configuration section
///
/// Global server settings for the NANO runtime.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ServerConfigSection {
    /// Port to listen on (default: 8080)
    #[serde(default = "default_port")]
    pub port: u16,

    /// Host address to bind to (default: "0.0.0.0")
    #[serde(default = "default_host")]
    pub host: String,
}

impl Default for ServerConfigSection {
    fn default() -> Self {
        Self {
            port: default_port(),
            host: default_host(),
        }
    }
}

fn default_port() -> u16 {
    8080
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

/// Root configuration structure
///
/// The top-level configuration that defines all applications and server settings.
/// This is loaded from the JSON configuration file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NanoConfig {
    /// List of applications to host
    pub apps: Vec<AppConfig>,

    /// Server configuration
    #[serde(default)]
    pub server: ServerConfigSection,
}

/// Validation errors
///
/// Structured error information for configuration validation failures.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationErrors {
    /// List of individual error messages
    pub errors: Vec<String>,
}

impl ValidationErrors {
    /// Create a new validation errors container
    pub fn new() -> Self {
        Self { errors: Vec::new() }
    }

    /// Add an error message
    pub fn add(&mut self, error: impl Into<String>) {
        self.errors.push(error.into());
    }

    /// Returns true if there are no errors
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }
}

impl Default for ValidationErrors {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ValidationErrors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, error) in self.errors.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "- {}", error)?;
        }
        Ok(())
    }
}

impl std::error::Error for ValidationErrors {}

/// Validates an AppConfig
///
/// Performs comprehensive validation of an application configuration:
/// - Hostname is non-empty and valid DNS name
/// - Entrypoint path exists and is readable
/// - Memory limits within bounds (16-2048 MB)
/// - Timeout within bounds (1-300 seconds)
/// - Worker count within bounds (1-32)
///
/// # Arguments
///
/// * `config` - The AppConfig to validate
/// * `base_path` - Optional base directory for relative entrypoint paths
///
/// # Returns
///
/// `Ok(())` if valid, `Err(ValidationErrors)` with details if invalid
///
/// # Examples
///
/// ```rust
/// use nano::config::{AppConfig, validate_config};
/// use std::path::Path;
///
/// let config = AppConfig {
///     hostname: "api.example.com".to_string(),
///     entrypoint: "/app/index.js".to_string(),
///     env_vars: Default::default(),
///     limits: Default::default(),
/// };
///
/// // Validate without entrypoint existence check (for testing)
/// // In production, you'd pass a base path and validate the file exists
/// ```
pub fn validate_config(
    config: &AppConfig,
    base_path: Option<&std::path::Path>,
) -> Result<(), ValidationErrors> {
    let mut errors = ValidationErrors::new();

    // Validate hostname
    if config.hostname.is_empty() {
        errors.add("hostname cannot be empty");
    } else if !is_valid_hostname(&config.hostname) {
        errors.add(format!("'{}' is not a valid hostname", config.hostname));
    }

    // Validate entrypoint
    if config.entrypoint.is_empty() {
        errors.add("entrypoint cannot be empty");
    } else if config.entrypoint.contains("..") {
        // Path traversal prevention (per T-05-04)
        errors.add(format!(
            "entrypoint '{}' contains '..' which is not allowed for security",
            config.entrypoint
        ));
    } else if let Some(base) = base_path {
        let full_path = if std::path::Path::new(&config.entrypoint).is_absolute() {
            std::path::PathBuf::from(&config.entrypoint)
        } else {
            base.join(&config.entrypoint)
        };

        if !full_path.exists() {
            errors.add(format!(
                "entrypoint '{}' not found (resolved to: {})",
                config.entrypoint,
                full_path.display()
            ));
        } else if !full_path.is_file() {
            errors.add(format!("entrypoint '{}' is not a file", config.entrypoint));
        }
    }

    // Validate limits
    if config.limits.memory_mb < 16 || config.limits.memory_mb > 2048 {
        errors.add(format!(
            "memory_mb must be between 16 and 2048, got {}",
            config.limits.memory_mb
        ));
    }

    if config.limits.timeout_secs < 1 || config.limits.timeout_secs > 300 {
        errors.add(format!(
            "timeout_secs must be between 1 and 300, got {}",
            config.limits.timeout_secs
        ));
    }

    if config.limits.workers < 1 || config.limits.workers > 32 {
        errors.add(format!(
            "workers must be between 1 and 32, got {}",
            config.limits.workers
        ));
    }

    // Validate env vars (per T-05-02: check for suspicious patterns)
    for (key, value) in &config.env_vars {
        // Check for empty keys
        if key.is_empty() {
            errors.add("environment variable key cannot be empty");
        }
        // Check for keys that look like path traversal attempts
        if key.contains("..") || key.contains('/') || key.contains('\\') {
            errors.add(format!("suspicious environment variable key: '{}'", key));
        }
        // Check for overly long values (potential DoS)
        if value.len() > 65536 {
            errors.add(format!(
                "environment variable '{}' value exceeds 64KB limit",
                key
            ));
        }
    }

    // Validate VFS backend configuration
    match config.vfs_backend {
        VfsBackendType::Memory => {
            // Memory backend requires no additional config
        }
        VfsBackendType::Disk => {
            if config.vfs_disk.is_none() {
                errors.add("vfs_backend is 'disk' but vfs_disk configuration is missing");
            } else {
                let disk_config = config.vfs_disk.as_ref().unwrap();
                if disk_config.base_path.is_empty() {
                    errors.add("vfs_disk.base_path cannot be empty");
                } else if disk_config.base_path.contains("..") {
                    errors.add("vfs_disk.base_path contains '..' which is not allowed for security");
                }
            }
        }
        VfsBackendType::S3 => {
            if config.vfs_s3.is_none() {
                errors.add("vfs_backend is 's3' but vfs_s3 configuration is missing");
            } else {
                let s3_config = config.vfs_s3.as_ref().unwrap();
                if s3_config.endpoint.is_empty() {
                    errors.add("vfs_s3.endpoint cannot be empty");
                }
                if s3_config.bucket.is_empty() {
                    errors.add("vfs_s3.bucket cannot be empty");
                }
                if s3_config.access_key.is_empty() {
                    errors.add("vfs_s3.access_key cannot be empty");
                }
                if s3_config.secret_key.is_empty() {
                    errors.add("vfs_s3.secret_key cannot be empty");
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Validates a complete NanoConfig
///
/// Validates the entire configuration including:
/// - At least one app is defined
/// - Maximum 1000 apps (per T-05-03: DoS prevention)
/// - No duplicate hostnames (case-insensitive)
/// - Each app passes individual validation
///
/// # Arguments
///
/// * `config` - The NanoConfig to validate
/// * `base_path` - Optional base directory for entrypoint validation
///
/// # Returns
///
/// `Ok(())` if valid, `Err(ValidationErrors)` with all detected issues
pub fn validate_nano_config(
    config: &NanoConfig,
    base_path: Option<&std::path::Path>,
) -> Result<(), ValidationErrors> {
    let mut errors = ValidationErrors::new();

    // Check app count bounds (per T-05-03)
    if config.apps.is_empty() {
        errors.add("configuration must define at least one application");
    } else if config.apps.len() > 1000 {
        errors.add(format!(
            "too many applications: {} (max 1000)",
            config.apps.len()
        ));
    }

    // Check for duplicate hostnames
    let mut seen_hostnames: std::collections::HashSet<String> = std::collections::HashSet::new();
    for app in &config.apps {
        let lower_hostname = app.hostname.to_lowercase();
        if seen_hostnames.contains(&lower_hostname) {
            errors.add(format!(
                "duplicate hostname: '{}' (case-insensitive)",
                app.hostname
            ));
        } else {
            seen_hostnames.insert(lower_hostname);
        }
    }

    // Validate each app
    for (i, app) in config.apps.iter().enumerate() {
        match validate_config(app, base_path) {
            Ok(()) => {}
            Err(app_errors) => {
                for error in &app_errors.errors {
                    errors.add(format!("app[{}] ({}): {}", i, app.hostname, error));
                }
            }
        }
    }

    // Validate server config
    if config.server.port == 0 {
        errors.add("server port cannot be 0");
    }

    if config.server.host.is_empty() {
        errors.add("server host cannot be empty");
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Checks if a string is a valid hostname
///
/// Validates hostname format per RFC 1123:
/// - Only alphanumeric characters, hyphens, and dots
/// - Each label <= 63 characters
/// - Total length <= 253 characters
/// - Cannot start or end with hyphen
/// - No consecutive dots
fn is_valid_hostname(hostname: &str) -> bool {
    if hostname.is_empty() {
        return false;
    }

    // Check total length
    if hostname.len() > 253 {
        return false;
    }

    // Check each label
    let labels: Vec<&str> = hostname.split('.').collect();
    for label in labels {
        // Empty label (consecutive dots or leading/trailing dot)
        if label.is_empty() {
            return false;
        }

        // Label too long
        if label.len() > 63 {
            return false;
        }

        // Check characters
        let bytes = label.as_bytes();

        // Cannot start or end with hyphen
        if bytes[0] == b'-' || bytes[bytes.len() - 1] == b'-' {
            return false;
        }

        // Only alphanumeric and hyphens allowed
        for &b in bytes {
            if !(b.is_ascii_alphanumeric() || b == b'-') {
                return false;
            }
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_limits_defaults() {
        let limits = AppLimits::default();
        assert_eq!(limits.memory_mb, 128);
        assert_eq!(limits.timeout_secs, 30);
        assert_eq!(limits.workers, 4);
    }

    #[test]
    fn test_app_config_deserialization() {
        let json = r#"{
            "hostname": "api.example.com",
            "entrypoint": "/app/index.js",
            "env_vars": {"API_KEY": "secret123"},
            "limits": {"memory_mb": 256, "timeout_secs": 60, "workers": 8}
        }"#;

        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.hostname, "api.example.com");
        assert_eq!(config.entrypoint, "/app/index.js");
        assert_eq!(
            config.env_vars.get("API_KEY"),
            Some(&"secret123".to_string())
        );
        assert_eq!(config.limits.memory_mb, 256);
        assert_eq!(config.limits.timeout_secs, 60);
        assert_eq!(config.limits.workers, 8);
    }

    #[test]
    fn test_app_config_deserialization_defaults() {
        let json = r#"{
            "hostname": "api.example.com",
            "entrypoint": "/app/index.js"
        }"#;

        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.hostname, "api.example.com");
        assert_eq!(config.entrypoint, "/app/index.js");
        assert!(config.env_vars.is_empty());
        assert_eq!(config.limits.memory_mb, 128); // default
        assert_eq!(config.limits.timeout_secs, 30); // default
        assert_eq!(config.limits.workers, 4); // default
    }

    #[test]
    fn test_validation_rejects_empty_hostname() {
        let config = AppConfig {
            hostname: "".to_string(),
            entrypoint: "/app/index.js".to_string(),
            env_vars: Default::default(),
            limits: Default::default(),
        };

        let result = validate_config(&config, None);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.errors.iter().any(|e| e.contains("hostname")));
    }

    #[test]
    fn test_validation_rejects_empty_entrypoint() {
        let config = AppConfig {
            hostname: "api.example.com".to_string(),
            entrypoint: "".to_string(),
            env_vars: Default::default(),
            limits: Default::default(),
        };

        let result = validate_config(&config, None);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.errors.iter().any(|e| e.contains("entrypoint")));
    }

    #[test]
    fn test_validation_rejects_invalid_hostname() {
        let config = AppConfig {
            hostname: "not a valid hostname!".to_string(),
            entrypoint: "/app/index.js".to_string(),
            env_vars: Default::default(),
            limits: Default::default(),
        };

        let result = validate_config(&config, None);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.errors.iter().any(|e| e.contains("hostname")));
    }

    #[test]
    fn test_validation_rejects_path_traversal() {
        let config = AppConfig {
            hostname: "api.example.com".to_string(),
            entrypoint: "../../../etc/passwd".to_string(),
            env_vars: Default::default(),
            limits: Default::default(),
        };

        let result = validate_config(&config, None);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .errors
            .iter()
            .any(|e| e.contains("..") || e.contains("security")));
    }

    #[test]
    fn test_validation_rejects_invalid_memory() {
        let config = AppConfig {
            hostname: "api.example.com".to_string(),
            entrypoint: "/app/index.js".to_string(),
            env_vars: Default::default(),
            limits: AppLimits {
                memory_mb: 5, // too low
                timeout_secs: 30,
                workers: 4,
            },
        };

        let result = validate_config(&config, None);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.errors.iter().any(|e| e.contains("memory_mb")));
    }

    #[test]
    fn test_validation_rejects_invalid_timeout() {
        let config = AppConfig {
            hostname: "api.example.com".to_string(),
            entrypoint: "/app/index.js".to_string(),
            env_vars: Default::default(),
            limits: AppLimits {
                memory_mb: 128,
                timeout_secs: 0, // too low
                workers: 4,
            },
        };

        let result = validate_config(&config, None);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.errors.iter().any(|e| e.contains("timeout_secs")));
    }

    #[test]
    fn test_validation_rejects_invalid_workers() {
        let config = AppConfig {
            hostname: "api.example.com".to_string(),
            entrypoint: "/app/index.js".to_string(),
            env_vars: Default::default(),
            limits: AppLimits {
                memory_mb: 128,
                timeout_secs: 30,
                workers: 100, // too high
            },
        };

        let result = validate_config(&config, None);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.errors.iter().any(|e| e.contains("workers")));
    }

    #[test]
    fn test_validation_accepts_valid_config() {
        let config = AppConfig {
            hostname: "api.example.com".to_string(),
            entrypoint: "/app/index.js".to_string(),
            env_vars: Default::default(),
            limits: Default::default(),
        };

        let result = validate_config(&config, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_valid_hostname_variations() {
        // Valid hostnames
        assert!(is_valid_hostname("example.com"));
        assert!(is_valid_hostname("api.example.com"));
        assert!(is_valid_hostname("a.b.c.d.example.com"));
        assert!(is_valid_hostname("test-123.example-site.org"));
        assert!(is_valid_hostname("localhost"));

        // Invalid hostnames
        assert!(!is_valid_hostname("")); // empty
        assert!(!is_valid_hostname("-example.com")); // starts with hyphen
        assert!(!is_valid_hostname("example-.com")); // label ends with hyphen
        assert!(!is_valid_hostname("example..com")); // consecutive dots
        assert!(!is_valid_hostname(".example.com")); // starts with dot
        assert!(!is_valid_hostname("example.com.")); // ends with dot (for our purposes)
        assert!(!is_valid_hostname("ex ample.com")); // space
        assert!(!is_valid_hostname("ex_ample.com")); // underscore
    }

    #[test]
    fn test_nano_config_deserialization() {
        let json = r#"{
            "apps": [
                {
                    "hostname": "api.example.com",
                    "entrypoint": "/apps/api/index.js",
                    "env_vars": {"API_KEY": "secret123"},
                    "limits": {"memory_mb": 128, "timeout_secs": 30, "workers": 4}
                },
                {
                    "hostname": "blog.example.com",
                    "entrypoint": "/apps/blog/index.js",
                    "env_vars": {"DB_URL": "localhost"},
                    "limits": {"memory_mb": 64, "timeout_secs": 10, "workers": 2}
                }
            ],
            "server": {"port": 8080, "host": "0.0.0.0"}
        }"#;

        let config: NanoConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.apps.len(), 2);
        assert_eq!(config.server.port, 8080);
        assert_eq!(config.server.host, "0.0.0.0");
    }

    #[test]
    fn test_validate_nano_config_rejects_duplicates() {
        let config = NanoConfig {
            apps: vec![
                AppConfig {
                    hostname: "api.example.com".to_string(),
                    entrypoint: "/app1.js".to_string(),
                    env_vars: Default::default(),
                    limits: Default::default(),
                },
                AppConfig {
                    hostname: "API.EXAMPLE.COM".to_string(), // same as above, different case
                    entrypoint: "/app2.js".to_string(),
                    env_vars: Default::default(),
                    limits: Default::default(),
                },
            ],
            server: Default::default(),
        };

        let result = validate_nano_config(&config, None);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.errors.iter().any(|e| e.contains("duplicate")));
    }

    #[test]
    fn test_validate_nano_config_rejects_empty_apps() {
        let config = NanoConfig {
            apps: vec![],
            server: Default::default(),
        };

        let result = validate_nano_config(&config, None);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.errors.iter().any(|e| e.contains("at least one")));
    }

    #[test]
    fn test_validate_nano_config_rejects_too_many_apps() {
        let mut apps = Vec::new();
        for i in 0..1001 {
            apps.push(AppConfig {
                hostname: format!("app{}.example.com", i),
                entrypoint: "/app.js".to_string(),
                env_vars: Default::default(),
                limits: Default::default(),
            });
        }

        let config = NanoConfig {
            apps,
            server: Default::default(),
        };

        let result = validate_nano_config(&config, None);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.errors.iter().any(|e| e.contains("too many")));
    }

    #[test]
    fn test_validate_nano_config_accepts_valid() {
        let config = NanoConfig {
            apps: vec![AppConfig {
                hostname: "api.example.com".to_string(),
                entrypoint: "/app.js".to_string(),
                env_vars: Default::default(),
                limits: Default::default(),
            }],
            server: Default::default(),
        };

        let result = validate_nano_config(&config, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_deny_unknown_fields() {
        // This should fail to deserialize because of unknown field
        let json = r#"{
            "hostname": "api.example.com",
            "entrypoint": "/app/index.js",
            "unknown_field": "value"
        }"#;

        let result: Result<AppConfig, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_env_var_validation() {
        let mut env_vars = HashMap::new();
        env_vars.insert("../etc/passwd".to_string(), "value".to_string()); // suspicious key

        let config = AppConfig {
            hostname: "api.example.com".to_string(),
            entrypoint: "/app.js".to_string(),
            env_vars,
            limits: Default::default(),
        };

        let result = validate_config(&config, None);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.errors.iter().any(|e| e.contains("suspicious")));
    }

    #[test]
    fn test_vfs_backend_type_default() {
        let backend_type: VfsBackendType = Default::default();
        assert_eq!(backend_type, VfsBackendType::Memory);
    }

    #[test]
    fn test_vfs_backend_type_deserialization() {
        assert_eq!(
            serde_json::from_str::<VfsBackendType>("\"memory\"").unwrap(),
            VfsBackendType::Memory
        );
        assert_eq!(
            serde_json::from_str::<VfsBackendType>("\"disk\"").unwrap(),
            VfsBackendType::Disk
        );
        assert_eq!(
            serde_json::from_str::<VfsBackendType>("\"s3\"").unwrap(),
            VfsBackendType::S3
        );
    }

    #[test]
    fn test_vfs_disk_config_deserialization() {
        let json = r#"{"base_path": "/data/nano"}"#;
        let config: VfsDiskConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.base_path, "/data/nano");
    }

    #[test]
    fn test_vfs_s3_config_deserialization() {
        let json = r#"{
            "endpoint": "http://localhost:9000",
            "bucket": "nano-vfs",
            "region": "us-east-1",
            "access_key": "minioadmin",
            "secret_key": "minioadmin",
            "prefix": "app1",
            "path_style": true
        }"#;
        let config: VfsS3Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.endpoint, "http://localhost:9000");
        assert_eq!(config.bucket, "nano-vfs");
        assert_eq!(config.prefix, Some("app1".to_string()));
        assert!(config.path_style);
    }

    #[test]
    fn test_app_config_with_vfs_disk() {
        let json = r#"{
            "hostname": "api.example.com",
            "entrypoint": "/app/index.js",
            "vfs_backend": "disk",
            "vfs_disk": {
                "base_path": "/data/api"
            }
        }"#;

        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.vfs_backend, VfsBackendType::Disk);
        assert!(config.vfs_disk.is_some());
        assert_eq!(config.vfs_disk.unwrap().base_path, "/data/api");
    }

    #[test]
    fn test_validation_rejects_disk_without_config() {
        let config = AppConfig {
            hostname: "api.example.com".to_string(),
            entrypoint: "/app.js".to_string(),
            env_vars: Default::default(),
            limits: Default::default(),
            vfs_backend: VfsBackendType::Disk,
            vfs_disk: None,
            vfs_s3: None,
        };

        let result = validate_config(&config, None);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.errors.iter().any(|e| e.contains("vfs_disk") && e.contains("missing")));
    }

    #[test]
    fn test_validation_rejects_s3_without_config() {
        let config = AppConfig {
            hostname: "api.example.com".to_string(),
            entrypoint: "/app.js".to_string(),
            env_vars: Default::default(),
            limits: Default::default(),
            vfs_backend: VfsBackendType::S3,
            vfs_disk: None,
            vfs_s3: None,
        };

        let result = validate_config(&config, None);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.errors.iter().any(|e| e.contains("vfs_s3") && e.contains("missing")));
    }

    #[test]
    fn test_validation_rejects_disk_path_traversal() {
        let config = AppConfig {
            hostname: "api.example.com".to_string(),
            entrypoint: "/app.js".to_string(),
            env_vars: Default::default(),
            limits: Default::default(),
            vfs_backend: VfsBackendType::Disk,
            vfs_disk: Some(VfsDiskConfig {
                base_path: "../../../etc/passwd".to_string(),
            }),
            vfs_s3: None,
        };

        let result = validate_config(&config, None);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.errors.iter().any(|e| e.contains("base_path") && e.contains("..")));
    }
}
