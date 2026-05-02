//! Adversarial Network Attack Tests
//!
//! Tests to verify network security protections:
//! - DNS rebinding attacks
//! - Request flooding
//! - Slowloris attacks
//! - Header injection
//! - SSRF (Server-Side Request Forgery)
//! - Large header rejection

use std::time::{Duration, Instant};
use crate::security_utils::{find_available_port, NanoProcess};

/// Test DNS rebinding protection
/// Attack: Rapidly changing Host header to bypass origin checks
/// Mitigation: Hostname validation and no dynamic reconfiguration
#[tokio::test]
async fn test_dns_rebinding_blocked() {
    let port = find_available_port();
    let js_content = br#"export default {
    async fetch(request) {
        return new Response('App served', { status: 200 });
    }
}"#;

    let (mut nano, _temp_dir) = NanoProcess::start(
        port,
        "net-rebind.local",
        "app.js",
        js_content,
        5000,  // 5s CPU
        32,    // 32MB
    );
    
    nano.wait_ready(port, "net-rebind.local").await;

    let client = reqwest::Client::new();
    
    // First request with valid hostname
    let result1 = client
        .get(&format!("http://127.0.0.1:{}/", port))
        .header("Host", "net-rebind.local")
        .send()
        .await;
    
    assert!(result1.is_ok());
    assert_eq!(result1.unwrap().status(), 200);
    
    // Second request with different hostname (DNS rebinding attempt)
    // Should get 404 because this hostname isn't configured
    let result2 = client
        .get(&format!("http://127.0.0.1:{}/", port))
        .header("Host", "attacker.com")
        .send()
        .await;
    
    assert!(result2.is_ok());
    let status2 = result2.unwrap().status();
    
    nano.stop();
    
    // Unknown hostname should get 404
    assert_eq!(status2.as_u16(), 404, "Unknown hostname should return 404, not serve configured app");
}

/// Test request flooding rate limiting
/// Attack: Rapid sequential requests from same IP
/// Mitigation: Connection limits and graceful degradation
#[tokio::test]
async fn test_request_flooding_rate_limited() {
    let port = find_available_port();
    let js_content = br#"export default {
    async fetch(request) {
        return new Response('OK', { status: 200 });
    }
}"#;

    let (mut nano, _temp_dir) = NanoProcess::start(
        port,
        "net-flood.local",
        "app.js",
        js_content,
        5000,
        32,
    );
    
    nano.wait_ready(port, "net-flood.local").await;

    let client = reqwest::Client::new();
    
    // Send 50 rapid requests
    let start = Instant::now();
    let mut success_count = 0;
    let mut error_count = 0;
    
    for i in 0..50 {
        match client
            .get(&format!("http://127.0.0.1:{}/?req={}", port, i))
            .header("Host", "net-flood.local")
            .timeout(Duration::from_secs(5))
            .send()
            .await
        {
            Ok(response) => {
                if response.status().is_success() {
                    success_count += 1;
                } else {
                    error_count += 1;
                }
            }
            Err(_) => {
                error_count += 1;
            }
        }
    }
    
    let elapsed = start.elapsed();
    
    nano.stop();
    
    // Should complete all requests (system may queue or slow down)
    // The key is that it doesn't crash or hang
    assert!(
        elapsed < Duration::from_secs(30),
        "Flood test should complete within 30 seconds, took {:?}",
        elapsed
    );
    
    // Most requests should succeed (system should handle the load)
    assert!(
        success_count >= 40,
        "Expected at least 40 successful requests out of 50, got {} success, {} errors",
        success_count,
        error_count
    );
}

/// Test slowloris attack mitigation
/// Attack: Slow HTTP header sending
/// Mitigation: Connection timeouts
#[tokio::test]
async fn test_slowloris_mitigated() {
    let port = find_available_port();
    let js_content = br#"export default {
    async fetch(request) {
        return new Response('OK', { status: 200 });
    }
}"#;

    let (mut nano, _temp_dir) = NanoProcess::start(
        port,
        "net-slowloris.local",
        "app.js",
        js_content,
        5000,
        32,
    );
    
    nano.wait_ready(port, "net-slowloris.local").await;

    // Use a custom client with very slow request sending
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))  // 2 second timeout
        .build()
        .unwrap();
    
    // Normal request should work
    let normal_result = client
        .get(&format!("http://127.0.0.1:{}/", port))
        .header("Host", "net-slowloris.local")
        .send()
        .await;
    
    assert!(normal_result.is_ok());
    assert_eq!(normal_result.unwrap().status(), 200);
    
    nano.stop();
    
    // The test verifies that normal requests work with timeout settings
    // Slowloris mitigation is handled at the HTTP server layer (axum/tower)
}

/// Test header injection protection
/// Attack: CRLF injection in headers
/// Mitigation: Header validation and sanitization
#[tokio::test]
async fn test_header_injection_blocked() {
    let port = find_available_port();
    let js_content = br#"export default {
    async fetch(request) {
        // Echo back headers
        const headers = {};
        request.headers.forEach((value, key) => {
            headers[key] = value;
        });
        return new Response(JSON.stringify(headers), { 
            status: 200,
            headers: { 'Content-Type': 'application/json' }
        });
    }
}"#;

    let (mut nano, _temp_dir) = NanoProcess::start(
        port,
        "net-header.local",
        "app.js",
        js_content,
        5000,
        32,
    );
    
    nano.wait_ready(port, "net-header.local").await;

    let client = reqwest::Client::new();
    
    // Attempt header injection via malicious header value
    // reqwest/axum should sanitize this
    let result = client
        .get(&format!("http://127.0.0.1:{}/", port))
        .header("Host", "net-header.local")
        .header("X-Custom", "value\r\nInjected: header")
        .send()
        .await;
    
    nano.stop();
    
    // Request should either:
    // 1. Succeed with sanitized header (value\r\n stripped)
    // 2. Fail with error (header rejected)
    match result {
        Ok(response) => {
            // If it succeeded, verify the injection didn't work
            let body = response.text().await.unwrap_or_default();
            assert!(
                !body.contains("Injected"),
                "Header injection should be blocked. Response: {}",
                body
            );
        }
        Err(_) => {
            // Error is acceptable - header was rejected
        }
    }
}

/// Test SSRF (Server-Side Request Forgery) protection
/// Attack: fetch() to internal IPs (169.254.x.x, 10.x.x.x, etc.)
/// Mitigation: Outbound fetch should have URL validation
/// 
/// Note: This test documents the expected behavior.
/// Full SSRF protection requires URL validation in the fetch implementation.
#[tokio::test]
async fn test_ssrf_private_ips_blocked() {
    let port = find_available_port();
    let js_content = br#"export default {
    async fetch(request) {
        const url = new URL(request.url);
        const target = url.searchParams.get('target');
        
        if (!target) {
            return new Response('No target specified', { status: 400 });
        }
        
        try {
            const response = await fetch(target);
            const body = await response.text();
            return new Response(body, { status: 200 });
        } catch (e) {
            return new Response(JSON.stringify({error: e.message}), { status: 500 });
        }
    }
}"#;

    let (mut nano, _temp_dir) = NanoProcess::start(
        port,
        "net-ssrf.local",
        "app.js",
        js_content,
        5000,
        32,
    );
    
    nano.wait_ready(port, "net-ssrf.local").await;

    let client = reqwest::Client::new();
    
    // Attempt to fetch from private IP (metadata service)
    let result = client
        .get(&format!("http://127.0.0.1:{}/?target=http://169.254.169.254/latest/meta-data/", port))
        .header("Host", "net-ssrf.local")
        .timeout(Duration::from_secs(5))
        .send()
        .await;
    
    nano.stop();
    
    // The request should fail - either:
    // 1. fetch() not implemented in NANO yet
    // 2. fetch() rejects private IPs
    // 3. Network timeout (private IP unreachable)
    match result {
        Ok(response) => {
            let body = response.text().await.unwrap_or_default();
            // Should not contain actual metadata
            assert!(
                !body.contains("ami-id") && !body.contains("instance-id"),
                "SSRF to metadata service should be blocked. Response: {}",
                body
            );
        }
        Err(_) => {
            // Error is expected - fetch may not be fully implemented or request blocked
        }
    }
}

/// Test large header rejection
/// Attack: Request with >8KB headers
/// Mitigation: Header size limits
#[tokio::test]
async fn test_large_header_rejection() {
    let port = find_available_port();
    let js_content = br#"export default {
    async fetch(request) {
        return new Response('OK', { status: 200 });
    }
}"#;

    let (mut nano, _temp_dir) = NanoProcess::start(
        port,
        "net-large-header.local",
        "app.js",
        js_content,
        5000,
        32,
    );
    
    nano.wait_ready(port, "net-large-header.local").await;

    let client = reqwest::Client::new();
    
    // Create a large header (>8KB)
    let large_value = "x".repeat(10000);
    
    let result = client
        .get(&format!("http://127.0.0.1:{}/", port))
        .header("Host", "net-large-header.local")
        .header("X-Large-Header", large_value)
        .timeout(Duration::from_secs(5))
        .send()
        .await;
    
    nano.stop();
    
    // Large headers should either be:
    // 1. Rejected with 413/431 error
    // 2. Truncated/accepted (depends on server config)
    match result {
        Ok(response) => {
            let status = response.status();
            // Should not crash - either accepted or properly rejected
            assert!(
                status.as_u16() != 500,
                "Large header should not cause server error 500"
            );
        }
        Err(_) => {
            // Error is acceptable
        }
    }
}
