//! WebAssembly Implementation
//!
//! Provides first-class WebAssembly support equivalent to JavaScript execution.
//! Uses V8's native WasmModuleObject API for compilation and execution.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// WASM module cache for compiled modules
/// 
/// CompiledWasmModule can be shared between isolates and contexts via Arc,
/// providing fast module restoration without recompilation.
pub struct WasmModuleCache {
    /// Map from module hash to compiled module wrapped in Arc
    /// 
    /// The hash is computed from the original WASM wire bytes.
    /// CompiledWasmModule is Send + Sync, so it can be shared safely via Arc.
    modules: Arc<Mutex<HashMap<String, Arc<v8::CompiledWasmModule>>>>,
}

impl WasmModuleCache {
    /// Create a new empty module cache
    pub fn new() -> Self {
        Self {
            modules: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    
    /// Get a compiled module from cache
    /// 
    /// # Arguments
    /// * `hash` - The hash of the WASM bytes
    /// 
    /// # Returns
    /// * `Some(Arc<CompiledWasmModule>)` if found in cache
    /// * `None` if not cached
    pub fn get(&self, hash: &str) -> Option<Arc<v8::CompiledWasmModule>> {
        let modules = self.modules.lock().unwrap();
        modules.get(hash).cloned()
    }
    
    /// Store a compiled module in cache
    /// 
    /// # Arguments
    /// * `hash` - The hash of the WASM bytes
    /// * `module` - The compiled module to cache (wrapped in Arc)
    pub fn store(&self, hash: &str, module: Arc<v8::CompiledWasmModule>) {
        let mut modules = self.modules.lock().unwrap();
        modules.insert(hash.to_string(), module);
    }
    
    /// Check if a module is cached
    pub fn contains(&self, hash: &str) -> bool {
        let modules = self.modules.lock().unwrap();
        modules.contains_key(hash)
    }
    
    /// Clear the cache
    pub fn clear(&self) {
        let mut modules = self.modules.lock().unwrap();
        modules.clear();
    }
}

impl Default for WasmModuleCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Compile WASM bytes into a module
/// 
/// This function uses V8's native synchronous compilation API.
/// It first checks the cache, then compiles if not found.
/// 
/// # Arguments
/// * `scope` - The V8 scope for compilation
/// * `bytes` - The WASM wire bytes to compile
/// * `cache` - Optional module cache for deduplication
/// 
/// # Returns
/// * `Ok(Local<WasmModuleObject>)` - The compiled module
/// * `Err(WasmCompileError)` - Compilation failed
pub fn compile_module<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    bytes: &[u8],
    cache: Option<&WasmModuleCache>,
) -> Result<v8::Local<'s, v8::WasmModuleObject>, WasmCompileError> {
    // Compute hash for cache lookup
    let hash = compute_hash(bytes);
    
    // Check cache first
    if let Some(cache) = cache {
        if let Some(compiled) = cache.get(&hash) {
            // Create module from cached compiled module
            // Deref Arc to get &CompiledWasmModule
            return v8::WasmModuleObject::from_compiled_module(scope, &compiled)
                .ok_or_else(|| WasmCompileError::CacheCorrupted);
        }
    }
    
    // Compile the module
    match v8::WasmModuleObject::compile(scope, bytes) {
        Some(module) => {
            // Store in cache if provided
            if let Some(cache) = cache {
                let compiled = module.get_compiled_module();
                cache.store(&hash, Arc::new(compiled));
            }
            Ok(module)
        }
        None => Err(WasmCompileError::CompilationFailed),
    }
}

/// Compute a hash of WASM bytes for cache keys
fn compute_hash(bytes: &[u8]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// WASM compilation errors
#[derive(Debug, Clone)]
pub enum WasmCompileError {
    /// V8 compilation failed (invalid WASM or V8 error)
    CompilationFailed,
    /// Cache entry exists but is corrupted
    CacheCorrupted,
    /// Module too large
    ModuleTooLarge,
}

impl std::fmt::Display for WasmCompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WasmCompileError::CompilationFailed => write!(f, "WASM compilation failed"),
            WasmCompileError::CacheCorrupted => write!(f, "Cached module corrupted"),
            WasmCompileError::ModuleTooLarge => write!(f, "WASM module exceeds size limit"),
        }
    }
}

impl std::error::Error for WasmCompileError {}

/// Validate WASM bytes without compiling
/// 
/// This is a fast check that only validates the magic number and version.
/// Use this for quick rejection of invalid input.
pub fn validate_wasm_bytes(bytes: &[u8]) -> Result<(), WasmValidationError> {
    if bytes.len() < 8 {
        return Err(WasmValidationError::TooSmall);
    }
    
    // Check magic number: \0asm
    if &bytes[0..4] != b"\0asm" {
        return Err(WasmValidationError::InvalidMagic);
    }
    
    // Check version (1.0 or 2.0)
    let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if version != 1 && version != 2 {
        return Err(WasmValidationError::UnsupportedVersion(version));
    }
    
    Ok(())
}

/// WASM validation errors
#[derive(Debug, Clone)]
pub enum WasmValidationError {
    /// File too small to be valid WASM
    TooSmall,
    /// Invalid magic number
    InvalidMagic,
    /// Unsupported version
    UnsupportedVersion(u32),
}

impl std::fmt::Display for WasmValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WasmValidationError::TooSmall => write!(f, "WASM file too small"),
            WasmValidationError::InvalidMagic => write!(f, "Invalid WASM magic number"),
            WasmValidationError::UnsupportedVersion(v) => write!(f, "Unsupported WASM version: {}", v),
        }
    }
}

impl std::error::Error for WasmValidationError {}

#[cfg(test)]
mod tests {
    use super::*;
    
    /// Minimal valid WASM v1.0 module
    fn minimal_wasm() -> Vec<u8> {
        vec![
            0x00, 0x61, 0x73, 0x6d,  // magic: \0asm
            0x01, 0x00, 0x00, 0x00,  // version: 1
        ]
    }
    
    /// WASM module with add function
    fn add_wasm() -> Vec<u8> {
        vec![
            // Magic + version
            0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00,
            // Type section (section id 1)
            0x01, 0x07, 0x01, 0x60, 0x02, 0x7f, 0x7f, 0x01, 0x7f,
            // Function section (section id 3)
            0x03, 0x02, 0x01, 0x00,
            // Export section (section id 7)
            0x07, 0x08, 0x01, 0x03, 0x61, 0x64, 0x64, 0x00, 0x00,
            // Code section (section id 10)
            0x0a, 0x09, 0x01, 0x07, 0x00, 0x20, 0x00, 0x20, 0x01, 0x6a, 0x0b,
        ]
    }
    
    #[test]
    fn test_validate_valid_wasm() {
        let wasm = minimal_wasm();
        assert!(validate_wasm_bytes(&wasm).is_ok());
        
        let add = add_wasm();
        assert!(validate_wasm_bytes(&add).is_ok());
    }
    
    #[test]
    fn test_validate_invalid_magic() {
        let invalid = b"wasm\x01\x00\x00\x00";
        assert!(validate_wasm_bytes(invalid).is_err());
    }
    
    #[test]
    fn test_validate_too_small() {
        let too_small = b"\0asm";
        assert!(validate_wasm_bytes(too_small).is_err());
    }
    
    #[test]
    fn test_cache_hash_computation() {
        let wasm1 = minimal_wasm();
        let wasm2 = minimal_wasm();
        let different = add_wasm();
        
        let hash1 = compute_hash(&wasm1);
        let hash2 = compute_hash(&wasm2);
        let hash3 = compute_hash(&different);
        
        assert_eq!(hash1, hash2, "Same bytes should have same hash");
        assert_ne!(hash1, hash3, "Different bytes should have different hash");
    }
    
    #[test]
    fn test_module_cache_store_and_retrieve() {
        let cache = WasmModuleCache::new();
        
        // Note: We can't test actual module storage without a V8 isolate,
        // but we can test the cache operations
        assert!(!cache.contains("test_hash"));
        
        // Store and retrieve would require a real CompiledWasmModule
        // which we can only get from V8 compilation
    }
}
