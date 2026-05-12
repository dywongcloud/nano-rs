# NANO Dependency Audit

**Date:** 2026-05-12  
**Total Direct Dependencies:** 40  
**TigerStyle Target:** Minimize to <10 essential dependencies

## Executive Summary

This audit categorizes all 40 direct dependencies in nano-rs according to TigerStyle principles:
- **Zero dependencies apart from toolchain**
- **Dependencies risk safety and performance**
- **For each dependency, ask: "Can we implement this in <100 lines?"**
- **Prefer vendoring small utilities over external crates**

## Category 1: Essential (Cannot Remove)

| Crate | Version | Purpose | Transitive Deps | Risk | Rationale |
|-------|---------|---------|-----------------|------|-----------|
| v8 | 147 | V8 JavaScript engine | ~5 | Low | Core primitive, pre-built binaries, no alternative |
| tokio | 1.52 | Async runtime | ~20 | Low | Industry standard, required for async I/O |
| anyhow | 1.0 | Error handling | 0 | Low | Small, stable, ergonomic error propagation |
| thiserror | 2.0 | Error derive macros | 0 | Low | Compile-time only, zero runtime cost |
| ring | 0.17 | Cryptographic primitives | ~3 | Low | Used by rustls, well-audited, no good alternative |

**Count: 5 dependencies**

## Category 2: Security-Critical (Review but Keep)

| Crate | Version | Purpose | Transitive Deps | Risk | Rationale |
|-------|---------|---------|-----------------|------|-----------|
| rustls | 0.23 | TLS implementation | ~10 | Low | Modern TLS, ring-based, safer than OpenSSL |
| webpki-roots | 0.26 | Root certificates | 0 | Low | Mozilla root store, required for HTTPS |

**Count: 2 dependencies**

## Category 3: HTTP Stack (Candidate for Consolidation)

| Crate | Version | Purpose | Transitive Deps | Action | Notes |
|-------|---------|---------|-----------------|--------|-------|
| axum | 0.8 | HTTP server framework | ~15 | Keep | High-level routing, well-maintained |
| tower | 0.5 | Middleware abstraction | ~5 | Keep | axum dependency |
| tower-http | 0.6 | HTTP middleware | ~10 | Optional | Make compression/cors optional |
| hyper | 1.4 | HTTP implementation | ~8 | Keep | Underlying HTTP, required |
| hyper-util | 0.1 | Hyper utilities | ~5 | Keep | hyper companion |
| http-body-util | 0.1 | Body utilities | ~3 | Keep | Small, useful |
| tokio-rustls | 0.26 | TLS integration | ~5 | Keep | Required for HTTPS |
| reqwest | 0.12 | HTTP client | ~30 | Review | Large client, consider hyper-only |

**Action:** Make reqwest optional, use hyper client directly  
**Potential Reduction:** -30 transitive deps

## Category 4: WebCrypto (Consider Vendoring)

| Crate | Version | Purpose | Lines of Code | Vendoring Candidate |
|-------|---------|---------|---------------|---------------------|
| rsa | 0.9 | RSA operations | ~10K | Maybe | Complex, well-tested |
| p256 | 0.13 | P-256 elliptic curve | ~5K | No | Cryptography, requires expertise |
| p384 | 0.13 | P-384 elliptic curve | ~5K | No | Cryptography, requires expertise |
| ecdsa | 0.16 | ECDSA operations | ~3K | No | Cryptography |
| elliptic-curve | 0.13 | Curve traits | ~2K | No | Trait infrastructure |
| signature | 2.2 | Signature traits | ~1K | No | Trait infrastructure |
| hkdf | 0.12 | HKDF key derivation | ~500 | Yes | Small, could vendor |
| pbkdf2 | 0.12 | PBKDF2 key derivation | ~500 | Yes | Small, could vendor |
| hmac | 0.12 | HMAC operations | ~1K | Maybe | Could use ring::hmac |
| zeroize | 1.8 | Secure memory clearing | ~1K | No | Security-critical, widely used |
| getrandom | 0.2 | Random number generation | ~1K | No | Platform abstraction |
| sha2 | 0.10 | SHA-2 hashing | ~2K | No | ring provides alternative |

**Action:** 
- Replace sha2 with ring::digest where possible
- Evaluate hkdf/pbkdf2 vendoring (small, simple)
- Keep cryptographic primitives (high risk to change)

## Category 5: Serialization (Candidate for Zero-Copy)

| Crate | Version | Purpose | Transitive Deps | Action |
|-------|---------|---------|-----------------|--------|
| serde | 1.0 | Serialization framework | ~5 | Review | Consider zero-copy alternatives |
| serde_json | 1.0 | JSON parsing | ~3 | Review | Large, could use simd-json or custom |

**Action:** Evaluate simd-json or custom minimal JSON parser  
**TigerStyle Note:** serde is convenient but adds overhead

## Category 6: Utilities (High Vendoring Potential)

| Crate | Version | Purpose | Lines | Vendoring Decision |
|-------|---------|---------|-------|-------------------|
| base64 | 0.22 | Base64 encoding | ~2K | Vendor | Simple algorithm, stable spec |
| url | 2.5 | URL parsing | ~5K | Keep | Complex spec, security-critical |
| percent-encoding | 2.3 | URL encoding | ~500 | Vendor | Simple, could inline |
| bytes | 1.6 | Byte buffer utilities | ~3K | Keep | Tokio ecosystem standard |
| lazy_static | 1.4 | Static initialization | ~500 | Remove | Use std::sync::OnceLock (Rust 1.70+) |
| dashmap | 6.1 | Concurrent hashmap | ~5K | Keep | Complex, well-tested |
| uuid | 1.8 | UUID generation | ~5K | Optional | Only needed for some features |
| chrono | 0.4 | Date/time handling | ~10K | Optional | Large, could use minimal time crate |

**Actions:**
- Vendor base64 (~200 lines for encode/decode)
- Replace lazy_static with std::sync::OnceLock
- Make uuid, chrono optional features

## Category 7: VFS & I/O (Feature-Gate)

| Crate | Version | Purpose | Action |
|-------|---------|---------|--------|
| tar | 0.4 | Archive format | Keep | Required for slivers |
| tempfile | 3.10 | Temp files | Keep | Security-critical, keep audited |
| walkdir | 2.5 | Directory walking | Vendor | ~1K lines, simple recursion |
| rust-s3 | 0.37 | S3 backend | Optional | Already feature-gated |
| tokio-util | 0.7 | Async utilities | Optional | Already feature-gated |

## Category 8: Networking (Keep)

| Crate | Version | Purpose | Action |
|-------|---------|---------|--------|
| tokio-tungstenite | 0.24 | WebSocket | Keep | Phase 42 feature |
| flate2 | 1.0 | Compression | Keep | Phase 25 feature |
| async-trait | 0.1 | Async traits | Keep | Required for VFS backends |
| signal-hook | 0.3 | Signal handling | Keep | Required for graceful shutdown |
| pollster | 0.4 | Block on async | Review | Used only in specific places |

## Category 9: CLI (Keep)

| Crate | Version | Purpose | Action |
|-------|---------|---------|--------|
| clap | 4.5 | CLI parsing | Keep | Compile-time only for binary |
| rand | 0.8 | Random utilities | Keep | Used for temp file suffixes |

## Category 10: Platform (Keep)

| Crate | Version | Purpose | Action |
|-------|---------|---------|--------|
| libc | 0.2 | C library bindings | Keep | Required for platform APIs |
| nix | 0.29 | Unix APIs | Keep | Safer wrappers for libc |
| home | 0.5.11 | Home dir detection | Remove | Use dirs crate or std::env |

## Summary

| Category | Count | Action |
|----------|-------|--------|
| Essential | 5 | Keep |
| Security-Critical | 2 | Keep |
| HTTP Stack | 8 | Make reqwest optional |
| WebCrypto | 12 | Evaluate sha2→ring, vendor hkdf/pbkdf2 |
| Serialization | 2 | Evaluate zero-copy alternatives |
| Utilities | 6 | Vendor base64, remove lazy_static |
| VFS & I/O | 5 | Vendor walkdir, keep rest |
| Networking | 5 | Keep |
| CLI | 2 | Keep |
| Platform | 3 | Remove home |

**Target Reduction:**
- Direct deps: 40 → 25
- Transitive deps: ~150 → ~80
- Lines of vendored code: ~2K (base64, walkdir, hkdf, pbkdf2)

## Supply Chain Risk Assessment

| Risk Level | Crates | Mitigation |
|------------|--------|------------|
| High | rustls, ring, v8 | Pin to specific versions, monitor CVEs |
| Medium | tokio, axum, hyper | Regular updates, LTS versions |
| Low | Small utility crates | Vendoring candidates |

## Recommendations Priority

1. **P0:** Remove lazy_static (use OnceLock)
2. **P1:** Vendor base64, percent-encoding
3. **P2:** Make reqwest optional
4. **P3:** Vendor walkdir
5. **P4:** Evaluate serde alternatives
6. **P5:** Consolidate crypto (sha2→ring)

## Implementation Status

| Action | Status | Commit |
|--------|--------|--------|
| Dependency audit document | Complete | - |
| lazy_static removal | Planned | - |
| base64 vendoring | Planned | - |
| percent-encoding vendoring | Planned | - |
| Feature flags | Planned | - |
| walkdir vendoring | Future | - |
| serde evaluation | Future | - |
