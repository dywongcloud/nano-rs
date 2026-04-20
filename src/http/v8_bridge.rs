//! V8 Bridge for WinterCG types
//!
//! This module provides serialization for Request/Response objects that
//! will be used with V8 JavaScript contexts in Phase 3.
//!
//! Per D-06: JSON serialization → V8 parse (simpler than direct V8 API creation).
//!
//! Note: Full V8 object creation happens in Phase 3. This module provides
//! the serialization layer and JSON utilities.

use crate::http::{NanoRequest, NanoResponse};

/// Serialize a NanoRequest to JSON string
///
/// Creates a JSON representation matching the WinterCG Request interface.
/// This JSON can be parsed in V8 using JSON.parse() per D-06.
///
/// # Arguments
///
/// * `request` - The NanoRequest to serialize
///
/// # Returns
///
/// A JSON string representation of the request
///
/// # Example
///
/// ```
/// use nano::http::{NanoRequest, NanoUrl, NanoHeaders};
/// use nano::http::v8_bridge::serialize_request_to_json;
///
/// let url = NanoUrl::parse("https://example.com/api").unwrap();
/// let request = NanoRequest::new(
///     "GET".to_string(),
///     url,
///     NanoHeaders::new(),
///     None,
/// );
/// let json = serialize_request_to_json(&request);
/// assert!(json.contains("\"method\":\"GET\""));
/// ```
pub fn serialize_request_to_json(request: &NanoRequest) -> String {
    // Build JSON manually to ensure correct WinterCG structure
    let mut json = String::from("{");

    // method
    json.push_str(&format!(
        "\"method\":\"{}\",",
        escape_json(request.method())
    ));

    // url
    json.push_str(&format!(
        "\"url\":\"{}\",",
        escape_json(&request.url_string())
    ));

    // headers
    json.push_str("\"headers\":{");
    let mut first = true;
    request.headers().for_each(|name, value| {
        if !first {
            json.push(',');
        }
        first = false;
        json.push_str(&format!(
            "\"{}\":\"{}\"",
            escape_json(name),
            escape_json(value)
        ));
    });
    json.push_str("},");

    // body (base64 encoded if present)
    if let Some(body) = request.body() {
        let base64 = base64_encode(body);
        json.push_str(&format!("\"body\":\"{}\",\"bodyUsed\":true", base64));
    } else {
        json.push_str("\"body\":null,\"bodyUsed\":false");
    }

    json.push('}');
    json
}

/// Serialize a NanoResponse to JSON string
///
/// Creates a JSON representation matching the WinterCG Response interface.
///
/// # Arguments
///
/// * `response` - The NanoResponse to serialize
///
/// # Returns
///
/// A JSON string representation of the response
pub fn serialize_response_to_json(response: &NanoResponse) -> String {
    let mut json = String::from("{");

    // status
    json.push_str(&format!("\"status\":{},", response.status()));

    // statusText
    json.push_str(&format!(
        "\"statusText\":\"{}\",",
        escape_json(response.status_text())
    ));

    // headers
    json.push_str("\"headers\":{");
    let mut first = true;
    response.headers().for_each(|name, value| {
        if !first {
            json.push(',');
        }
        first = false;
        json.push_str(&format!(
            "\"{}\":\"{}\"",
            escape_json(name),
            escape_json(value)
        ));
    });
    json.push_str("},");

    // body (base64 encoded if present)
    if let Some(body) = response.body() {
        let base64 = base64_encode(body);
        json.push_str(&format!("\"body\":\"{}\"", base64));
    } else {
        json.push_str("\"body\":null");
    }

    json.push('}');
    json
}

/// Escape string for JSON safety
///
/// Escapes backslashes, quotes, and control characters for JSON.
///
/// # Arguments
///
/// * `s` - The string to escape
///
/// # Returns
///
/// The escaped string safe for JSON inclusion
fn escape_json(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

/// Base64 encoding helper
///
/// Simple base64 encoding without external crate dependency.
fn base64_encode(input: &[u8]) -> String {
    const BASE64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut result = String::with_capacity((input.len() + 2) / 3 * 4);

    let chunks = input.chunks_exact(3);
    let remainder = chunks.remainder();

    for chunk in chunks {
        let b = ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | (chunk[2] as u32);
        result.push(BASE64_CHARS[((b >> 18) & 0x3F) as usize] as char);
        result.push(BASE64_CHARS[((b >> 12) & 0x3F) as usize] as char);
        result.push(BASE64_CHARS[((b >> 6) & 0x3F) as usize] as char);
        result.push(BASE64_CHARS[(b & 0x3F) as usize] as char);
    }

    match remainder.len() {
        1 => {
            let b = (remainder[0] as u32) << 16;
            result.push(BASE64_CHARS[((b >> 18) & 0x3F) as usize] as char);
            result.push(BASE64_CHARS[((b >> 12) & 0x3F) as usize] as char);
            result.push('=');
            result.push('=');
        }
        2 => {
            let b = ((remainder[0] as u32) << 16) | ((remainder[1] as u32) << 8);
            result.push(BASE64_CHARS[((b >> 18) & 0x3F) as usize] as char);
            result.push(BASE64_CHARS[((b >> 12) & 0x3F) as usize] as char);
            result.push(BASE64_CHARS[((b >> 6) & 0x3F) as usize] as char);
            result.push('=');
        }
        _ => {}
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{NanoHeaders, NanoUrl};

    #[test]
    fn test_request_serialization() {
        let url = NanoUrl::parse("https://example.com/api").unwrap();
        let mut headers = NanoHeaders::new();
        headers.set("Content-Type", "application/json");
        let request = NanoRequest::new(
            "POST".to_string(),
            url,
            headers,
            Some(bytes::Bytes::from("test body")),
        );

        let json = serialize_request_to_json(&request);
        assert!(json.contains("\"method\":\"POST\""));
        assert!(json.contains("\"url\":\"https://example.com/api\""));
        // Headers are stored lowercase per D-07
        assert!(json.contains("\"content-type\""));
        assert!(json.contains("\"bodyUsed\":true"));
    }

    #[test]
    fn test_response_serialization() {
        let response =
            NanoResponse::new(200, NanoHeaders::new(), Some(bytes::Bytes::from("Hello")));

        let json = serialize_response_to_json(&response);
        assert!(json.contains("\"status\":200"));
        assert!(json.contains("\"statusText\":\"OK\""));
    }

    #[test]
    fn test_escape_json() {
        // Test escaping quotes
        let result = escape_json("hello\"world");
        assert!(result.contains("hello"));
        assert!(result.contains("\\\""));
        assert!(result.contains("world"));

        // Test escaping newlines
        let result = escape_json("line1\nline2");
        assert!(result.contains("line1"));
        assert!(result.contains("\\n"));
        assert!(result.contains("line2"));

        // Test escaping tabs
        let result = escape_json("tab\there");
        assert!(result.contains("tab"));
        assert!(result.contains("\\t"));
        assert!(result.contains("here"));

        // Test escaping backslashes
        let result = escape_json("path\\to\\file");
        assert!(result.contains("path"));
        assert!(result.contains("\\\\")); // Two backslashes in output
        assert!(result.contains("to"));
        assert!(result.contains("file"));
    }

    #[test]
    fn test_base64_encode() {
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
        assert_eq!(base64_encode(b"test"), "dGVzdA==");
        assert_eq!(base64_encode(b"abc"), "YWJj");
        assert_eq!(base64_encode(b""), "");
    }

    #[test]
    fn test_request_serialization_no_body() {
        let url = NanoUrl::parse("https://example.com/api").unwrap();
        let headers = NanoHeaders::new();
        let request = NanoRequest::new("GET".to_string(), url, headers, None);

        let json = serialize_request_to_json(&request);
        assert!(json.contains("\"body\":null"));
        assert!(json.contains("\"bodyUsed\":false"));
    }
}
