//! End-to-end tests for CPU time limits
//!
//! These tests spawn the NANO binary and verify actual timeout behavior.
//! Run with: cargo test --test cpu_timeout_e2e_test -- --test-threads=1

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU16, Ordering};

// Re-export scopeguard from dev-dependencies if available
// For now, we'll use a simple manual cleanup approach

// Global port counter to avoid conflicts
// Start from a random-ish base to avoid TIME_WAIT conflicts from previous runs
static PORT_COUNTER: AtomicU16 = AtomicU16::new(29000);

fn get_unique_port() -> u16 {
    PORT_COUNTER.fetch_add(1, Ordering::SeqCst)
}

fn nano_binary_path() -> PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let project_root = PathBuf::from(manifest_dir);
    
    let release_path = project_root.join("target/release/nano-rs");
    if release_path.exists() {
        return release_path;
    }
    
    let debug_path = project_root.join("target/debug/nano-rs");
    if debug_path.exists() {
        return debug_path;
    }
    
    panic!("NANO binary not found. Build with: cargo build");
}

fn create_test_dir(name: &str) -> PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir()
        .join(format!("nano_e2e_{}_{}_{}", name, std::process::id(), timestamp));
    fs::remove_dir_all(&temp_dir).ok();
    fs::create_dir_all(&temp_dir).expect("Failed to create test dir");
    temp_dir
}

fn cleanup_test_dir(path: &PathBuf) {
    fs::remove_dir_all(path).ok();
}

fn write_test_file(dir: &PathBuf, filename: &str, content: &str) {
    fs::write(dir.join(filename), content).expect(&format!("Failed to write {}", filename));
}

async fn wait_for_server(port: u16, hostname: &str, max_wait_secs: u64) -> Result<(), String> {
    // Wait longer for V8 initialization between tests
    tokio::time::sleep(Duration::from_secs(1)).await;
    
    let client = reqwest::Client::new();
    let start = Instant::now();
    let max_wait = Duration::from_secs(max_wait_secs);

    while start.elapsed() < max_wait {
        match client
            .get(format!("http://127.0.0.1:{}/", port))
            .header("Host", hostname)
            .timeout(Duration::from_secs(2))
            .send()
            .await
        {
            Ok(_) => return Ok(()),
            Err(_) => {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }

    Err(format!("Server failed to start on port {} within {} seconds", port, max_wait_secs))
}

struct NanoProcess {
    child: std::process::Child,
    port: u16,
    hostname: String,
}

impl NanoProcess {
    fn start(test_dir: &PathBuf, config: &str, port: u16, hostname: &str) -> Self {
        write_test_file(test_dir, "config.json", config);

        let nano_path = nano_binary_path();
        let child = Command::new(&nano_path)
            .arg("run")
            .arg("--config")
            .arg(test_dir.join("config.json"))
            .current_dir(test_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn NANO");

        Self {
            child,
            port,
            hostname: hostname.to_string(),
        }
    }

    async fn wait_ready(&mut self) {
        if let Err(e) = wait_for_server(self.port, &self.hostname, 15).await {
            panic!("{}", e);
        }
    }

    fn stop(&mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
    }
}

impl Drop for NanoProcess {
    fn drop(&mut self) {
        self.stop();
    }
}

#[tokio::test]
#[ignore = "E2E test - run manually with: cargo test --test cpu_timeout_e2e_test -- --ignored --test-threads=1"]
async fn test_js_cpu_timeout() {
    let test_dir = create_test_dir("js_timeout");
    let port = get_unique_port();

    let js_content = r#"export default {
    async fetch(request) {
        while (true) { Math.random(); }
    }
}"#;
    write_test_file(&test_dir, "infinite.js", js_content);

    let base_path_escaped = serde_json::to_string(&test_dir.to_str().unwrap()).unwrap();
    let base_path_escaped = base_path_escaped.trim_matches('"');
    let config = format!(r#"{{
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
            "vfs_disk": {{"base_path": "{}"}}
        }}],
        "server": {{"port": {}, "host": "127.0.0.1"}}
    }}"#, base_path_escaped, port);
    
    let mut nano = NanoProcess::start(&test_dir, &config, port, "timeout.local");
    nano.wait_ready().await;

    let start = Instant::now();
    let client = reqwest::Client::new();
    let result = client
        .get(&format!("http://127.0.0.1:{}/", port))
        .header("Host", "timeout.local")
        .timeout(Duration::from_secs(5))
        .send()
        .await;
    let elapsed = start.elapsed();

    nano.stop();

    // Should timeout quickly (within ~200ms real time for 10ms CPU limit)
    assert!(elapsed < Duration::from_millis(500), 
        "CPU timeout took too long: {:?}. Expected <500ms", elapsed);

    match result {
        Ok(response) => {
            assert!(response.status().is_server_error() || response.status().as_u16() == 504,
                "Expected error for CPU timeout, got {}", response.status());
        }
        Err(_) => {} // Timeout is expected
    }

    println!("JS CPU timeout test passed: elapsed={:?}", elapsed);
    cleanup_test_dir(&test_dir);
}

#[tokio::test]
#[ignore = "E2E test - run manually with: cargo test --test cpu_timeout_e2e_test -- --ignored --test-threads=1"]
async fn test_js_within_cpu_limit() {
    let test_dir = create_test_dir("js_normal");
    let port = get_unique_port();

    let js_content = r#"export default {
    async fetch(request) {
        let sum = 0;
        for (let i = 0; i < 1000; i++) { sum += i; }
        return new Response(JSON.stringify({sum}), {
            status: 200,
            headers: {'Content-Type': 'application/json'}
        });
    }
}"#;
    write_test_file(&test_dir, "normal.js", js_content);

    let base_path_escaped = serde_json::to_string(&test_dir.to_str().unwrap()).unwrap();
    let base_path_escaped = base_path_escaped.trim_matches('"');
    let config = format!(r#"{{
        "apps": [{{
            "hostname": "normal.local",
            "entrypoint": "./normal.js",
            "limits": {{
                "memory_mb": 128,
                "timeout_secs": 30,
                "workers": 1,
                "cpu_time_ms": 100,
                "cpu_time_enabled": true
            }},
            "vfs_backend": "disk",
            "vfs_disk": {{"base_path": "{}"}}
        }}],
        "server": {{"port": {}, "host": "127.0.0.1"}}
    }}"#, base_path_escaped, port);

    let mut nano = NanoProcess::start(&test_dir, &config, port, "normal.local");
    nano.wait_ready().await;

    let client = reqwest::Client::new();
    let result = client
        .get(&format!("http://127.0.0.1:{}/", port))
        .header("Host", "normal.local")
        .timeout(Duration::from_secs(5))
        .send()
        .await;

    nano.stop();

    match result {
        Ok(response) => {
            assert!(response.status().is_success(), 
                "Expected success, got {}", response.status());
            let body = response.text().await.unwrap_or_default();
            assert!(body.contains("499500"), "Expected sum 499500, got: {}", body);
        }
        Err(e) => panic!("Request failed: {}", e),
    }

    println!("JS normal execution test passed");
    cleanup_test_dir(&test_dir);
}

#[tokio::test]
#[ignore = "E2E test - run manually with: cargo test --test cpu_timeout_e2e_test -- --ignored --test-threads=1"]
async fn test_wasm_cpu_timeout() {
    let test_dir = create_test_dir("wasm_timeout");
    let port = get_unique_port();

    let wasm_bytes = include_bytes!("../examples/wasm-test/add.wasm");
    fs::write(test_dir.join("add.wasm"), wasm_bytes).expect("Failed to write WASM");

    let js_content = r#"export default {
    async fetch(request) {
        const wasmBytes = await Nano.fs.readFile('add.wasm');
        const module = await WebAssembly.compile(wasmBytes);
        const instance = await WebAssembly.instantiate(module, {});
        while (true) { instance.exports.add(1, 1); }
    }
}"#;
    write_test_file(&test_dir, "wasm_infinite.js", js_content);

    let base_path_escaped = serde_json::to_string(&test_dir.to_str().unwrap()).unwrap();
    let base_path_escaped = base_path_escaped.trim_matches('"');
    let config = format!(r#"{{
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
            "vfs_disk": {{"base_path": "{}"}}
        }}],
        "server": {{"port": {}, "host": "127.0.0.1"}}
    }}"#, base_path_escaped, port);

    let mut nano = NanoProcess::start(&test_dir, &config, port, "wasm-timeout.local");
    nano.wait_ready().await;

    let start = Instant::now();
    let client = reqwest::Client::new();
    let result = client
        .get(&format!("http://127.0.0.1:{}/", port))
        .header("Host", "wasm-timeout.local")
        .timeout(Duration::from_secs(5))
        .send()
        .await;
    let elapsed = start.elapsed();

    nano.stop();

    assert!(elapsed < Duration::from_millis(500),
        "WASM CPU timeout took too long: {:?}. Expected <500ms", elapsed);

    match result {
        Ok(response) => {
            assert!(response.status().is_server_error() || response.status().as_u16() == 504,
                "Expected error for WASM CPU timeout, got {}", response.status());
        }
        Err(_) => {}
    }

    println!("WASM CPU timeout test passed: elapsed={:?}", elapsed);
    cleanup_test_dir(&test_dir);
}

#[tokio::test]
#[ignore = "E2E test - run manually with: cargo test --test cpu_timeout_e2e_test -- --ignored --test-threads=1"]
async fn test_wasm_within_cpu_limit() {
    let test_dir = create_test_dir("wasm_normal");
    let port = get_unique_port();

    let wasm_bytes = include_bytes!("../examples/wasm-test/add.wasm");
    fs::write(test_dir.join("add.wasm"), wasm_bytes).expect("Failed to write WASM");

    let js_content = r#"export default {
    async fetch(request) {
        const url = new URL(request.url);
        const a = parseInt(url.searchParams.get('a') || '5');
        const b = parseInt(url.searchParams.get('b') || '3');
        const wasmBytes = await Nano.fs.readFile('add.wasm');
        const module = await WebAssembly.compile(wasmBytes);
        const instance = await WebAssembly.instantiate(module, {});
        const result = instance.exports.add(a, b);
        return new Response(JSON.stringify({a, b, result}), {
            status: 200,
            headers: {'Content-Type': 'application/json'}
        });
    }
}"#;
    write_test_file(&test_dir, "wasm_normal.js", js_content);

    let base_path_escaped = serde_json::to_string(&test_dir.to_str().unwrap()).unwrap();
    let base_path_escaped = base_path_escaped.trim_matches('"');
    let config = format!(r#"{{
        "apps": [{{
            "hostname": "wasm-normal.local",
            "entrypoint": "./wasm_normal.js",
            "limits": {{
                "memory_mb": 128,
                "timeout_secs": 30,
                "workers": 1,
                "cpu_time_ms": 100,
                "cpu_time_enabled": true
            }},
            "vfs_backend": "disk",
            "vfs_disk": {{"base_path": "{}"}}
        }}],
        "server": {{"port": {}, "host": "127.0.0.1"}}
    }}"#, base_path_escaped, port);

    let mut nano = NanoProcess::start(&test_dir, &config, port, "wasm-normal.local");
    nano.wait_ready().await;

    let client = reqwest::Client::new();
    let result = client
        .get(&format!("http://127.0.0.1:{}/?a=10&b=20", port))
        .header("Host", "wasm-normal.local")
        .timeout(Duration::from_secs(5))
        .send()
        .await;

    nano.stop();

    match result {
        Ok(response) => {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
                    assert!(status.is_success(),
                "Expected success, got {} with body: {}", status, body);
            assert!(body.contains("30"), "Expected result 30, got: {}", body);
        }
        Err(e) => panic!("Request failed: {}", e),
    }

    println!("WASM normal execution test passed");
    // DEBUG: Don't clean up to inspect files
    println!("Test directory: {:?}", test_dir);
    std::thread::sleep(std::time::Duration::from_secs(5));
    cleanup_test_dir(&test_dir);
}

#[tokio::test]
#[ignore = "E2E test - run manually with: cargo test --test cpu_timeout_e2e_test -- --ignored --test-threads=1"]
async fn test_cpu_limit_per_isolate() {
    let test_dir = create_test_dir("per_isolate");
    let port = get_unique_port();

    let js_content = r#"export default {
    async fetch(request) {
        let iterations = 0;
        const endTime = Date.now() + 50;
        while (Date.now() < endTime) { iterations++; }
        return new Response(JSON.stringify({iterations}), {
            status: 200,
            headers: {'Content-Type': 'application/json'}
        });
    }
}"#;
    write_test_file(&test_dir, "compute.js", js_content);

    let base_path_escaped = serde_json::to_string(&test_dir.to_str().unwrap()).unwrap();
    let base_path_escaped = base_path_escaped.trim_matches('"');
    let config = format!(r#"{{
        "apps": [{{
            "hostname": "compute.local",
            "entrypoint": "./compute.js",
            "limits": {{
                "memory_mb": 128,
                "timeout_secs": 30,
                "workers": 2,
                "cpu_time_ms": 100,
                "cpu_time_enabled": true
            }},
            "vfs_backend": "disk",
            "vfs_disk": {{"base_path": "{}"}}
        }}],
        "server": {{"port": {}, "host": "127.0.0.1"}}
    }}"#, base_path_escaped, port);

    let mut nano = NanoProcess::start(&test_dir, &config, port, "compute.local");
    nano.wait_ready().await;

    let client = reqwest::Client::new();
    
    // Send concurrent requests
    let futures = (0..3).map(|_| {
        client
            .get(&format!("http://127.0.0.1:{}/", port))
            .header("Host", "compute.local")
            .timeout(Duration::from_secs(5))
            .send()
    });

    let results = futures::future::join_all(futures).await;
    nano.stop();

    let mut success_count = 0;
    for result in results {
        match result {
            Ok(response) if response.status().is_success() => {
                success_count += 1;
            }
            Ok(response) => {
                println!("Got error status: {}", response.status());
            }
            Err(e) => {
                println!("Request error: {}", e);
            }
        }
    }

    assert!(success_count >= 2, "Expected at least 2 successful requests, got {}", success_count);
    println!("Per-isolate CPU limit test passed: {}/3 succeeded", success_count);
    cleanup_test_dir(&test_dir);
}
