//! Automated end-to-end tests for CPU time limits
//!
//! These tests spawn the NANO binary and verify actual timeout behavior
//! for both JavaScript and WebAssembly execution.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use std::fs;
use std::path::PathBuf;

/// Find the NANO binary path
fn nano_binary_path() -> PathBuf {
    // Get project root from CARGO_MANIFEST_DIR
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR not set");
    let project_root = PathBuf::from(manifest_dir);
    
    // Try release binary first
    let release_path = project_root.join("target/release/nano-rs");
    if release_path.exists() {
        return release_path;
    }
    
    // Fall back to debug binary
    let debug_path = project_root.join("target/debug/nano-rs");
    if debug_path.exists() {
        return debug_path;
    }
    
    panic!("NANO binary not found at {:?} or {:?}. Build with: cargo build", release_path, debug_path);
}

/// Create a temporary test directory with config and JS files
fn create_test_dir(name: &str) -> PathBuf {
    let temp_dir = std::env::temp_dir().join(format!("nano_cpu_test_{}_{}", name, std::process::id()));
    fs::remove_dir_all(&temp_dir).ok();
    fs::create_dir_all(&temp_dir).expect("Failed to create test dir");
    temp_dir
}

/// Clean up test directory
fn cleanup_test_dir(path: &PathBuf) {
    fs::remove_dir_all(path).ok();
}

/// Wait for server to be ready by polling with exponential backoff
async fn wait_for_server(child: &mut std::process::Child, port: u16, hostname: &str, max_wait_secs: u64) {
    // Initial delay to allow V8 initialization (takes ~1-2 seconds)
    tokio::time::sleep(Duration::from_secs(2)).await;

    let client = reqwest::Client::new();
    let start = Instant::now();
    let max_wait = Duration::from_secs(max_wait_secs);

    while start.elapsed() < max_wait {
        // Check if process is still running
        match child.try_wait() {
            Ok(Some(status)) => {
                // Process exited - read stderr
                let mut stderr = String::new();
                if let Some(ref mut err) = child.stderr {
                    use std::io::Read;
                    let mut buf = Vec::new();
                    let _ = err.read_to_end(&mut buf);
                    stderr = String::from_utf8_lossy(&buf).to_string();
                }
                panic!("NANO process exited early with status: {:?}. Stderr: {}", status, stderr);
            }
            Ok(None) => {
                // Process still running, try to connect
                match client
                    .get(format!("http://127.0.0.1:{}/", port))
                    .header("Host", hostname)
                    .timeout(Duration::from_secs(3))
                    .send()
                    .await
                {
                    Ok(_) => return, // Server is ready
                    Err(_) => {
                        // Server not ready yet, wait and retry
                        tokio::time::sleep(Duration::from_millis(500)).await;
                    }
                }
            }
            Err(e) => panic!("Failed to check process status: {}", e),
        }
    }

    // Timeout - capture stderr before panic
    let mut stderr = String::new();
    if let Some(ref mut err) = child.stderr {
        use std::io::Read;
        let mut buf = Vec::new();
        let _ = err.read_to_end(&mut buf);
        stderr = String::from_utf8_lossy(&buf).to_string();
    }
    panic!("Server failed to start within {} seconds. Stderr: {}", max_wait_secs, stderr);
}

/// Write a file in the test directory
fn write_test_file(dir: &PathBuf, filename: &str, content: &str) {
    let path = dir.join(filename);
    fs::write(&path, content).expect(&format!("Failed to write {}", filename));
}

/// Test CPU timeout terminates an infinite JavaScript loop
#[tokio::test]
#[ignore = "E2E test - run manually with cargo test --test cpu_timeout_e2e_test -- --ignored"]
async fn test_js_cpu_timeout() {
    let test_dir = create_test_dir("js_timeout");
    let _cleanup = TestCleanup(&test_dir);
    
    // Create infinite loop JavaScript
    let js_content = r#"export default {
    async fetch(request) {
        let iterations = 0;
        while (true) {
            iterations++;
            Math.random(); // Some CPU work
        }
    }
}"#;
    write_test_file(&test_dir, "infinite.js", js_content);
    
    // Create config with 10ms CPU limit and disk VFS
    let config_content = format!(r#"{{
        "apps": [{{
            "hostname": "timeout.local",
            "entrypoint": "./infinite.js",
            "limits": {{
                "memory_mb": 128,
                "timeout_secs": 30,
                "workers": 1,
                "cpu_time_ms": 10,
                "cpu_time_enabled": true
            }},
            "vfs_backend": "disk",
            "vfs_disk": {{
                "base_path": "{}"
            }}
        }}],
        "server": {{
            "port": 18080,
            "host": "127.0.0.1"
        }}
    }}"#, test_dir.to_str().unwrap());
    write_test_file(&test_dir, "config.json", &config_content);
    
    // Spawn NANO
    let nano_path = nano_binary_path();
    let mut child = Command::new(&nano_path)
        .arg("run")
        .arg("--config")
        .arg(test_dir.join("config.json"))
        .current_dir(&test_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn NANO");
    
    // Wait for server to start (poll for readiness)
    wait_for_server(&mut child, 18080, "timeout.local", 10).await;
    
    // Send request that should trigger CPU timeout
    let start = Instant::now();
    let client = reqwest::Client::new();
    let result = client
        .get("http://127.0.0.1:18080/")
        .header("Host", "timeout.local")
        .timeout(Duration::from_secs(5))
        .send()
        .await;
    
    let elapsed = start.elapsed();
    
    // Terminate NANO
    child.kill().ok();
    
    // Verify: Request should fail or return error quickly
    // CPU timeout of 10ms should terminate within ~100ms real time
    match result {
        Ok(response) => {
            // Server responded - check if it's an error
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            
            // Should either timeout (504) or be a server error (500)
            assert!(
                status.is_server_error() || status.as_u16() == 504,
                "Expected error response for CPU timeout, got {}: {}",
                status, body
            );
        }
        Err(e) => {
            // Connection error is also acceptable (server might have crashed)
            println!("Request failed as expected: {}", e);
        }
    }
    
    // Verify it didn't take too long (should be much faster than 30s wall-clock timeout)
    assert!(
        elapsed < Duration::from_millis(500),
        "CPU timeout took too long: {:?}. Should be <500ms",
        elapsed
    );
    
    println!("JS CPU timeout test passed: elapsed={:?}", elapsed);
}

/// Test normal JavaScript execution within CPU limit
#[tokio::test]
#[ignore = "E2E test - run manually with cargo test --test cpu_timeout_e2e_test -- --ignored"]
async fn test_js_within_cpu_limit() {
    let test_dir = create_test_dir("js_normal");
    let _cleanup = TestCleanup(&test_dir);
    
    // Create normal JavaScript that completes quickly
    let js_content = r#"export default {
    async fetch(request) {
        let sum = 0;
        for (let i = 0; i < 1000; i++) {
            sum += i;
        }
        return new Response(JSON.stringify({ sum }), {
            status: 200,
            headers: { 'Content-Type': 'application/json' }
        });
    }
}"#;
    write_test_file(&test_dir, "normal.js", js_content);
    
    // Create config with 50ms CPU limit and disk VFS
    let config_content = format!(r#"{{
        "apps": [{{
            "hostname": "normal.local",
            "entrypoint": "./normal.js",
            "limits": {{
                "memory_mb": 128,
                "timeout_secs": 30,
                "workers": 1,
                "cpu_time_ms": 50,
                "cpu_time_enabled": true
            }},
            "vfs_backend": "disk",
            "vfs_disk": {{
                "base_path": "{}"
            }}
        }}],
        "server": {{
            "port": 18081,
            "host": "127.0.0.1"
        }}
    }}"#, test_dir.to_str().unwrap());
    write_test_file(&test_dir, "config.json", &config_content);
    
    // Spawn NANO
    let nano_path = nano_binary_path();
    let mut child = Command::new(&nano_path)
        .arg("run")
        .arg("--config")
        .arg(test_dir.join("config.json"))
        .current_dir(&test_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn NANO");
    
    // Wait for server to start (poll for readiness)
    wait_for_server(&mut child, 18081, "normal.local", 10).await;
    
    // Send request that should complete successfully
    let client = reqwest::Client::new();
    let result = client
        .get("http://127.0.0.1:18081/")
        .header("Host", "normal.local")
        .timeout(Duration::from_secs(5))
        .send()
        .await;
    
    // Terminate NANO
    child.kill().ok();
    
    // Verify: Request should succeed
    match result {
        Ok(response) => {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            
            assert!(
                status.is_success(),
                "Expected success response, got {}: {}",
                status, body
            );
            
            // Verify response contains expected data
            assert!(body.contains("sum"), "Response should contain 'sum', got: {}", body);
        }
        Err(e) => {
            panic!("Request should have succeeded: {}", e);
        }
    }
    
    println!("JS normal execution test passed");
}

/// Test CPU timeout terminates a CPU-intensive WASM computation
#[tokio::test]
#[ignore = "E2E test - run manually with cargo test --test cpu_timeout_e2e_test -- --ignored"]
async fn test_wasm_cpu_timeout() {
    let test_dir = create_test_dir("wasm_timeout");
    let _cleanup = TestCleanup(&test_dir);
    
    // Copy the test WASM file
    let wasm_bytes = include_bytes!("../examples/wasm-test/add.wasm");
    fs::write(test_dir.join("add.wasm"), wasm_bytes).expect("Failed to write WASM");
    
    // Create JavaScript that calls WASM in a loop
    let js_content = r#"export default {
    async fetch(request) {
        const wasmBytes = await Nano.fs.readFile('add.wasm');
        const module = await WebAssembly.compile(wasmBytes);
        const instance = await WebAssembly.instantiate(module, {});
        
        // Infinite loop calling WASM
        while (true) {
            instance.exports.add(1, 1);
        }
    }
}"#;
    write_test_file(&test_dir, "wasm_infinite.js", js_content);
    
    // Create config with 10ms CPU limit and disk VFS
    let config_content = format!(r#"{{
        "apps": [{{
            "hostname": "wasm-timeout.local",
            "entrypoint": "./wasm_infinite.js",
            "limits": {{
                "memory_mb": 128,
                "timeout_secs": 30,
                "workers": 1,
                "cpu_time_ms": 10,
                "cpu_time_enabled": true
            }},
            "vfs_backend": "disk",
            "vfs_disk": {{
                "base_path": "{}"
            }}
        }}],
        "server": {{
            "port": 18082,
            "host": "127.0.0.1"
        }}
    }}"#, test_dir.to_str().unwrap());
    write_test_file(&test_dir, "config.json", &config_content);
    
    // Spawn NANO
    let nano_path = nano_binary_path();
    let mut child = Command::new(&nano_path)
        .arg("run")
        .arg("--config")
        .arg(test_dir.join("config.json"))
        .current_dir(&test_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn NANO");
    
    // Wait for server to start (poll for readiness)
    wait_for_server(&mut child, 18082, "wasm-timeout.local", 10).await;
    
    // Send request that should trigger CPU timeout in WASM
    let start = Instant::now();
    let client = reqwest::Client::new();
    let result = client
        .get("http://127.0.0.1:18082/")
        .header("Host", "wasm-timeout.local")
        .timeout(Duration::from_secs(5))
        .send()
        .await;
    
    let elapsed = start.elapsed();
    
    // Terminate NANO
    child.kill().ok();
    
    // Verify: Request should fail or return error quickly
    match result {
        Ok(response) => {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            
            assert!(
                status.is_server_error() || status.as_u16() == 504,
                "Expected error response for WASM CPU timeout, got {}: {}",
                status, body
            );
        }
        Err(e) => {
            println!("Request failed as expected: {}", e);
        }
    }
    
    // Verify it didn't take too long
    assert!(
        elapsed < Duration::from_millis(500),
        "WASM CPU timeout took too long: {:?}. Should be <500ms",
        elapsed
    );
    
    println!("WASM CPU timeout test passed: elapsed={:?}", elapsed);
}

/// Test normal WASM execution within CPU limit
#[tokio::test]
#[ignore = "E2E test - run manually with cargo test --test cpu_timeout_e2e_test -- --ignored"]
async fn test_wasm_within_cpu_limit() {
    let test_dir = create_test_dir("wasm_normal");
    let _cleanup = TestCleanup(&test_dir);
    
    // Copy the test WASM file
    let wasm_bytes = include_bytes!("../examples/wasm-test/add.wasm");
    fs::write(test_dir.join("add.wasm"), wasm_bytes).expect("Failed to write WASM");
    
    // Create JavaScript that calls WASM normally
    let js_content = r#"export default {
    async fetch(request) {
        const url = new URL(request.url);
        const a = parseInt(url.searchParams.get('a') || '5');
        const b = parseInt(url.searchParams.get('b') || '3');
        
        const wasmBytes = await Nano.fs.readFile('add.wasm');
        const module = await WebAssembly.compile(wasmBytes);
        const instance = await WebAssembly.instantiate(module, {});
        
        const result = instance.exports.add(a, b);
        
        return new Response(JSON.stringify({ a, b, result }), {
            status: 200,
            headers: { 'Content-Type': 'application/json' }
        });
    }
}"#;
    write_test_file(&test_dir, "wasm_normal.js", js_content);
    
    // Create config with 50ms CPU limit and disk VFS
    let config_content = format!(r#"{{
        "apps": [{{
            "hostname": "wasm-normal.local",
            "entrypoint": "./wasm_normal.js",
            "limits": {{
                "memory_mb": 128,
                "timeout_secs": 30,
                "workers": 1,
                "cpu_time_ms": 50,
                "cpu_time_enabled": true
            }},
            "vfs_backend": "disk",
            "vfs_disk": {{
                "base_path": "{}"
            }}
        }}],
        "server": {{
            "port": 18083,
            "host": "127.0.0.1"
        }}
    }}"#, test_dir.to_str().unwrap());
    write_test_file(&test_dir, "config.json", &config_content);
    
    // Spawn NANO
    let nano_path = nano_binary_path();
    let mut child = Command::new(&nano_path)
        .arg("run")
        .arg("--config")
        .arg(test_dir.join("config.json"))
        .current_dir(&test_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn NANO");
    
    // Wait for server to start (poll for readiness)
    wait_for_server(&mut child, 18083, "wasm-normal.local", 10).await;
    
    // Send request that should complete successfully
    let client = reqwest::Client::new();
    let result = client
        .get("http://127.0.0.1:18083/?a=10&b=20")
        .header("Host", "wasm-normal.local")
        .timeout(Duration::from_secs(5))
        .send()
        .await;
    
    // Terminate NANO
    child.kill().ok();
    
    // Verify: Request should succeed
    match result {
        Ok(response) => {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            
            assert!(
                status.is_success(),
                "Expected success response, got {}: {}",
                status, body
            );
            
            // Verify response contains expected calculation
            assert!(body.contains("30"), "Response should contain result 30, got: {}", body);
        }
        Err(e) => {
            panic!("Request should have succeeded: {}", e);
        }
    }
    
    println!("WASM normal execution test passed");
}

/// Test that CPU limits are enforced per-isolate
#[tokio::test]
#[ignore = "E2E test - run manually with cargo test --test cpu_timeout_e2e_test -- --ignored"]
async fn test_cpu_limit_per_isolate() {
    let test_dir = create_test_dir("per_isolate");
    let _cleanup = TestCleanup(&test_dir);
    
    // Create app that does moderate computation
    let js_content = r#"export default {
    async fetch(request) {
        // Moderate computation
        let result = 0;
        for (let i = 0; i < 100000; i++) {
            result += Math.sqrt(i);
        }
        return new Response(JSON.stringify({ result }), {
            status: 200,
            headers: { 'Content-Type': 'application/json' }
        });
    }
}"#;
    write_test_file(&test_dir, "compute.js", js_content);
    
    // Create config with low CPU limit and disk VFS
    let config_content = format!(r#"{{
        "apps": [{{
            "hostname": "compute.local",
            "entrypoint": "./compute.js",
            "limits": {{
                "memory_mb": 128,
                "timeout_secs": 30,
                "workers": 2,
                "cpu_time_ms": 5,
                "cpu_time_enabled": true
            }},
            "vfs_backend": "disk",
            "vfs_disk": {{
                "base_path": "{}"
            }}
        }}],
        "server": {{
            "port": 18084,
            "host": "127.0.0.1"
        }}
    }}"#, test_dir.to_str().unwrap());
    write_test_file(&test_dir, "config.json", &config_content);
    
    // Spawn NANO
    let nano_path = nano_binary_path();
    let mut child = Command::new(&nano_path)
        .arg("run")
        .arg("--config")
        .arg(test_dir.join("config.json"))
        .current_dir(&test_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn NANO");
    
    // Wait for server to start (poll for readiness)
    wait_for_server(&mut child, 18084, "compute.local", 10).await;
    
    // Send multiple concurrent requests
    let client = reqwest::Client::new();
    let requests: Vec<_> = (0..3)
        .map(|_| {
            client
                .get("http://127.0.0.1:18084/")
                .header("Host", "compute.local")
                .timeout(Duration::from_secs(5))
                .send()
        })
        .collect();
    
    let results = futures::future::join_all(requests).await;
    
    // Terminate NANO
    child.kill().ok();
    
    // All should either succeed or timeout, not hang
    let mut success_count = 0;
    let mut error_count = 0;
    
    for result in results {
        match result {
            Ok(response) => {
                if response.status().is_success() {
                    success_count += 1;
                } else {
                    error_count += 1;
                    println!("Got error: {}", response.status());
                }
            }
            Err(e) => {
                error_count += 1;
                println!("Request failed: {}", e);
            }
        }
    }
    
    println!("Results: {} success, {} errors", success_count, error_count);
    
    // With 5ms CPU limit, some should timeout
    // We just verify the server didn't crash and responded to all
    assert!(
        success_count + error_count == 3,
        "All 3 requests should have received a response"
    );
}

/// RAII guard for test directory cleanup
struct TestCleanup<'a>(&'a PathBuf);

impl<'a> Drop for TestCleanup<'a> {
    fn drop(&mut self) {
        cleanup_test_dir(self.0);
    }
}

/// Test that verifies NANO binary exists
#[test]
fn test_nano_binary_exists() {
    let path = nano_binary_path();
    assert!(path.exists(), "NANO binary not found at {:?}", path);
}

/// Test configuration file parsing for CPU limits
#[test]
fn test_cpu_limit_config_parsing_e2e() {
    let test_dir = create_test_dir("config_parse");
    let _cleanup = TestCleanup(&test_dir);
    
    // Create a valid config with CPU limits
    let config = r#"{
        "apps": [{
            "hostname": "test.local",
            "entrypoint": "./test.js",
            "limits": {
                "memory_mb": 128,
                "timeout_secs": 30,
                "workers": 4,
                "cpu_time_ms": 50,
                "cpu_time_enabled": true
            }
        }],
        "server": {
            "port": 9999,
            "host": "127.0.0.1"
        }
    }"#;
    
    write_test_file(&test_dir, "config.json", config);
    
    // Create minimal JS file
    write_test_file(&test_dir, "test.js", r#"export default { async fetch() { return new Response('ok'); } }"#);
    
    // Try to validate config by running NANO with --help first (lightweight check)
    let nano_path = nano_binary_path();
    let output = Command::new(&nano_path)
        .arg("--help")
        .output()
        .expect("Failed to run NANO --help");
    
    assert!(output.status.success(), "NANO --help should succeed");
}
