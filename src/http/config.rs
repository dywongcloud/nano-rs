//! HTTP server configuration
//!
//! Provides configuration types for the HTTP server including
//! port binding, host address, and environment variable loading.

use anyhow::{Context, Result};
use std::net::SocketAddr;

/// Server configuration for the HTTP listener
///
/// Controls how the HTTP server binds to network interfaces including
/// the port number and bind address. Supports loading from environment
/// variables for deployment flexibility.
///
/// # Examples
///
/// ```rust
/// use nano::http::ServerConfig;
///
/// let config = ServerConfig::default();
/// assert_eq!(config.port, 8080);
/// assert_eq!(config.host, "0.0.0.0");
/// ```
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Port to bind the HTTP server to
    ///
    /// Default: 8080
    /// Environment variable: `NANO_PORT`
    pub port: u16,

    /// Host address to bind to
    ///
    /// Use "0.0.0.0" to listen on all interfaces,
    /// or "127.0.0.1" for localhost only.
    ///
    /// Default: "0.0.0.0"
    /// Environment variable: `NANO_HOST`
    pub host: String,
}

impl Default for ServerConfig {
    /// Creates a default server configuration
    ///
    /// Uses port 8080 and binds to all interfaces (0.0.0.0).
    /// These defaults are suitable for containerized deployments
    /// and local development.
    fn default() -> Self {
        Self {
            port: 8080,
            host: "0.0.0.0".to_string(),
        }
    }
}

impl ServerConfig {
    /// Parse the host and port into a SocketAddr
    ///
    /// # Returns
    ///
    /// A `SocketAddr` suitable for binding with `TcpListener::bind()`.
    ///
    /// # Errors
    ///
    /// Returns an error if the host string cannot be parsed as an IP address.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use nano::http::ServerConfig;
    ///
    /// let config = ServerConfig::default();
    /// let addr = config.socket_addr().unwrap();
    /// assert_eq!(addr.port(), 8080);
    /// ```
    pub fn socket_addr(&self) -> Result<SocketAddr> {
        // Handle IPv6 addresses which need bracket notation
        let addr_str = if self.host.contains(':') {
            // IPv6 address - wrap in brackets
            format!("[{}]:{}", self.host, self.port)
        } else {
            // IPv4 address or hostname
            format!("{}:{}", self.host, self.port)
        };
        addr_str
            .parse::<SocketAddr>()
            .with_context(|| format!("Failed to parse socket address: {}", addr_str))
    }

    /// Load configuration from environment variables
    ///
    /// Reads `NANO_PORT` and `NANO_HOST` from the environment,
    /// falling back to defaults for any unset variables.
    ///
    /// # Environment Variables
    ///
    /// - `NANO_PORT` - Port number (default: 8080)
    /// - `NANO_HOST` - Bind address (default: "0.0.0.0")
    ///
    /// # Errors
    ///
    /// Returns an error if `NANO_PORT` is set but cannot be parsed as a u16.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use nano::http::ServerConfig;
    ///
    /// // With no env vars set, uses defaults
    /// let config = ServerConfig::from_env().unwrap();
    /// assert_eq!(config.port, 8080);
    /// ```
    pub fn from_env() -> Result<Self> {
        let port = std::env::var("NANO_PORT")
            .ok()
            .map(|s| {
                s.parse::<u16>()
                    .with_context(|| format!("NANO_PORT must be a valid port number, got: {}", s))
            })
            .transpose()?
            .unwrap_or(8080);

        let host = std::env::var("NANO_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());

        Ok(Self { port, host })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ServerConfig::default();
        assert_eq!(config.port, 8080);
        assert_eq!(config.host, "0.0.0.0");
    }

    #[test]
    fn test_socket_addr_parsing() {
        let config = ServerConfig::default();
        let addr = config.socket_addr().unwrap();
        assert_eq!(addr.port(), 8080);
        assert!(addr.is_ipv4());
    }

    #[test]
    fn test_socket_addr_ipv6() {
        let config = ServerConfig {
            port: 9090,
            host: "::1".to_string(),
        };
        let addr = config.socket_addr().unwrap();
        assert_eq!(addr.port(), 9090);
        assert!(addr.is_ipv6());
    }

    #[test]
    fn test_from_env_defaults() {
        // Clear environment variables to test defaults
        // This test may fail if env vars are set in the test environment
        let config = ServerConfig::from_env().unwrap();
        assert_eq!(config.port, 8080);
        assert_eq!(config.host, "0.0.0.0");
    }

    #[test]
    fn test_config_clone() {
        let config = ServerConfig::default();
        let cloned = config.clone();
        assert_eq!(config.port, cloned.port);
        assert_eq!(config.host, cloned.host);
    }
}
