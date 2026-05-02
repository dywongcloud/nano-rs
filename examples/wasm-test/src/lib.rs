//! Simple WASM module for NANO integration testing
//! 
//! Build: cargo build --target wasm32-unknown-unknown --release
//! Output: target/wasm32-unknown-unknown/release/nano_wasm_example.wasm

/// Add two integers - exported for JavaScript
#[no_mangle]
pub extern "C" fn add(a: i32, b: i32) -> i32 {
    a + b
}

/// Multiply two integers - exported for JavaScript
#[no_mangle]
pub extern "C" fn multiply(a: i32, b: i32) -> i32 {
    a * b
}

/// Factorial calculation - demonstrates CPU usage
#[no_mangle]
pub extern "C" fn factorial(n: i32) -> i32 {
    if n <= 1 {
        1
    } else {
        n * factorial(n - 1)
    }
}

/// Memory for string operations
static mut BUFFER: [u8; 1024] = [0; 1024];

/// Get pointer to buffer for JS to read
#[no_mangle]
pub extern "C" fn get_buffer_ptr() -> *mut u8 {
    unsafe { BUFFER.as_mut_ptr() }
}

/// Write a greeting to the buffer
#[no_mangle]
pub extern "C" fn write_greeting(name_ptr: *const u8, name_len: i32) -> i32 {
    if name_ptr.is_null() || name_len <= 0 || name_len > 100 {
        return -1;
    }
    
    let greeting = b"Hello, ";
    let name = unsafe { std::slice::from_raw_parts(name_ptr, name_len as usize) };
    
    unsafe {
        // Write "Hello, "
        let mut offset = 0;
        for &byte in greeting.iter() {
            if offset >= BUFFER.len() {
                break;
            }
            BUFFER[offset] = byte;
            offset += 1;
        }
        
        // Write name
        for &byte in name.iter() {
            if offset >= BUFFER.len() {
                break;
            }
            BUFFER[offset] = byte;
            offset += 1;
        }
        
        // Null terminate
        if offset < BUFFER.len() {
            BUFFER[offset] = 0;
        }
    }
    
    0 // Success
}
