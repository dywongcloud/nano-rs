//! V8 integration tests - EPT fix verification
//!
//! These tests verify the EPT (ExternalPointerTable) fix prevents SIGSEGV
//! crashes during isolate lifecycle operations. The EPT bug (AP-02 from Zig
//! version) manifests as random crashes when isolates are rapidly created
//! and disposed, especially with ArrayBuffer operations.
//!
//! The fix: A strong v8::Global<Value> sentinel per isolate keeps the EPT
//! segment mapped, preventing background GC from unmapping it prematurely.
//!
//! # EPT Verification Strategy
//!
//! The primary verification is simply that these tests run without crashing.
//! Without the EPT fix, the stress tests would SIGSEGV due to EPT segment
//! unmapping during rapid isolate create/dispose cycles.

use nano::v8::{initialize_platform, NanoIsolate};

/// Initialize V8 platform once for all tests
fn setup() {
    initialize_platform().expect("Failed to initialize V8 platform");
}

/// Test that isolates can be created and disposed without crashing
///
/// This test verifies the basic isolate lifecycle works:
/// - Create isolate with EPT sentinel
/// - Create context within isolate
/// - Dispose isolate (sentinel dropped first, then isolate)
#[test]
fn test_basic_isolate_lifecycle() {
    setup();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    let _context = isolate.create_context();

    // Both context and isolate are dropped here
    // This should NOT cause a SIGSEGV thanks to the EPT fix
    // Test passes if we reach this point without crashing
}

/// Stress test: Create and dispose 100 isolates sequentially
///
/// This test catches the AP-02 EPT bug from the Zig version. Without the
/// sentinel fix, this would likely crash with SIGSEGV due to the background
/// GC unmapping the array_buffer_sweeper_space EPT segment while isolates
/// are being created/disposed rapidly.
///
/// # Critical EPT Fix Verification
/// - 100 isolate create/dispose cycles
/// - Each cycle: create isolate → create context → drop both
/// - If no SIGSEGV occurs, the EPT fix is working
#[test]
fn test_ept_stress_100_isolates() {
    setup();

    const NUM_ISOLATES: usize = 100;

    for i in 0..NUM_ISOLATES {
        let mut isolate = NanoIsolate::new().expect(&format!("Failed to create isolate {}", i));
        let _context = isolate.create_context();

        // Isolate and context drop here
        // Each iteration tests the EPT fix
    }

    // If we reach here without SIGSEGV, the EPT fix is verified
    println!(
        "EPT stress test passed: {} isolates created/disposed without crash",
        NUM_ISOLATES
    );
}

/// Test multiple context creation/disposal within a single isolate
///
/// This verifies that contexts can be created and disposed within an
/// isolate's lifetime without triggering the EPT bug.
#[test]
fn test_context_lifecycle_within_isolate() {
    setup();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

    // Create and dispose 10 contexts within the same isolate
    for _ in 0..10 {
        let _context = isolate.create_context();
        // Context is dropped here
    }

    // Isolate is dropped here
}

/// Test that rapid sequential isolate creation works
///
/// This simulates the worker pool scenario where isolates are frequently
/// created and disposed as workers cycle.
#[test]
fn test_rapid_isolate_creation() {
    setup();

    // Rapidly create and dispose 50 isolates
    for _ in 0..50 {
        let isolate = NanoIsolate::new().expect("Failed to create isolate");
        // Immediately drop - no context creation
        drop(isolate);
    }
}

/// Test isolate with multiple context create/dispose cycles
///
/// This tests the more complex scenario of isolates that handle
/// multiple requests (context resets).
#[test]
fn test_isolate_with_context_resets() {
    setup();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

    // Simulate 20 request cycles (context create/dispose)
    for _ in 0..20 {
        let _context = isolate.create_context();
        // Context is dropped here (end of request)
    }

    // Isolate is dropped here (end of worker lifecycle)
}
