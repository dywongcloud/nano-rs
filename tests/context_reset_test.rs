//! Persistent-scope isolate lifecycle tests
//!
//! The old "context reset" architecture has been replaced with persistent V8 scopes.
//! Isolates now live for MAX_REQUESTS_PER_ISOLATE requests before recycling.
//! These tests verify the new architecture's correctness.

use nano::v8::initialize_platform;
use nano::worker::context::ContextManager;

fn init_platform() {
    let _ = initialize_platform();
}

#[test]
fn test_context_manager_creation() {
    init_platform();
    let manager = ContextManager::new().expect("ContextManager creation failed");
    assert_eq!(manager.request_count(), 0);
    assert!(!manager.is_handler_initialized("anything"));
}

#[test]
fn test_context_manager_request_count() {
    init_platform();
    let mut manager = ContextManager::new().expect("ContextManager creation failed");
    assert_eq!(manager.request_count(), 0);
    manager.increment_request_count();
    assert_eq!(manager.request_count(), 1);
    manager.increment_request_count();
    assert_eq!(manager.request_count(), 2);
}

#[test]
fn test_context_manager_isolate_id_unique() {
    init_platform();
    let m1 = ContextManager::new().expect("m1 failed");
    let m2 = ContextManager::new().expect("m2 failed");
    assert_ne!(m1.isolate_id(), m2.isolate_id());
}
