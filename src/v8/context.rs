//! V8 context management with proper HandleScope nesting
//!
//! This module provides context creation and scope management following
//! the nested HandleScope pattern (D-04 from PITFALLS.md) to prevent
//! memory leaks during script execution.
//!
//! # HandleScope Nesting Pattern
//!
//! The critical pattern for V8 memory safety:
//! 1. Create HandleScope for the operation
//! 2. Create context within that scope
//! 3. Create ContextScope to enter the context
//! 4. Perform script operations
//! 5. Scopes drop automatically (RAII), freeing temporary handles
//!
//! Reference: PITFALLS.md §2 - Handle Scope Misuse Causing Memory Leaks

/// Create a new V8 context within the given HandleScope
///
/// This function creates a context with default global template.
/// The context is valid as long as the HandleScope remains alive.
///
/// # Example
/// ```
/// use nano::v8::{initialize_platform, NanoIsolate};
/// use nano::v8::context::create_context;
///
/// initialize_platform().unwrap();
/// let mut isolate = NanoIsolate::new().unwrap();
/// let mut handle_scope = v8::HandleScope::new(isolate.isolate());
/// let context = create_context(&mut handle_scope);
/// // Context is now ready for script execution
/// ```
pub fn create_context<'s>(scope: &mut v8::HandleScope<'s, ()>) -> v8::Local<'s, v8::Context> {
    // Create context with default global template
    v8::Context::new(scope, Default::default())
}

#[cfg(test)]
mod tests {
    use crate::v8::{platform, NanoIsolate};

    /// Helper to ensure platform is initialized for tests
    fn init_platform() {
        platform::initialize_platform().expect("Failed to initialize V8 platform");
    }

    /// Test that we can create a context using proper HandleScope nesting
    #[test]
    fn test_create_context() {
        init_platform();

        let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

        // Create HandleScope for the operation
        let mut handle_scope = v8::HandleScope::new(isolate.isolate());

        // Create context within the scope
        let _context = super::create_context(&mut handle_scope);

        // Context created successfully - test passes if no crash
    }

    /// Test the nested scope pattern (critical for memory safety per D-04)
    #[test]
    fn test_nested_scope_pattern() {
        init_platform();

        let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

        // Scope 1: HandleScope for the operation
        let mut scope = v8::HandleScope::new(isolate.isolate());
        let context = super::create_context(&mut scope);

        // Scope 2: ContextScope to enter the context
        let _context_scope = v8::ContextScope::new(&mut scope, context);

        // Within context_scope, we can execute scripts
        // When context_scope drops, we exit the context
        // When scope drops, temporary handles are freed
    }

    /// Test creating context via NanoIsolate helper
    #[test]
    fn test_isolate_create_context() {
        init_platform();

        let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
        let _context = isolate.create_context();

        // Context created successfully
    }
}
