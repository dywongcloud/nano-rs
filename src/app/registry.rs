//! Application registry for multi-app hosting
//!
//! Manages the mapping of hostnames to application configurations
//! and provides lookup functionality for request routing.

use std::collections::HashMap;
use std::sync::Arc;

use crate::config::AppConfig;

/// Registry of all hosted applications
#[derive(Debug, Clone)]
pub struct AppRegistry {
    /// Map of hostnames to application configurations
    apps: Arc<HashMap<String, AppConfig>>,
}

impl AppRegistry {
    /// Create a new registry from a map of apps
    pub fn new(apps: HashMap<String, AppConfig>) -> Self {
        Self {
            apps: Arc::new(apps),
        }
    }

    /// Create registry from config
    pub fn from_config(config: crate::config::NanoConfig) -> Self {
        let apps: HashMap<String, AppConfig> = config
            .apps
            .into_iter()
            .map(|app| (app.hostname.clone(), app))
            .collect();
        Self::new(apps)
    }

    /// Get application configuration by hostname
    pub fn get(&self, hostname: &str) -> Option<AppConfig> {
        self.apps.get(hostname).cloned()
    }

    /// Check if hostname is registered
    pub fn contains(&self, hostname: &str) -> bool {
        self.apps.contains_key(hostname)
    }

    /// Get all registered hostnames
    pub fn all_hostnames(&self) -> impl Iterator<Item = String> + '_ {
        self.apps.keys().cloned()
    }

    /// Get count of registered apps
    pub fn count(&self) -> usize {
        self.apps.len()
    }
}

impl Default for AppRegistry {
    fn default() -> Self {
        Self::new(HashMap::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn create_test_config(hostname: &str) -> AppConfig {
        AppConfig {
            hostname: hostname.to_string(),
            entrypoint: format!("./{}.js", hostname),
            env_vars: HashMap::new(),
            limits: crate::config::AppLimits::default(),
        }
    }

    #[test]
    fn test_registry_get() {
        let mut apps = HashMap::new();
        apps.insert("app1".to_string(), create_test_config("app1"));

        let registry = AppRegistry::new(apps);

        assert!(registry.get("app1").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_registry_contains() {
        let mut apps = HashMap::new();
        apps.insert("app1".to_string(), create_test_config("app1"));

        let registry = AppRegistry::new(apps);

        assert!(registry.contains("app1"));
        assert!(!registry.contains("nonexistent"));
    }

    #[test]
    fn test_registry_all_hostnames() {
        let mut apps = HashMap::new();
        apps.insert("app1".to_string(), create_test_config("app1"));
        apps.insert("app2".to_string(), create_test_config("app2"));

        let registry = AppRegistry::new(apps);
        let hostnames: Vec<_> = registry.all_hostnames().collect();

        assert_eq!(hostnames.len(), 2);
        assert!(hostnames.contains(&"app1".to_string()));
        assert!(hostnames.contains(&"app2".to_string()));
    }
}
