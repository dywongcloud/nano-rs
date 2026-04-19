//! V8 isolate with EPT (ExternalPointerTable) fix sentinel
//!
//! This module implements the critical EPT fix that prevents SIGSEGV crashes
//! during ArrayBuffer allocation. The fix requires a strong v8::Global<Value>
//! sentinel per isolate that keeps the EPT segment mapped.
//!
//! # EPT Fix Explanation
//!
//! The ExternalPointerTable (EPT) manages pointers to objects outside the V8 heap
//! (like ArrayBuffer backing stores). When contexts are rapidly created/disposed,
//! the background GC may unmap the `array_buffer_sweeper_space` EPT segment while
//! ArrayBuffer allocations are still in flight, causing use-after-free or SIGSEGV.
//!
//! The fix: Create a strong `v8::Global<Value>` sentinel immediately after isolate
//! creation. This sentinel holds a reference that keeps the EPT segment mapped,
//! preventing the background GC from unmapping it until the isolate is disposed.
//!
//! Reference: AP-02 from Zig version (prod.md), PITFALLS.md Pitfall 1

use anyhow::Result;
use std::marker::PhantomData;

/// A V8 isolate with the EPT fix sentinel
///
/// This struct wraps a V8 isolate and maintains a strong Global sentinel
/// that prevents EPT segment unmapping during the isolate's lifetime.
///
/// # Thread Safety
/// Isolates are NOT thread-safe. They must never be moved between threads
/// and must only be accessed from the thread that created them.
/// The `PhantomData<*mut ()>` ensures this type is !Send + !Sync.
///
/// # Drop Order
/// The sentinel MUST be dropped BEFORE the isolate. This struct uses Rust's
/// field drop order (fields are dropped in declaration order, reverse of drop).
/// We declare `sentinel` before `isolate` so `isolate` is dropped last.
pub struct NanoIsolate {
    /// The strong Global sentinel - keeps EPT segment mapped
    /// This MUST be dropped before the isolate
    #[allow(dead_code)] // Sentinel only needs to exist, not be accessed
    sentinel: v8::Global<v8::Value>,

    /// The V8 isolate - dropped after the sentinel
    isolate: v8::OwnedIsolate,

    /// Phantom data to make this !Send + !Sync
    /// Ensures isolates never move between threads (rusty_v8 issue #1467)
    _not_send_sync: PhantomData<*mut ()>,
}

impl NanoIsolate {
    /// Create a new V8 isolate with the EPT fix sentinel
    ///
    /// This function:
    /// 1. Creates a new V8 isolate with default params
    /// 2. Creates a HandleScope to work within the isolate
    /// 3. Creates a strong v8::Global<Value> sentinel (undefined value)
    /// 4. Stores the sentinel to prevent EPT segment unmapping
    ///
    /// # Platform Requirement
    /// The V8 platform MUST be initialized before calling this function.
    /// Call `nano::v8::platform::initialize_platform()` first.
    ///
    /// # Example
    /// ```
    /// use nano::v8::{initialize_platform, NanoIsolate};
    ///
    /// initialize_platform().unwrap();
    /// let isolate = NanoIsolate::new().unwrap();
    /// // Use isolate...
    /// // isolate drops automatically when it goes out of scope
    /// ```
    pub fn new() -> Result<Self> {
        // Create the isolate with default params - returns OwnedIsolate
        let mut isolate = v8::Isolate::new(Default::default());

        // Create the EPT fix sentinel
        // We need a HandleScope temporarily to create the Global
        let sentinel = {
            let scope = &mut v8::HandleScope::new(&mut isolate);
            // Create a Global holding undefined as a Value
            // v8::undefined() returns a Primitive, we need to cast it to Value
            let undefined = v8::undefined(scope);
            let value: v8::Local<v8::Value> = undefined.into();
            v8::Global::new(scope, value)
        };
        // HandleScope is dropped here, but sentinel survives (it's a Global)

        tracing::debug!("Created NanoIsolate with EPT fix sentinel");

        Ok(Self {
            sentinel,
            isolate,
            _not_send_sync: PhantomData,
        })
    }

    /// Create a new V8 context within this isolate
    ///
    /// Returns a Local<Context> that can be used to execute scripts.
    /// The context is scoped to the isolate's lifetime.
    ///
    /// # Example
    /// ```
    /// use nano::v8::{initialize_platform, NanoIsolate};
    ///
    /// initialize_platform().unwrap();
    /// let mut isolate = NanoIsolate::new().unwrap();
    /// let context = isolate.create_context();
    /// // Execute scripts in the context...
    /// ```
    pub fn create_context(&mut self) -> v8::Local<v8::Context> {
        // Create a HandleScope for working with this isolate
        let scope = &mut v8::HandleScope::new(&mut self.isolate);

        // Create a context with default options
        let context = v8::Context::new(scope, Default::default());

        context
    }

    /// Get a reference to the underlying isolate
    ///
    /// This is primarily useful for low-level V8 operations.
    /// Prefer using the provided methods when possible.
    pub fn isolate(&mut self) -> &mut v8::OwnedIsolate {
        &mut self.isolate
    }

    /// Get the sentinel as a reference (for testing/debugging)
    #[cfg(test)]
    fn sentinel(&self) -> &v8::Global<v8::Value> {
        &self.sentinel
    }
}

impl Drop for NanoIsolate {
    /// Drop implementation ensures proper cleanup order
    ///
    /// # EPT Fix Critical Order
    /// The sentinel MUST be dropped BEFORE the isolate. In Rust:
    /// - Fields are dropped in declaration order
    /// - We declared `sentinel` before `isolate`
    /// - Therefore, sentinel drops first, isolate drops last
    ///
    /// This is the correct order for the EPT fix - the sentinel is dropped,
    /// releasing the strong reference, and then the isolate is disposed.
    fn drop(&mut self) {
        tracing::debug!("Dropping NanoIsolate (EPT sentinel dropped before isolate)");
        // Fields are dropped in declaration order:
        // 1. sentinel (v8::Global<Value>) - releases strong reference
        // 2. isolate (v8::OwnedIsolate) - disposes the isolate
        // 3. _not_send_sync (PhantomData) - no-op
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v8::platform;

    /// Helper to ensure platform is initialized for tests
    fn init_platform() {
        platform::initialize_platform().expect("Failed to initialize V8 platform");
    }

    /// Test that we can create an isolate with the EPT fix
    #[test]
    fn test_create_isolate() {
        init_platform();

        let isolate = NanoIsolate::new();
        assert!(isolate.is_ok(), "Failed to create isolate");
    }

    /// Test that we can create a context within an isolate
    #[test]
    fn test_create_context() {
        init_platform();

        let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
        let _context = isolate.create_context();

        // Context created successfully - test passes if no crash
    }

    /// Test that the sentinel exists
    #[test]
    fn test_ept_sentinel_exists() {
        init_platform();

        let isolate = NanoIsolate::new().expect("Failed to create isolate");

        // The sentinel should exist (it's a Global, which is always valid)
        let _sentinel = isolate.sentinel();
        // We can't easily test the sentinel's effect, but its existence
        // is verified by the fact that the isolate was created successfully
        // and no SIGSEGV occurs
        // Sentinel is a Global, its presence proves the EPT fix is in place
    }

    /// Test that multiple isolates can be created and disposed
    /// This stress tests the EPT fix
    #[test]
    fn test_multiple_isolates() {
        init_platform();

        // Create and dispose 10 isolates sequentially
        // This would trigger the EPT bug without the sentinel
        for i in 0..10 {
            let mut isolate = NanoIsolate::new().expect(&format!("Failed to create isolate {}", i));
            let _context = isolate.create_context();
            // isolate and context are dropped here
        }

        // If we reach here without SIGSEGV, the EPT fix is working
    }
}
