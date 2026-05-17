//! Data Plane: Optimized request execution.
//!
//! Per TigerStyle:
//! - NO validation checks in hot path (control plane handles validation)
//! - NO dynamic allocations (pre-allocated by control plane)
//! - Minimal branching (lookup tables over conditionals)
//! - Zero-copy where possible
//! - CPU sprints through batches

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

use anyhow::{anyhow, Result};

// Thread-local storage for the worker thread's Tokio runtime handle.
// This allows fetch() and other async operations to access the runtime.
thread_local! {
    static WORKER_RUNTIME: RefCell<Option<tokio::runtime::Handle>> = RefCell::new(None);
}

/// Get the worker thread's Tokio runtime handle if available.
pub fn with_worker_runtime<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&tokio::runtime::Handle) -> R,
{
    WORKER_RUNTIME.with(|runtime| {
        runtime.borrow().as_ref().map(f)
    })
}

/// Set the worker runtime handle for the current thread.
pub fn set_worker_runtime(handle: tokio::runtime::Handle) {
    WORKER_RUNTIME.with(|runtime| {
        *runtime.borrow_mut() = Some(handle);
    });
}

/// Code cache entry with modification time tracking.
struct CodeCacheEntry {
    code: Arc<str>,
    modified: SystemTime,
}

/// Thread-safe code cache to avoid disk reads on every request.
///
/// This significantly reduces latency for frequently accessed entrypoints
/// by caching the file contents in memory and only re-reading when the
/// file modification time changes.
static CODE_CACHE: RwLock<Option<HashMap<String, CodeCacheEntry>>> = RwLock::new(None);

/// Initialize the code cache on first use.
pub fn init_code_cache() {
    let mut cache = CODE_CACHE.write().unwrap();
    if cache.is_none() {
        *cache = Some(HashMap::new());
    }
}

/// Read code from cache or disk, with automatic cache invalidation.
///
/// This function caches file contents to avoid repeated disk reads,
/// which is a significant latency optimization (can save 1-5ms per request).
pub fn read_code_cached(entrypoint: &str) -> Result<Arc<str>> {
    // Fast path: check if we can read from cache
    {
        let cache_read = CODE_CACHE.read().unwrap();
        if let Some(cache) = cache_read.as_ref() {
            if let Some(entry) = cache.get(entrypoint) {
                // Check if file has been modified since we cached it
                if let Ok(metadata) = std::fs::metadata(entrypoint) {
                    if let Ok(modified) = metadata.modified() {
                        if modified == entry.modified {
                            // Cache hit - return cached code
                            return Ok(entry.code.clone());
                        }
                    }
                }
            }
        }
    }

    // Slow path: read from disk and update cache
    let code = std::fs::read_to_string(entrypoint)
        .map_err(|e| anyhow!("Failed to read entrypoint '{}': {}", entrypoint, e))?;

    let modified = std::fs::metadata(entrypoint)
        .and_then(|m| m.modified())
        .unwrap_or_else(|_| std::time::SystemTime::now());

    let code_arc: Arc<str> = code.into();

    // Update cache
    {
        let mut cache_write = CODE_CACHE.write().unwrap();
        if cache_write.is_none() {
            *cache_write = Some(HashMap::new());
        }
        if let Some(cache) = cache_write.as_mut() {
            cache.insert(entrypoint.to_string(), CodeCacheEntry {
                code: code_arc.clone(),
                modified,
            });
        }
    }

    Ok(code_arc)
}

// Thread-local storage for isolate termination request.
// This is checked by the main thread during execution to determine
// if the timer thread has requested termination.
// Global atomic state for cross-thread isolate termination
// Timer thread needs to access the isolate pointer stored by the main thread
static TERMINATION_REQUESTED: AtomicBool = AtomicBool::new(false);
static TERMINATION_ISOLATE_PTR: AtomicPtr<v8::Isolate> = AtomicPtr::new(std::ptr::null_mut());

/// Request termination of the current V8 isolate.
///
/// Called by the timer thread when CPU timeout is reached.
fn request_isolate_termination() {
    TERMINATION_REQUESTED.store(true, Ordering::SeqCst);

    let ptr = TERMINATION_ISOLATE_PTR.load(Ordering::SeqCst);
    if !ptr.is_null() {
        // SAFETY: Pointer is non-null and valid (set by CpuTimeoutGuard::new)
        // Terminate execution is safe to call even if already terminating
        unsafe {
            if let Some(isolate) = ptr.as_ref() {
                isolate.terminate_execution();
            }
        }
        // Record CPU timeout enforcement event
        crate::metrics::METRICS.record_cpu_timeout();
    }
}

/// Guard that sets up CPU timeout enforcement for V8 execution.
///
/// Uses a wall-clock timer as an approximation of CPU time.
/// Note: True CPU time measurement requires platform-specific APIs (e.g., getrusage
/// on Unix, GetProcessTimes on Windows) which are not yet integrated. The wall-clock
/// approximation works for most cases but may be affected by system load.
///
/// The timer thread calls request_isolate_termination() when timeout is reached.
pub struct CpuTimeoutGuard {
    /// Handle to the timer thread
    timer_thread: Option<std::thread::JoinHandle<()>>,
}

impl CpuTimeoutGuard {
    /// Create a new CPU timeout guard.
    ///
    /// # Arguments
    /// * `isolate` - The V8 isolate to terminate on timeout
    /// * `limit_ms` - Wall time limit in milliseconds (used as approximation for CPU time)
    pub fn new(isolate: &mut v8::Isolate, limit_ms: u32) -> Self {
        let isolate_ptr: *mut v8::Isolate = isolate as *mut _;
        TERMINATION_ISOLATE_PTR.store(isolate_ptr, Ordering::SeqCst);
        TERMINATION_REQUESTED.store(false, Ordering::SeqCst);

        let timer_thread = std::thread::spawn(move || {
            let limit_duration = std::time::Duration::from_millis(limit_ms as u64);
            std::thread::sleep(limit_duration);
            request_isolate_termination();
        });

        Self {
            timer_thread: Some(timer_thread),
        }
    }
}

impl Drop for CpuTimeoutGuard {
    fn drop(&mut self) {
        if let Some(thread) = self.timer_thread.take() {
            let _ = thread.join();
        }
        TERMINATION_ISOLATE_PTR.store(std::ptr::null_mut(), Ordering::SeqCst);
        TERMINATION_REQUESTED.store(false, Ordering::SeqCst);
    }
}

