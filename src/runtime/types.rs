//! Runtime types for timers and abort signals
//!
//! This module provides the core types for:
//! - Timer management (TimerId, TimerHandle)
//! - AbortSignal state management

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::Mutex;

/// Unique timer ID
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TimerId(u64);

impl TimerId {
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::SeqCst))
    }

    pub fn value(&self) -> u64 {
        self.0
    }
}

impl Default for TimerId {
    fn default() -> Self {
        Self::new()
    }
}

/// Timer handle for tracking active timers
#[derive(Debug)]
pub struct TimerHandle {
    pub id: TimerId,
    pub is_interval: bool,
    pub cancelled: Arc<AtomicU64>, // 0 = active, 1 = cancelled
}

impl TimerHandle {
    pub fn cancel(&self) {
        self.cancelled.store(1, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst) == 1
    }
}

/// AbortSignal state shared between controller and signal
#[derive(Debug, Clone)]
pub struct AbortSignalState {
    pub aborted: Arc<AtomicU64>,
    pub reason: Arc<Mutex<Option<String>>>,
}

impl AbortSignalState {
    pub fn new() -> Self {
        Self {
            aborted: Arc::new(AtomicU64::new(0)),
            reason: Arc::new(Mutex::new(None)),
        }
    }

    pub fn abort(&self, reason: Option<String>) {
        self.aborted.store(1, Ordering::SeqCst);
        if let Ok(mut guard) = self.reason.try_lock() {
            *guard = reason;
        }
    }

    pub fn is_aborted(&self) -> bool {
        self.aborted.load(Ordering::SeqCst) == 1
    }

    pub async fn get_reason(&self) -> Option<String> {
        self.reason.lock().await.clone()
    }
}

impl Default for AbortSignalState {
    fn default() -> Self {
        Self::new()
    }
}

/// Global registry for abort signal states
/// Used to share state between AbortController and AbortSignal instances
use std::collections::HashMap;
use std::sync::Mutex as StdMutex;

lazy_static::lazy_static! {
    static ref ABORT_REGISTRY: StdMutex<HashMap<u64, AbortSignalState>> = StdMutex::new(HashMap::new());
}

/// Register a new abort signal state and return its ID
pub fn register_abort_state(state: AbortSignalState) -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    if let Ok(mut registry) = ABORT_REGISTRY.lock() {
        registry.insert(id, state);
    }
    id
}

/// Get an abort signal state by ID
pub fn get_abort_state(id: u64) -> Option<AbortSignalState> {
    if let Ok(registry) = ABORT_REGISTRY.lock() {
        registry.get(&id).cloned()
    } else {
        None
    }
}

/// Remove an abort signal state from registry
pub fn remove_abort_state(id: u64) {
    if let Ok(mut registry) = ABORT_REGISTRY.lock() {
        registry.remove(&id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timer_id_generation() {
        let id1 = TimerId::new();
        let id2 = TimerId::new();
        assert_ne!(id1.value(), id2.value());
    }

    #[test]
    fn test_timer_handle_cancel() {
        let handle = TimerHandle {
            id: TimerId::new(),
            is_interval: false,
            cancelled: Arc::new(AtomicU64::new(0)),
        };
        
        assert!(!handle.is_cancelled());
        handle.cancel();
        assert!(handle.is_cancelled());
    }

    #[test]
    fn test_abort_signal_state() {
        let state = AbortSignalState::new();
        assert!(!state.is_aborted());
        
        state.abort(Some("test reason".to_string()));
        assert!(state.is_aborted());
    }

    #[test]
    fn test_abort_registry() {
        let state = AbortSignalState::new();
        let id = register_abort_state(state.clone());
        
        let retrieved = get_abort_state(id);
        assert!(retrieved.is_some());
        assert!(!retrieved.unwrap().is_aborted());
        
        // Clean up
        remove_abort_state(id);
    }
}
