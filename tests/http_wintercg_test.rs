//! WinterTC types integration tests
//!
//! Tests verify full WinterTC compliance for Request, Response, Headers,
//! URL, and URLSearchParams types.

use bytes::Bytes;
use nano::http::{NanoHeaders, NanoRequest, NanoResponse, NanoUrl, NanoUrlSearchParams};

#[test]
fn test_full_request_response_cycle() {
    // Create request
    let url = NanoUrl::parse("https://api.example.com/users?id=123").unwrap();
    let mut headers = NanoHeaders::new();
    headers.set("Accept", "application/json");
    headers.set("Authorization", "Bearer token123");

    let request = NanoRequest::new("GET".to_string(), url, headers, None);

    // Verify request properties
    assert_eq!(request.method(), "GET");
    assert_eq!(request.url().pathname(), "/users");
    assert_eq!(
        request.url().search_params().get("id"),
        Some("123".to_string())
    );
    assert_eq!(
        request.headers().get("Accept"),
        Some("application/json".to_string())
    );
    assert_eq!(
        request.headers().get("Authorization"),
        Some("Bearer token123".to_string())
    );

    // Create response
    let mut response_headers = NanoHeaders::new();
    response_headers.set("Content-Type", "application/json");

    let response = NanoResponse::new(200, response_headers, Some(Bytes::from(r#"{"users":[]}"#)));

    // Verify response
    assert_eq!(response.status(), 200);
    assert_eq!(response.status_text(), "OK");
    assert_eq!(
        response.headers().get("Content-Type"),
        Some("application/json".to_string())
    );
}

#[test]
fn test_url_full_compliance() {
    let url = NanoUrl::parse(
        "https://user:pass@example.com:8080/path/to/resource?foo=bar&baz=qux#section",
    )
    .unwrap();

    // All WinterTC URL properties
    assert_eq!(url.protocol(), "https:");
    assert_eq!(url.host(), "example.com");
    assert_eq!(url.hostname(), "example.com");
    assert_eq!(url.port(), Some(8080));
    assert_eq!(url.pathname(), "/path/to/resource");
    assert_eq!(url.search(), "?foo=bar&baz=qux");
    assert_eq!(url.hash(), "#section");
    assert!(url.href().contains("https://"));
    assert!(url.origin().contains("example.com"));
}

#[test]
fn test_headers_api_compliance() {
    let mut headers = NanoHeaders::new();

    // Test all WinterTC Headers methods
    headers.append("Accept", "application/json");
    headers.append("Accept", "text/html");
    headers.set("Content-Type", "application/json");
    headers.append("Set-Cookie", "session=abc");
    headers.append("Set-Cookie", "user=xyz");

    // get() - returns comma-combined (except Set-Cookie)
    assert_eq!(
        headers.get("Accept"),
        Some("application/json, text/html".to_string())
    );
    assert_eq!(
        headers.get("Content-Type"),
        Some("application/json".to_string())
    );

    // get_set_cookie() - returns array
    let cookies = headers.get_set_cookie();
    assert_eq!(cookies.len(), 2);
    assert!(cookies.contains(&"session=abc".to_string()));

    // has()
    assert!(headers.has("Content-Type"));
    assert!(!headers.has("X-Unknown"));

    // Case insensitive (per D-07)
    assert_eq!(
        headers.get("content-type"),
        Some("application/json".to_string())
    );
    assert_eq!(
        headers.get("CONTENT-TYPE"),
        Some("application/json".to_string())
    );

    // delete()
    headers.delete("Accept");
    assert!(!headers.has("Accept"));
}

#[test]
fn test_url_search_params_compliance() {
    let params = NanoUrlSearchParams::from_query(Some("foo=bar&foo=baz&qux=quux&special=%26%3D"));

    // get() - returns first value
    assert_eq!(params.get("foo"), Some("bar".to_string()));

    // get_all() - returns all values
    let all_foo = params.get_all("foo");
    assert_eq!(all_foo.len(), 2);
    assert!(all_foo.contains(&"bar".to_string()));
    assert!(all_foo.contains(&"baz".to_string()));

    // has()
    assert!(params.has("qux"));
    assert!(!params.has("unknown"));

    // Percent decoding per D-10
    assert_eq!(params.get("special"), Some("&=".to_string()));

    // to_string()
    let query_string = params.to_string();
    assert!(query_string.contains("foo=bar"));
    assert!(query_string.contains("qux=quux"));
}

#[test]
fn test_lossy_percent_decoding() {
    // D-10: Invalid UTF-8 sequences become U+FFFD
    let params = NanoUrlSearchParams::from_query(Some("invalid=%FF%FE%FD"));
    let value = params.get("invalid").unwrap();
    assert!(value.contains('\u{FFFD}')); // Replacement character
}

#[test]
fn test_request_response_builder_pattern() {
    // Test Request creation with builder-style pattern
    let url = NanoUrl::parse("https://api.example.com/items").unwrap();
    let mut headers = NanoHeaders::new();
    headers.set("Content-Type", "application/json");

    let request = NanoRequest::new(
        "POST".to_string(),
        url,
        headers,
        Some(Bytes::from(r#"{"name":"test"}"#)),
    );

    assert_eq!(request.method(), "POST");
    assert!(request.body().is_some());

    // Test Response builder
    let response = NanoResponse::ok()
        .with_header("X-Custom-Header", "value")
        .with_header("Content-Type", "text/plain")
        .with_body("Success");

    assert_eq!(response.status(), 200);
    assert_eq!(
        response.headers().get("X-Custom-Header"),
        Some("value".to_string())
    );
    assert_eq!(
        response.headers().get("Content-Type"),
        Some("text/plain".to_string())
    );

    // Test response with specific status
    let not_found = NanoResponse::with_status(404)
        .with_header("Content-Type", "application/json")
        .with_body(r#"{"error":"Not Found"}"#);

    assert_eq!(not_found.status(), 404);
    assert_eq!(not_found.status_text(), "Not Found");
}

#[test]
fn test_headers_set_cookie_handling() {
    // D-08: Set-Cookie headers remain separate
    let mut headers = NanoHeaders::new();

    // Add multiple cookies
    headers.append("Set-Cookie", "session=abc123; HttpOnly; Secure");
    headers.append("Set-Cookie", "user=xyz; Path=/; SameSite=Strict");
    headers.append("Set-Cookie", "prefs=dark_mode; Max-Age=3600");

    // get_set_cookie returns all values
    let cookies = headers.get_set_cookie();
    assert_eq!(cookies.len(), 3);
    assert!(cookies.iter().any(|c| c.contains("session=")));
    assert!(cookies.iter().any(|c| c.contains("user=")));
    assert!(cookies.iter().any(|c| c.contains("prefs=")));

    // get() returns only the first for Set-Cookie
    let first_cookie = headers.get("Set-Cookie");
    assert!(first_cookie.is_some());
    assert!(first_cookie.unwrap().contains("session="));

    // Other headers combine with commas
    headers.append("X-Custom", "value1");
    headers.append("X-Custom", "value2");
    let combined = headers.get("X-Custom");
    assert_eq!(combined, Some("value1, value2".to_string()));
}

#[test]
fn test_url_various_protocols() {
    // Test different URL schemes
    let http_url = NanoUrl::parse("http://example.com/path").unwrap();
    assert_eq!(http_url.protocol(), "http:");

    let https_url = NanoUrl::parse("https://secure.example.com/path").unwrap();
    assert_eq!(https_url.protocol(), "https:");

    // Test URL with non-default port
    let custom_port = NanoUrl::parse("https://example.com:8443/path").unwrap();
    assert_eq!(custom_port.port(), Some(8443));

    // Test URL without explicit port (uses scheme default)
    // Note: The url crate normalizes default ports, so port() returns None
    let no_port = NanoUrl::parse("https://example.com/path").unwrap();
    // port() returns None when using the scheme's default port
    assert_eq!(no_port.port(), None);
}

#[test]
fn test_axum_conversion_roundtrip() {
    // Test that we can convert from axum headers to NanoHeaders and back
    let mut axum_headers = axum::http::HeaderMap::new();
    axum_headers.insert("X-Request-Id", "12345".parse().unwrap());
    axum_headers.insert("Accept", "application/json".parse().unwrap());

    // Convert to NanoHeaders
    let nano_headers = NanoHeaders::from_axum_headers(&axum_headers);
    assert!(nano_headers.has("x-request-id"));
    assert_eq!(
        nano_headers.get("Accept"),
        Some("application/json".to_string())
    );

    // Convert back to axum
    let back_to_axum = nano_headers.to_axum_headers();
    assert_eq!(
        back_to_axum
            .get("x-request-id")
            .and_then(|v| v.to_str().ok()),
        Some("12345")
    );
}
