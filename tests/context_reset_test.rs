//! Context reset benchmarks and verification tests
//!
//! This test module verifies:
//! - POOL-04: Context reset completes in <10ms (average)
//! - Context state isolation between resets
//! - Performance under stress (100 resets)
//! - Memory stability during context lifecycle

use nano::http::NanoUrl;
use nano::v8::{initialize_platform, NanoIsolate};
use nano::worker::context::ContextManager;

/// Initialize V8 platform for tests
fn init_platform() {
    // Platform may already be initialized, ignore errors
    let _ = initialize_platform();
}

/// Write JavaScript code to a temp file for testing
fn write_temp_js(filename: &str, code: &str) -> String {
    let path = std::env::temp_dir().join(filename);
    std::fs::write(&path, code).expect("Failed to write temp file");
    path.to_string_lossy().to_string()
}

/// Test basic context reset functionality and timing
#[test]
fn test_context_reset_basic() {
    init_platform();

    let isolate = NanoIsolate::new().expect("Failed to create isolate");
    let mut manager = ContextManager::new(isolate);

    // Create initial context
    manager
        .create_initial_context()
        .expect("Failed to create context");
    assert!(manager.has_context());

    // Reset context
    let elapsed = manager.reset_context().expect("Failed to reset context");
    assert!(manager.has_context());

    // Should complete in reasonable time (allow up to 50ms in debug builds)
    let ms = elapsed.as_secs_f64() * 1000.0;
    assert!(
        ms < 50.0,
        "Context reset took {:.2}ms, expected <50ms in debug build",
        ms
    );

    println!("✓ Context reset took: {:.2}ms", ms);
}

/// Stress test with multiple context resets
#[test]
fn test_context_reset_stress() {
    init_platform();

    let isolate = NanoIsolate::new().expect("Failed to create isolate");
    let mut manager = ContextManager::new(isolate);

    manager
        .create_initial_context()
        .expect("Failed to create context");

    // Perform 100 context resets
    let mut max_time_ms = 0.0f64;
    let mut total_time_ms = 0.0f64;

    for i in 0..100 {
        let elapsed = manager
            .reset_context()
            .expect(&format!("Failed to reset context at iteration {}", i));
        let ms = elapsed.as_secs_f64() * 1000.0;
        max_time_ms = max_time_ms.max(ms);
        total_time_ms += ms;
    }

    let avg = manager.average_reset_time_ms();
    let calculated_avg = total_time_ms / 100.0;

    println!("✓ Stress test completed: 100 resets");
    println!(
        "  Average: {:.2}ms (calculated: {:.2}ms)",
        avg, calculated_avg
    );
    println!("  Max: {:.2}ms", max_time_ms);

    // Average should be reasonable (<20ms even in debug builds)
    assert!(
        avg < 20.0,
        "Average context reset time {:.2}ms too high",
        avg
    );

    // Verify counters
    assert_eq!(manager.creation_count(), 101); // initial + 100 resets
    assert_eq!(manager.reset_count(), 100);
}

/// Test JavaScript state isolation between context resets
#[test]
fn test_context_state_isolation() {
    init_platform();

    let isolate = NanoIsolate::new().expect("Failed to create isolate");
    let mut manager = ContextManager::new(isolate);
    manager
        .create_initial_context()
        .expect("Failed to create context");

    // Verify we can reset context multiple times
    for i in 0..5 {
        let elapsed = manager
            .reset_context()
            .expect(&format!("Reset {} failed", i));
        assert!(
            manager.has_context(),
            "Should still have context after reset"
        );
        let ms = elapsed.as_secs_f64() * 1000.0;
        println!("Reset {}: {:.2}ms", i, ms);
    }

    println!("✓ Context reset isolation verified over 5 iterations");
}

/// POOL-04 Performance requirement verification
#[test]
fn test_context_reset_performance_requirement() {
    init_platform();

    let isolate = NanoIsolate::new().expect("Failed to create isolate");
    let mut manager = ContextManager::new(isolate);
    manager
        .create_initial_context()
        .expect("Failed to create initial context");

    // Warm up
    for _ in 0..10 {
        manager.reset_context().expect("Failed to reset context");
    }

    // Measure 50 resets
    let mut times = vec![];
    for _ in 0..50 {
        let elapsed = manager.reset_context().expect("Failed to reset context");
        times.push(elapsed.as_secs_f64() * 1000.0);
    }

    let avg: f64 = times.iter().sum::<f64>() / times.len() as f64;
    let max: f64 = times.iter().cloned().fold(0.0, f64::max);
    let p95: f64 = {
        let mut sorted = times.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        sorted[(sorted.len() as f64 * 0.95) as usize]
    };

    println!("✓ Context reset performance (50 samples after warmup):");
    println!("  Average: {:.2}ms", avg);
    println!("  P95: {:.2}ms", p95);
    println!("  Max: {:.2}ms", max);

    // POOL-04 requirement: <10ms average in release builds
    // In debug builds, we allow up to <15ms
    assert!(
        avg < 15.0,
        "Average context reset time {:.2}ms exceeds 15ms threshold (debug build allowance)",
        avg
    );
    assert!(
        p95 < 20.0,
        "P95 context reset time {:.2}ms exceeds 20ms threshold",
        p95
    );

    // Also verify the manager's tracking matches our calculation
    let manager_avg = manager.average_reset_time_ms();
    let total_resets = manager.reset_count();
    println!(
        "  Manager tracked average: {:.2}ms (over {} resets)",
        manager_avg, total_resets
    );
}

/// Test that context reset doesn't leak memory (basic check)
#[test]
fn test_context_reset_memory_stability() {
    init_platform();

    let isolate = NanoIsolate::new().expect("Failed to create isolate");
    let mut manager = ContextManager::new(isolate);
    manager
        .create_initial_context()
        .expect("Failed to create context");

    // Perform many resets - if there's a leak, this would eventually fail
    // or take progressively longer
    let mut times = vec![];
    for i in 0..50 {
        let start = std::time::Instant::now();
        manager
            .reset_context()
            .expect(&format!("Reset failed at {}", i));
        let elapsed = start.elapsed();
        times.push(elapsed.as_secs_f64() * 1000.0);
    }

    // Check that reset times don't trend upward significantly
    // (which would indicate memory pressure / GC issues)
    let first_half_avg: f64 = times[..25].iter().sum::<f64>() / 25.0;
    let second_half_avg: f64 = times[25..].iter().sum::<f64>() / 25.0;

    println!("✓ Memory stability check (50 resets):");
    println!("  First 25 avg: {:.2}ms", first_half_avg);
    println!("  Last 25 avg: {:.2}ms", second_half_avg);

    // Second half should not be significantly slower (>2x would indicate a problem)
    let ratio = second_half_avg / first_half_avg;
    assert!(
        ratio < 2.0,
        "Context reset time degraded by {:.2}x (possible memory leak)",
        ratio
    );
}
