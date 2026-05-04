//! V8 API Abstractions for v147+ Compatibility
//!
//! This module provides helper types and functions to work with the new V8 v147
//! API which uses Pin<ScopeStorage> and PinnedRef patterns instead of direct
//! HandleScope references.
//!
//! # V147 API Pattern
//!
//! The key pattern in v147:
//! ```rust,ignore
//! let scope = std::pin::pin!(v8::HandleScope::new(isolate));
//! let mut scope = scope.init();
//! // scope is now PinnedRef<HandleScope> which can be used with V8 APIs
//! ```
//!
//! Note: Many internal traits are private in v147, so we use concrete types
//! and inline the pin! + init() pattern rather than trying to abstract over
//! scope types with generics.

/// Type alias for the pinned HandleScope (post-init)
///
/// This is the type you get after calling `.init()` on the pinned ScopeStorage.
/// It's a PinnedRef that can be used directly with V8 APIs.
pub type PinnedHandleScope<'a, 'b> = v8::PinnedRef<'a, v8::HandleScope<'b, ()>>;

/// Initialize a HandleScope and return a PinnedRef to it
///
/// This creates a HandleScope for an isolate using the v147 pattern.
///
/// # Example
/// ```rust,ignore
/// let scope = init_handle_scope(&mut isolate);
/// let context = v8::Context::new(&scope, Default::default());
/// ```
pub fn init_handle_scope<'a>(
    isolate: &'a mut v8::Isolate,
) -> v8::PinnedRef<'a, v8::HandleScope<'a, ()>> {
    let scope = std::pin::pin!(v8::HandleScope::new(isolate));
    scope.init()
}

/// Initialize a nested HandleScope from an existing scope
///
/// # Example
/// ```rust,ignore
/// let nested_scope = init_nested_handle_scope(&mut scope);
/// ```
pub fn init_nested_handle_scope<'a, 'b>(
    parent: &mut v8::PinnedRef<'_, v8::HandleScope<'b, ()>>,
) -> v8::PinnedRef<'a, v8::HandleScope<'a, ()>> {
    let scope = std::pin::pin!(v8::HandleScope::new(parent));
    scope.init()
}

/// Helper to create a context from a pinned scope
///
/// # Example
/// ```rust,ignore
/// let context = create_context_from_scope(&scope);
/// ```
pub fn create_context_from_scope<'a, 'b>(
    scope: &v8::PinnedRef<'_, v8::HandleScope<'b, ()>>,
) -> v8::Local<'b, v8::Context> {
    // In v147, Context::new takes &PinnedRef
    v8::Context::new(scope, Default::default())
}

/// Helper to create a Global from a pinned scope
///
/// In v147, Global::new takes &Isolate directly.
///
/// # Example
/// ```rust,ignore
/// let global = create_global_from_scope(&scope, local_value);
/// ```
pub fn create_global_from_scope<T>(
    scope: &v8::PinnedRef<'_, v8::HandleScope<'_, ()>>,
    handle: v8::Local<T>,
) -> v8::Global<T> {
    // Global::new needs &Isolate - we get it from the HandleScope
    // The HandleScope derefs to &Isolate
    v8::Global::new(&**scope, handle)
}

/// Helper to get undefined from a pinned scope
///
/// # Example
/// ```rust,ignore
/// let undefined = undefined_from_scope(&scope);
/// ```
pub fn undefined_from_scope<'a, 'b>(
    scope: &v8::PinnedRef<'_, v8::HandleScope<'b, ()>>,
) -> v8::Local<'b, v8::Primitive> {
    v8::undefined(scope)
}

/// Helper to get null from a pinned scope
///
/// # Example
/// ```rust,ignore
/// let null = null_from_scope(&scope);
/// ```
pub fn null_from_scope<'a, 'b>(
    scope: &v8::PinnedRef<'_, v8::HandleScope<'b, ()>>,
) -> v8::Local<'b, v8::Primitive> {
    v8::null(scope)
}

/// Helper to create a String from a pinned scope
///
/// # Example
/// ```rust,ignore
/// let str = string_from_scope(&scope, "hello");
/// ```
pub fn string_from_scope<'a, 'b>(
    scope: &v8::PinnedRef<'_, v8::HandleScope<'b, ()>>,
    value: &str,
) -> Option<v8::Local<'b, v8::String>> {
    v8::String::new(scope, value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v8::platform;

    #[test]
    fn test_init_handle_scope() {
        platform::initialize_platform().unwrap();

        let mut isolate = v8::Isolate::new(Default::default());
        let _scope = init_handle_scope(&mut isolate);
        // Test that we can create a scope without crashing
    }

    #[test]
    fn test_create_context_from_scope() {
        platform::initialize_platform().unwrap();

        let mut isolate = v8::Isolate::new(Default::default());
        let scope = init_handle_scope(&mut isolate);
        let _context = create_context_from_scope(&scope);
        // Test that we can create a context without crashing
    }
}
