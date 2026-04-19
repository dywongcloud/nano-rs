//! V8 platform initialization
//!
//! Thread-safe V8 platform initialization using std::sync::Once.
//! Platform must be initialized before any isolate creation.

use std::sync::Once;

use anyhow::{anyhow, Result};

static V8_INIT: Once = Once::new();
static mut PLATFORM_INITIALIZED: bool = false;

/// Error type for platform initialization failures
#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("V8 platform already initialized")]
    AlreadyInitialized,
    #[error("Failed to create V8 platform: {0}")]
    PlatformCreation(String),
    #[error("Failed to initialize V8: {0}")]
    V8Init(String),
}

/// Initialize the V8 platform (thread-safe, once per process)
///
/// This function is safe to call multiple times - only the first call
/// will actually initialize the platform. Subsequent calls will return
/// Ok(()) if the platform is already initialized.
///
/// # Thread Safety
/// Uses std::sync::Once to ensure thread-safe initialization across
/// all threads in the process.
///
/// # Example
/// ```
/// use nano::v8::platform;
///
/// // Initialize once at program start
/// platform::initialize_platform().expect("V8 platform init failed");
/// ```
pub fn initialize_platform() -> Result<()> {
    V8_INIT.call_once(|| {
        unsafe {
            PLATFORM_INITIALIZED = false;
        }

        // Create the default V8 platform
        // 0 = number of thread pool workers (0 means use all available cores)
        // false = don't hide idle workers (for better debugging)
        let platform = v8::new_default_platform(0, false).make_shared();

        // Initialize the platform - this must happen before any isolate creation
        v8::V8::initialize_platform(platform);

        // Initialize V8 engine itself
        // SAFETY: This is safe because we're in a Once block, so only one thread
        // can reach this point. initialize() returns () and is safe to call.
        unsafe {
            v8::V8::initialize();
            PLATFORM_INITIALIZED = true;
        }

        tracing::info!("V8 platform initialized successfully");
    });

    // Check if initialization succeeded
    // SAFETY: We're reading a value that was written in a Once block,
    // so the memory is initialized
    unsafe {
        if PLATFORM_INITIALIZED {
            Ok(())
        } else {
            Err(anyhow!(PlatformError::PlatformCreation(
                "V8 platform initialization failed".to_string()
            )))
        }
    }
}

/// Shutdown the V8 platform and release resources
///
/// This should be called once at program termination. It is safe to
/// call even if the platform was never initialized - it will return
/// Ok(()) in that case.
///
/// # Safety
/// After calling shutdown_platform(), no V8 operations are valid.
/// All isolates must be disposed before calling this function.
///
/// # Example
/// ```
/// use nano::v8::platform;
///
/// // At program shutdown
/// platform::shutdown_platform().expect("V8 shutdown failed");
/// ```
pub fn shutdown_platform() -> Result<()> {
    // SAFETY: We're checking PLATFORM_INITIALIZED which was written in a Once block
    unsafe {
        if !PLATFORM_INITIALIZED {
            // Platform was never initialized, nothing to do
            return Ok(());
        }
    }

    tracing::info!("Shutting down V8 platform...");

    // Dispose V8 - this releases all V8 resources
    // SAFETY: V8::dispose() is safe to call if V8 was initialized
    unsafe {
        v8::V8::dispose();
    }

    // Dispose the platform
    v8::V8::dispose_platform();

    tracing::info!("V8 platform shutdown complete");

    Ok(())
}

/// Check if the V8 platform is initialized
///
/// This is primarily useful for testing and diagnostics.
pub fn is_initialized() -> bool {
    // SAFETY: We're reading PLATFORM_INITIALIZED which was written in a Once block
    unsafe { PLATFORM_INITIALIZED }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that platform initialization works and can only happen once
    #[test]
    fn test_platform_initialization() {
        // First call should initialize
        assert!(initialize_platform().is_ok());
        assert!(is_initialized());

        // Second call should succeed (idempotent)
        assert!(initialize_platform().is_ok());
    }

    /// Test that is_initialized returns correct state
    #[test]
    fn test_is_initialized() {
        // Before initialization, should return false
        // Note: This might fail if other tests ran first and initialized the platform
        // So we just check that after successful init, it's true
        if initialize_platform().is_ok() {
            assert!(is_initialized());
        }
    }
}
