//! V8 Context Manager for fast context reset between requests
//!
//! This module provides the ContextManager which manages V8 context lifecycle
//! for security isolation between requests. Instead of recreating the entire
//! isolate (50-100ms), we reset just the context (<10ms target).
//!
//! ## Performance Target
//!
//! - Context reset: <10ms (dispose + recreate context)
//! - Full isolate recreation: 50-100ms (avoided)
//!
//! ## Security Model
//!
//! - Context reset clears JavaScript global scope between requests
//! - Each request gets a fresh context in the same isolate
//! - State leakage between tenants is prevented by context disposal
//!
//! ## Implementation Notes
//!
//! This implementation uses `v8::Global<v8::Context>` to store contexts across
//! scope boundaries. Local handles are temporary and scope-bound, while Globals
//! survive across HandleScope lifetimes.
//!
//! The ContextManager owns the NanoIsolate to avoid borrow checker issues
//! when both context management and script execution need isolate access.

use crate::v8::NanoIsolate;
use anyhow::Result;
use std::time::Duration;

/// Manages V8 context lifecycle for a worker thread
///
/// The ContextManager owns the NanoIsolate and manages the creation,
/// disposal, and reset of V8 contexts using v8::Global to survive across
/// scope boundaries. It tracks performance metrics for context reset operations.
pub struct ContextManager {
    isolate: NanoIsolate,
    current_context: Option<v8::Global<v8::Context>>,
    creation_count: u64,
    reset_count: u64,
    total_reset_time_ms: f64,
}

impl ContextManager {
    /// Create a new ContextManager with the given isolate
    pub fn new(isolate: NanoIsolate) -> Self {
        Self {
            isolate,
            current_context: None,
            creation_count: 0,
            reset_count: 0,
            total_reset_time_ms: 0.0,
        }
    }

    /// Create the initial context for this isolate
    pub fn create_initial_context(&mut self) -> Result<()> {
        let scope = &mut v8::HandleScope::new(self.isolate.isolate());
        let context = v8::Context::new(scope, Default::default());
        let global_context = v8::Global::new(scope, context);

        self.current_context = Some(global_context);
        self.creation_count = 1;

        tracing::debug!("Created initial V8 context");
        Ok(())
    }

    /// Reset the context by disposing the current one and creating a new one
    pub fn reset_context(&mut self) -> Result<Duration> {
        let start = std::time::Instant::now();

        // Dispose current context
        self.current_context = None;

        // Create new context with clean global scope
        let scope = &mut v8::HandleScope::new(self.isolate.isolate());
        let new_context = v8::Context::new(scope, Default::default());
        let global_context = v8::Global::new(scope, new_context);

        self.current_context = Some(global_context);
        self.creation_count += 1;
        self.reset_count += 1;

        let elapsed = start.elapsed();
        self.total_reset_time_ms += elapsed.as_secs_f64() * 1000.0;

        tracing::debug!(
            "Context reset completed in {:.2}ms (count: {})",
            elapsed.as_secs_f64() * 1000.0,
            self.reset_count
        );

        Ok(elapsed)
    }

    /// Reset context and return a Local reference to the new context
    pub fn reset_and_get_context<'s>(
        &mut self,
        scope: &mut v8::HandleScope<'s, ()>,
    ) -> Result<(Duration, v8::Local<'s, v8::Context>)> {
        let elapsed = self.reset_context()?;
        let context = self
            .context(scope)
            .ok_or_else(|| anyhow::anyhow!("Context unavailable after reset"))?;
        Ok((elapsed, context))
    }

    /// Get a Local reference to the current context for execution
    pub fn context<'s>(
        &self,
        scope: &mut v8::HandleScope<'s, ()>,
    ) -> Option<v8::Local<'s, v8::Context>> {
        self.current_context
            .as_ref()
            .map(|global| v8::Local::new(scope, global))
    }

    /// Get a mutable reference to the underlying isolate
    pub fn isolate_mut(&mut self) -> &mut NanoIsolate {
        &mut self.isolate
    }

    /// Get a reference to the VFS
    pub fn vfs(&self) -> Option<&crate::vfs::IsolateVfs> {
        Some(self.isolate.vfs())
    }

    /// Clone the current Global<Context>
    ///
    /// This can be used to reopen a Local<Context> within a HandleScope.
    /// Cloning a Global is cheap - it's just a handle reference.
    pub fn clone_context(&self) -> Option<v8::Global<v8::Context>> {
        self.current_context.clone()
    }

    /// Execute a function with the current context and isolate
    ///
    /// This method handles the borrow checker issues by using raw pointers.
    /// The caller is responsible for creating HandleScope and ContextScope.
    ///
    /// NOTE: This method reopens the Global<Context> to get a fresh Local.
    /// The returned Local is only valid within the caller's HandleScope.
    pub fn get_context_for_execution(&mut self) -> Option<*mut v8::OwnedIsolate> {
        // Just return the isolate pointer - caller will create scopes
        Some(self.isolate.isolate())
    }

    /// Check if a context is available
    pub fn has_context(&self) -> bool {
        self.current_context.is_some()
    }

    /// Get the average reset time in milliseconds
    pub fn average_reset_time_ms(&self) -> f64 {
        if self.reset_count > 0 {
            self.total_reset_time_ms / self.reset_count as f64
        } else {
            0.0
        }
    }

    /// Get the total number of contexts created
    pub fn creation_count(&self) -> u64 {
        self.creation_count
    }

    /// Get the total number of context resets performed
    pub fn reset_count(&self) -> u64 {
        self.reset_count
    }

    /// Get the total time spent in context reset operations
    pub fn total_reset_time_ms(&self) -> f64 {
        self.total_reset_time_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v8::platform;

    fn init_platform() {
        platform::initialize_platform().expect("Failed to initialize V8 platform");
    }

    #[test]
    fn test_context_manager_creation() {
        init_platform();

        let isolate = NanoIsolate::new().expect("Failed to create isolate");
        let manager = ContextManager::new(isolate);

        assert_eq!(manager.creation_count(), 0);
        assert_eq!(manager.reset_count(), 0);
        assert!(!manager.has_context());
    }

    #[test]
    fn test_create_initial_context() {
        init_platform();

        let isolate = NanoIsolate::new().expect("Failed to create isolate");
        let mut manager = ContextManager::new(isolate);

        manager
            .create_initial_context()
            .expect("Failed to create context");
        assert!(manager.has_context());
        assert_eq!(manager.creation_count(), 1);
    }

    #[test]
    fn test_reset_context() {
        init_platform();

        let isolate = NanoIsolate::new().expect("Failed to create isolate");
        let mut manager = ContextManager::new(isolate);

        manager
            .create_initial_context()
            .expect("Failed to create context");

        let elapsed = manager.reset_context().expect("Failed to reset context");

        assert!(manager.has_context());
        assert_eq!(manager.creation_count(), 2);
        assert_eq!(manager.reset_count(), 1);
        assert!(elapsed.as_secs_f64() > 0.0);
    }

    #[test]
    fn test_reset_timing() {
        init_platform();

        let isolate = NanoIsolate::new().expect("Failed to create isolate");
        let mut manager = ContextManager::new(isolate);

        manager
            .create_initial_context()
            .expect("Failed to create context");

        let elapsed = manager.reset_context().expect("Failed to reset context");
        let ms = elapsed.as_secs_f64() * 1000.0;

        println!("Context reset took: {:.2}ms", ms);
        assert!(ms < 100.0, "Context reset took too long: {:.2}ms", ms);
    }
}
