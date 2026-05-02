---
phase: 05-multi-app-hosting
plan: 01
subsystem: config
summary: "JSON config loading, app registry, and per-app environment variables"
dependency_graph:
  requires: ["04-complete"]
  provides: ["multi-app-config", "app-registry"]
tech_stack:
  added: [serde_json]
  patterns: [Registry pattern, Config validation]
key_files:
  created:
    - src/config/app.rs
    - src/config/loader.rs
    - src/config/mod.rs
    - src/app/registry.rs
    - src/app/mod.rs
  modified:
    - src/http/router.rs
    - src/http/server.rs
    - Cargo.toml
decisions:
  - AppConfig uses hostname as unique identifier
  - Environment variables explicitly configured per-app (not host env)
  - Entrypoint paths validated to prevent directory traversal
  - Config loaded at startup, registry built from validated config
metrics:
  duration: "~40 minutes"
  config_structs: 3 (AppConfig, AppLimits, AppEnv)
  validation_rules: 5
  tests: 4
---

## What Was Built

### Configuration Types (src/config/app.rs)
- `AppConfig` — Complete app configuration with hostname, entrypoint, workers, limits, env vars
- `AppLimits` — Memory (MB) and timeout (seconds) constraints
- `AppEnv` — Per-app environment variables (HashMap<String, String>)
- Validation: Memory limits 16-2048 MB, timeouts 1-300 seconds, path traversal prevention

### Config Loader (src/config/loader.rs)
- `load_config()` — Load and parse JSON config file
- `validate_config()` — Schema validation and rule checking
- `ServerConfig` — HTTP server bind address and admin port
- Error handling with detailed validation messages

### App Registry (src/app/registry.rs)
- `AppRegistry` — Thread-safe HashMap<hostname, AppConfig>
- `register_app()` — Add app to registry
- `get_app()` — Lookup by hostname (exact match)
- Support for multiple apps in single config
- Atomic swap for hot-reload support

### HTTP Router Integration
- Router resolves hostnames via `registry.get(hostname)`
- Virtual host dispatch to appropriate WorkerPool
- 404 handler for unknown hostnames

## Verification

### Unit Tests
- Config loading with valid/invalid JSON
- Validation of memory limits (bounds checking)
- Validation of timeout bounds
- Registry hostname lookup

### Integration
- Router integration with registry
- Multi-app config parsing

## Security

- **T-05-02**: Only configured env vars injected (not entire host environment)
- **T-05-04**: Entrypoint path validation prevents `../../etc/passwd` attacks
- Bounds checking prevents resource exhaustion

## Commits
- Part of multi-app hosting implementation
- Config and registry foundation

## Next Steps
- Phase 05-02: Memory limits and timeout enforcement
