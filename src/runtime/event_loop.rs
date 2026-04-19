//! Event loop and timer management
//!
//! This module provides the timer queue for managing async operations:
//! - setTimeout/setInterval scheduling
//! - clearTimeout/clearInterval cancellation
//! - Uses tokio timers for efficient async execution

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

use crate::runtime::types::{TimerId, TimerHandle};

/// Timer queue for managing active timers
#[derive(Debug, Default)]
pub struct TimerQueue {
    timers: Arc<Mutex<HashMap<TimerId, TimerHandle>>>,
}

impl TimerQueue {
    pub fn new() -> Self {
        Self {
            timers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Schedule a one-time timer
    pub async fn schedule<F>(
        &self,
        delay_ms: u64,
        callback: F,
    ) -> TimerId
    where
        F: FnOnce() + Send + 'static,
    {
        let id = TimerId::new();
        let handle = TimerHandle {
            id,
            is_interval: false,
            cancelled: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        };

        let cancelled = handle.cancelled.clone();
        let timers_clone = Arc::clone(&self.timers);

        {
            let mut timers = self.timers.lock().await;
            timers.insert(id, handle);
        }

        // Spawn timer task
        tokio::spawn(async move {
            sleep(Duration::from_millis(delay_ms)).await;

            // Check if cancelled before executing
            if cancelled.load(std::sync::atomic::Ordering::SeqCst) == 0 {
                callback();
            }

            // Remove from queue
            if let Ok(mut timers) = timers_clone.try_lock() {
                timers.remove(&id);
            }
        });

        id
    }

    /// Schedule a repeating interval timer
    pub async fn schedule_interval<F>(
        &self,
        interval_ms: u64,
        callback: F,
    ) -> TimerId
    where
        F: Fn() + Send + 'static,
    {
        let id = TimerId::new();
        let handle = TimerHandle {
            id,
            is_interval: true,
            cancelled: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        };

        let cancelled = handle.cancelled.clone();
        let timers_clone = Arc::clone(&self.timers);

        {
            let mut timers = self.timers.lock().await;
            timers.insert(id, handle);
        }

        // Spawn interval task
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(interval_ms));
            
            loop {
                interval.tick().await;

                // Check if cancelled before executing
                if cancelled.load(std::sync::atomic::Ordering::SeqCst) == 1 {
                    break;
                }

                callback();
            }

            // Remove from queue when done
            if let Ok(mut timers) = timers_clone.try_lock() {
                timers.remove(&id);
            }
        });

        id
    }

    /// Cancel a timer by ID
    pub async fn cancel(&self, id: TimerId) {
        let mut timers = self.timers.lock().await;
        if let Some(handle) = timers.get(&id) {
            handle.cancel();
        }
        timers.remove(&id);
    }

    /// Get count of active timers
    pub async fn active_count(&self) -> usize {
        let timers = self.timers.lock().await;
        timers.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[tokio::test]
    async fn test_timer_schedule_and_fire() {
        let queue = TimerQueue::new();
        let fired = Arc::new(AtomicBool::new(false));
        let fired_clone = fired.clone();

        let _id = queue.schedule(10, move || {
            fired_clone.store(true, Ordering::SeqCst);
        }).await;

        sleep(Duration::from_millis(20)).await;

        assert!(fired.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_timer_cancel() {
        let queue = TimerQueue::new();
        let fired = Arc::new(AtomicBool::new(false));
        let fired_clone = fired.clone();

        let id = queue.schedule(50, move || {
            fired_clone.store(true, Ordering::SeqCst);
        }).await;

        // Cancel immediately
        queue.cancel(id).await;

        sleep(Duration::from_millis(100)).await;

        assert!(!fired.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_timer_interval() {
        let queue = TimerQueue::new();
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count_clone = count.clone();

        let id = queue.schedule_interval(10, move || {
            count_clone.fetch_add(1, Ordering::SeqCst);
        }).await;

        // Let it fire a few times
        sleep(Duration::from_millis(35)).await;

        // Cancel
        queue.cancel(id).await;

        // Should have fired at least 2 times (tick, then 10ms, 20ms)
        let final_count = count.load(Ordering::SeqCst);
        assert!(final_count >= 2, "Expected at least 2 firings, got {}", final_count);
    }

    #[tokio::test]
    async fn test_timer_interval_cancel() {
        let queue = TimerQueue::new();
        let fired = Arc::new(AtomicBool::new(false));
        let fired_clone = fired.clone();

        let id = queue.schedule_interval(50, move || {
            fired_clone.store(true, Ordering::SeqCst);
        }).await;

        // Cancel immediately
        queue.cancel(id).await;

        sleep(Duration::from_millis(100)).await;

        assert!(!fired.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_active_count() {
        let queue = TimerQueue::new();

        assert_eq!(queue.active_count().await, 0);

        let id = queue.schedule(100, || {}).await;
        assert_eq!(queue.active_count().await, 1);

        queue.cancel(id).await;
        
        // After cancel, it may take a moment for the task to clean up
        sleep(Duration::from_millis(10)).await;
        assert_eq!(queue.active_count().await, 0);
    }
}
