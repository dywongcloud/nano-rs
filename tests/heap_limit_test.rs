//! Integration test for heap limit enforcement
//!
//! This test verifies that V8 heap limits are properly configured and
//! that the heap limit callback mechanism is in place.

use nano::v8::{initialize_platform, NanoIsolate};

/// Helper to ensure platform is initialized for tests
fn init_platform() {
    if !nano::v8::is_initialized() {
        initialize_platform().expect("Failed to initialize V8 platform");
    }
}

/// Test that heap limit is stored correctly after calling set_heap_limits
#[test]
fn test_heap_limit_stored() {
    init_platform();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    
    // Set 64MB heap limit
    isolate.set_heap_limits(32 * 1024 * 1024, 64 * 1024 * 1024);

    // Verify the limit was stored
    assert_eq!(isolate.heap_limit_bytes(), 64 * 1024 * 1024);
}

/// Test that heap limit can be updated after initial setting
///
/// Note: Only the stored limit value is updated on subsequent calls.
/// The V8 callback is only registered on the first call since V8 only
/// supports one near-heap-limit callback per isolate.
#[test]
fn test_heap_limit_update_value() {
    init_platform();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    
    // Set initial limit
    isolate.set_heap_limits(10 * 1024 * 1024, 16 * 1024 * 1024);
    assert_eq!(isolate.heap_limit_bytes(), 16 * 1024 * 1024);
    
    // Update to larger limit - stored value should change
    // (callback remains registered with original parameters)
    isolate.set_heap_limits(20 * 1024 * 1024, 32 * 1024 * 1024);
    assert_eq!(isolate.heap_limit_bytes(), 32 * 1024 * 1024);
}

/// Test that isolate remains usable after setting heap limits
///
/// This ensures setting heap limits doesn't break isolate functionality
#[test]
fn test_isolate_usable_after_setting_heap_limits() {
    init_platform();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    
    // Set heap limit before creating context
    isolate.set_heap_limits(10 * 1024 * 1024, 16 * 1024 * 1024);
    
    // Create context and verify isolate is still usable
    let context = isolate.create_context();
    {
        let scope_storage = std::pin::pin!(v8::HandleScope::new(isolate.isolate()));
        let mut scope = scope_storage.init();
        let local_context = v8::Local::new(&mut scope, &context);
        let mut ctx_scope = v8::ContextScope::new(&mut scope, local_context);

        // Simple script that should execute successfully
        let code = r#"
            const result = 2 + 2;
            result;
        "#;

        let code_str = v8::String::new(&mut ctx_scope, code).expect("Failed to create code string");
        let script = v8::Script::compile(&mut ctx_scope, code_str, None)
            .expect("Failed to compile script");

        let result = script.run(&mut ctx_scope);
        assert!(
            result.is_some(),
            "Execution should succeed after setting heap limits"
        );

        // Verify the result
        if let Some(value) = result {
            assert!(value.is_number(), "Result should be a number");
            if let Some(int) = value.to_integer(&mut ctx_scope) {
                assert_eq!(int.value(), 4, "2 + 2 should equal 4");
            }
        }
    }
}

/// Test that heap statistics are available
#[test]
fn test_heap_statistics_available() {
    init_platform();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    
    // Get heap stats before setting limits
    let stats_before = isolate.heap_statistics();
    assert!(stats_before.total_heap_size() > 0, "Heap should have some size");
    
    // Set heap limit
    isolate.set_heap_limits(10 * 1024 * 1024, 16 * 1024 * 1024);
    
    // Get heap stats after setting limits
    let stats_after = isolate.heap_statistics();
    assert!(stats_after.total_heap_size() > 0, "Heap should still have size after setting limits");
    
    // Heap limit should reflect our setting
    assert_eq!(isolate.heap_limit_bytes(), 16 * 1024 * 1024);
}

/// Test that heap limit callback mechanism is registered
///
/// This test verifies that the set_heap_limits method doesn't panic
/// and that the callback registration succeeds. The actual callback
/// triggering depends on V8's internal heap growth patterns.
#[test]
fn test_heap_limit_callback_registration() {
    init_platform();

    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    
    // Register heap limit callback - this should not panic
    isolate.set_heap_limits(1 * 1024 * 1024, 2 * 1024 * 1024);
    
    // Verify the limit is stored
    assert_eq!(isolate.heap_limit_bytes(), 2 * 1024 * 1024);
    
    // The isolate should still be functional
    let _context = isolate.create_context();
}
