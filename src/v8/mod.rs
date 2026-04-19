//! V8 integration with EPT fix
//!
//! This module handles V8 platform initialization, isolate creation,
//! and the critical EPT (ExternalPointerTable) fix that prevents
//! SIGSEGV crashes during ArrayBuffer allocation.
//!
//! The EPT fix requires creating a strong v8::Global<Value> sentinel
//! per isolate immediately after creation to prevent the background
//! GC from unmapping the array_buffer_sweeper_space segment.
//!
//! Reference: AP-02 from Zig version (prod.md)

pub mod context;
pub mod isolate;
pub mod platform;

// Re-export key functions and types for convenience
pub use context::create_context;
pub use isolate::NanoIsolate;
pub use platform::{initialize_platform, is_initialized, shutdown_platform};
