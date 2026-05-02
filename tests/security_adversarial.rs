//! Adversarial Security Testing Suite
//!
//! Comprehensive security tests covering 8 attack vectors:
//! - CPU exhaustion attacks (infinite loops, ReDoS)
//! - Memory exhaustion attacks (large allocations, leaks)
//! - VFS escape attempts (path traversal, symlinks)
//! - Network attacks (DNS rebinding, flooding, slowloris)
//! - JavaScript injection (prototype pollution, eval)
//! - WebAssembly attacks (validation bypasses)
//! - Multi-tenant isolation (cross-tenant access)
//! - Cryptographic attacks (weak keys, timing)
//!
//! Run with: cargo test --test security_adversarial

// Security test modules
mod security_utils;
mod adversarial_cpu;
mod adversarial_memory;
mod adversarial_vfs;
mod adversarial_network;
mod adversarial_js_injection;
mod adversarial_wasm;
mod adversarial_isolation;
mod adversarial_crypto;

// Re-export utilities
pub use security_utils::*;
