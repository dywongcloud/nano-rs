//! JavaScript handler that loads and executes a WASM module
//!
//! This demonstrates WebAssembly integration in NANO runtime.
//! The WASM module exports an "add" function that takes 2 i32 and returns i32.

export default {
    async fetch(request) {
        try {
            // Load WASM bytes from filesystem
            const wasmBytes = await Nano.fs.readFile('./add.wasm');
            
            // Validate WASM before compilation
            const isValid = WebAssembly.validate(wasmBytes);
            if (!isValid) {
                return new Response(JSON.stringify({
                    error: 'Invalid WASM module'
                }), { 
                    status: 500,
                    headers: { 'Content-Type': 'application/json' }
                });
            }
            
            // Compile the WASM module
            const module = await WebAssembly.compile(wasmBytes);
            
            // Instantiate with empty imports (our add function doesn't need imports)
            const instance = await WebAssembly.instantiate(module, {});
            
            // Get URL parameters
            const url = new URL(request.url);
            const a = parseInt(url.searchParams.get('a') || '5');
            const b = parseInt(url.searchParams.get('b') || '3');
            
            // Call exported WASM function
            const result = instance.exports.add(a, b);
            
            // Return result as JSON
            return new Response(JSON.stringify({
                operation: 'add',
                inputs: { a, b },
                result: result,
                wasm_valid: isValid,
                wasm_size: wasmBytes.length
            }, null, 2), {
                status: 200,
                headers: { 'Content-Type': 'application/json' }
            });
            
        } catch (error) {
            return new Response(JSON.stringify({
                error: error.message,
                stack: error.stack
            }, null, 2), { 
                status: 500,
                headers: { 'Content-Type': 'application/json' }
            });
        }
    }
}
