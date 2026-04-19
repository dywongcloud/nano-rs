//! Request and Response types for WinterCG compatibility
//!
//! These types bridge Rust HTTP handling with JavaScript execution,
//! providing WinterCG-compliant Request and Response objects.
//!
//! # Decisions
//!
//! - **D-05:** Hybrid body handling — buffer small bodies (<1MB) in memory, stream large bodies
//! - **D-06:** Response objects via JSON serialization → V8 parse (not direct V8 API creation)

use bytes::Bytes;
use crate::http::{NanoHeaders, NanoUrl};

/// WinterCG-compatible Request object
///
/// Represents an HTTP request that will be passed to the JavaScript handler.
/// Contains all WinterCG Request properties: method, url, headers, body.
#[derive(Debug, Clone)]
pub struct NanoRequest {
    method: String,
    url: NanoUrl,
    headers: NanoHeaders,
    body: Option<Bytes>,  // D-05: In-memory body for now (streaming in Phase 6)
}

impl NanoRequest {
    /// Create a new Request
    ///
    /// # Arguments
    ///
    /// * `method` - HTTP method (GET, POST, etc.)
    /// * `url` - The request URL
    /// * `headers` - HTTP headers
    /// * `body` - Optional request body
    ///
    /// # Returns
    ///
    /// A new `NanoRequest`
    pub fn new(method: String, url: NanoUrl, headers: NanoHeaders, body: Option<Bytes>) -> Self {
        Self { method, url, headers, body }
    }
    
    /// HTTP method
    ///
    /// Per WinterCG: https://developer.mozilla.org/en-US/docs/Web/API/Request/method
    pub fn method(&self) -> &str {
        &self.method
    }
    
    /// Request URL
    ///
    /// Per WinterCG: https://developer.mozilla.org/en-US/docs/Web/API/Request/url
    pub fn url(&self) -> &NanoUrl {
        &self.url
    }
    
    /// Request headers
    ///
    /// Per WinterCG: https://developer.mozilla.org/en-US/docs/Web/API/Request/headers
    pub fn headers(&self) -> &NanoHeaders {
        &self.headers
    }
    
    /// Request body
    ///
    /// Per WinterCG: https://developer.mozilla.org/en-US/docs/Web/API/Request/body
    pub fn body(&self) -> Option<&Bytes> {
        self.body.as_ref()
    }
    
    /// Convert URL to full string representation
    pub fn url_string(&self) -> String {
        self.url.href()
    }
    
    /// Create from axum request (used in router handler)
    ///
    /// Converts an axum request into a NanoRequest, buffering the body
    /// in memory per D-05 (streaming will be added in Phase 6).
    ///
    /// # Arguments
    ///
    /// * `method` - HTTP method
    /// * `uri` - Request URI
    /// * `headers` - HTTP headers
    /// * `body` - Axum body
    ///
    /// # Returns
    ///
    /// `Ok(NanoRequest)` on success, or an error if conversion fails
    pub async fn from_axum_request(
        method: axum::http::Method,
        uri: axum::http::Uri,
        headers: axum::http::HeaderMap,
        body: axum::body::Body,
    ) -> anyhow::Result<Self> {
        let method_str = method.to_string();
        let url_str = uri.to_string();
        let nano_url = NanoUrl::parse(&url_str)?;
        let nano_headers = NanoHeaders::from_axum_headers(&headers);
        
        // D-05: Buffer small bodies in memory
        // For now, buffer all bodies (streaming comes in Phase 6)
        let body_bytes = axum::body::to_bytes(body, 1048576)  // 1MB limit
            .await?;
        let body = if body_bytes.is_empty() { None } else { Some(body_bytes) };
        
        Ok(Self::new(method_str, nano_url, nano_headers, body))
    }
}

/// WinterCG-compatible Response object
///
/// Represents an HTTP response returned from the JavaScript handler.
/// Contains all WinterCG Response properties: status, statusText, headers, body.
#[derive(Debug, Clone)]
pub struct NanoResponse {
    status: u16,
    status_text: String,
    headers: NanoHeaders,
    body: Option<Bytes>,
}

impl NanoResponse {
    /// Create a new Response
    ///
    /// # Arguments
    ///
    /// * `status` - HTTP status code
    /// * `headers` - HTTP headers
    /// * `body` - Optional response body
    ///
    /// # Returns
    ///
    /// A new `NanoResponse` with auto-generated status text
    pub fn new(status: u16, headers: NanoHeaders, body: Option<Bytes>) -> Self {
        let status_text = Self::default_status_text(status);
        Self { status, status_text, headers, body }
    }
    
    /// HTTP status code
    ///
    /// Per WinterCG: https://developer.mozilla.org/en-US/docs/Web/API/Response/status
    pub fn status(&self) -> u16 {
        self.status
    }
    
    /// HTTP status text
    ///
    /// Per WinterCG: https://developer.mozilla.org/en-US/docs/Web/API/Response/statusText
    pub fn status_text(&self) -> &str {
        &self.status_text
    }
    
    /// Response headers
    ///
    /// Per WinterCG: https://developer.mozilla.org/en-US/docs/Web/API/Response/headers
    pub fn headers(&self) -> &NanoHeaders {
        &self.headers
    }
    
    /// Mutable access to response headers
    pub fn headers_mut(&mut self) -> &mut NanoHeaders {
        &mut self.headers
    }
    
    /// Response body
    ///
    /// Per WinterCG: https://developer.mozilla.org/en-US/docs/Web/API/Response/body
    pub fn body(&self) -> Option<&Bytes> {
        self.body.as_ref()
    }
    
    /// Set the response body
    pub fn set_body(&mut self, body: Bytes) {
        self.body = Some(body);
    }
    
    /// Convert to axum response (used in router handler)
    ///
    /// Converts this NanoResponse into an axum response that can be
    /// returned from the HTTP handler.
    ///
    /// # Returns
    ///
    /// An axum `Response` with all headers and body
    pub fn to_axum_response(&self) -> axum::response::Response<axum::body::Body> {
        let mut builder = axum::response::Response::builder()
            .status(self.status);
        
        // Add headers
        for (name, values) in self.headers.entries() {
            for value in values {
                if let Ok(header_name) = axum::http::HeaderName::from_bytes(name.as_bytes()) {
                    if let Ok(header_value) = axum::http::HeaderValue::from_str(value) {
                        builder = builder.header(header_name, header_value);
                    }
                }
            }
        }
        
        // Build response with body
        let body = self.body.clone()
            .map(|b| axum::body::Body::from(b))
            .unwrap_or_else(axum::body::Body::empty);
        
        builder.body(body).unwrap_or_default()
    }
    
    fn default_status_text(status: u16) -> String {
        match status {
            200 => "OK".to_string(),
            201 => "Created".to_string(),
            204 => "No Content".to_string(),
            301 => "Moved Permanently".to_string(),
            302 => "Found".to_string(),
            304 => "Not Modified".to_string(),
            400 => "Bad Request".to_string(),
            401 => "Unauthorized".to_string(),
            403 => "Forbidden".to_string(),
            404 => "Not Found".to_string(),
            405 => "Method Not Allowed".to_string(),
            500 => "Internal Server Error".to_string(),
            502 => "Bad Gateway".to_string(),
            503 => "Service Unavailable".to_string(),
            _ => "Unknown".to_string(),
        }
    }
}

/// Builder pattern for NanoResponse
impl NanoResponse {
    /// Create a 200 OK response
    ///
    /// # Returns
    ///
    /// A new `NanoResponse` with status 200
    pub fn ok() -> Self {
        Self::new(200, NanoHeaders::default(), None)
    }
    
    /// Create a response with a specific status code
    ///
    /// # Arguments
    ///
    /// * `status` - HTTP status code
    ///
    /// # Returns
    ///
    /// A new `NanoResponse` with the given status
    pub fn with_status(status: u16) -> Self {
        Self::new(status, NanoHeaders::default(), None)
    }
    
    /// Add a body to the response
    ///
    /// # Arguments
    ///
    /// * `body` - The body content (any type that converts to String)
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(Bytes::from(body.into()));
        self
    }
    
    /// Add a header to the response
    ///
    /// # Arguments
    ///
    /// * `name` - Header name
    /// * `value` - Header value
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.set(name, value);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{NanoUrl, NanoHeaders};
    
    #[test]
    fn test_request_creation() {
        let url = NanoUrl::parse("https://example.com/api").unwrap();
        let headers = NanoHeaders::new();
        let request = NanoRequest::new(
            "GET".to_string(),
            url,
            headers,
            None,
        );
        
        assert_eq!(request.method(), "GET");
        assert_eq!(request.url_string(), "https://example.com/api");
    }
    
    #[test]
    fn test_response_creation() {
        let response = NanoResponse::ok()
            .with_header("Content-Type", "text/plain")
            .with_body("Hello, World!");
        
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers().get("Content-Type"), Some("text/plain".to_string()));
        assert_eq!(response.body().map(|b| String::from_utf8_lossy(b).to_string()),
                   Some("Hello, World!".to_string()));
    }
    
    #[test]
    fn test_response_status_text() {
        let response = NanoResponse::new(404, NanoHeaders::new(), None);
        assert_eq!(response.status_text(), "Not Found");
        
        let response200 = NanoResponse::new(200, NanoHeaders::new(), None);
        assert_eq!(response200.status_text(), "OK");
        
        let response500 = NanoResponse::new(500, NanoHeaders::new(), None);
        assert_eq!(response500.status_text(), "Internal Server Error");
    }
    
    #[test]
    fn test_response_builder_chaining() {
        let response = NanoResponse::ok()
            .with_header("Content-Type", "application/json")
            .with_header("X-Custom", "value")
            .with_body(r#"{"success":true}"#);
        
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers().get("Content-Type"), Some("application/json".to_string()));
        assert_eq!(response.headers().get("X-Custom"), Some("value".to_string()));
    }
    
    #[test]
    fn test_response_with_status() {
        let response = NanoResponse::with_status(201)
            .with_header("Location", "/new-resource")
            .with_body("Created");
        
        assert_eq!(response.status(), 201);
        assert_eq!(response.status_text(), "Created");
    }
    
    #[test]
    fn test_to_axum_response() {
        let response = NanoResponse::ok()
            .with_header("Content-Type", "text/plain")
            .with_body("Test body");
        
        let axum_resp = response.to_axum_response();
        
        assert_eq!(axum_resp.status(), 200);
        assert_eq!(
            axum_resp.headers().get("content-type").and_then(|v| v.to_str().ok()),
            Some("text/plain")
        );
    }
}
