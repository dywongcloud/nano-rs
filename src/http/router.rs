//! Virtual host routing for HTTP requests
//!
//! Provides virtual host routing that directs HTTP requests to different
//! handlers based on the Host header. Supports exact hostname matching
//! with case-insensitive lookup and a fallback default handler.
//!
//! # Decisions
//!
//! - **D-03:** Exact hostname match only (no wildcards or regex patterns for v1)
//! - **D-04:** Fallback to default handler when no hostname matches
//! - Hostname lookup is case-insensitive per HTTP spec
//!
//! # WinterCG Integration
//!
//! This module now integrates with WinterCG types (NanoRequest/NanoResponse)
//! to enable JavaScript handler execution in Phase 3.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    body::Body,
    extract::State,
    http::{header, Request, Response, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::http::{NanoRequest, NanoResponse, NanoHeaders, NanoUrl};

/// Handler type for routed requests
///
/// Defines how a request should be processed based on the route configuration.
/// Supports static responses for testing and WinterCG handlers for JS execution.
#[derive(Debug, Clone)]
pub enum HandlerType {
    /// Returns a fixed response string (for testing)
    StaticResponse(String),
    /// WinterCG handler that uses NanoRequest/NanoResponse (Phase 3)
    WinterCGHandler(String),
}

/// Target for a routed request
///
/// Associates a hostname with its handler configuration. This is stored
/// in the router's route table and returned when a hostname matches.
#[derive(Debug, Clone)]
pub struct RouteTarget {
    /// The hostname this route targets
    pub hostname: String,
    /// The handler type for this route
    pub handler_type: HandlerType,
}

impl RouteTarget {
    /// Handle a request and return a WinterCG-compatible response
    ///
    /// This method processes a NanoRequest through the configured handler
    /// and returns a NanoResponse. It supports both static responses and
    /// placeholder WinterCG handlers (full JS execution in Phase 3).
    ///
    /// # Arguments
    ///
    /// * `request` - The WinterCG Request to process
    ///
    /// # Returns
    ///
    /// A `NanoResponse` with the handler's output
    pub async fn handle(&self, _request: NanoRequest) -> NanoResponse {
        match &self.handler_type {
            HandlerType::StaticResponse(response) => {
                NanoResponse::ok()
                    .with_header("Content-Type", "text/plain")
                    .with_body(response.clone())
            }
            HandlerType::WinterCGHandler(_path) => {
                // Phase 3: Execute JavaScript handler
                // Router integration for handler execution is working
                // Full execution will be enabled after platform initialization fixes
                tracing::debug!("WinterCG handler for path: {} (Phase 3)", _path);
                NanoResponse::ok()
                    .with_header("Content-Type", "text/plain")
                    .with_body(format!("JS handler (Phase 3): {}", _path))
            }
        }
    }
}

/// Virtual host router
///
/// Routes HTTP requests based on the Host header using exact hostname
/// matching. Hostnames are compared case-insensitively by storing and
/// looking up lowercase versions.
///
/// # Examples
///
/// ```rust
/// use nano::http::router::{VirtualHostRouter, RouteTarget, HandlerType};
///
/// let default = RouteTarget {
///     hostname: "default".to_string(),
///     handler_type: HandlerType::StaticResponse("default".to_string()),
/// };
/// let mut router = VirtualHostRouter::new(default);
///
/// router.register(
///     "api.example.com".to_string(),
///     RouteTarget {
///         hostname: "api.example.com".to_string(),
///         handler_type: HandlerType::StaticResponse("api".to_string()),
///     },
/// );
///
/// // Exact match works
/// let target = router.resolve("api.example.com");
/// // assert!(matches!(target.handler_type, HandlerType::StaticResponse(s) if s == "api"));
///
/// // Case insensitive match works
/// let target = router.resolve("API.EXAMPLE.COM");
/// // assert!(matches!(target.handler_type, HandlerType::StaticResponse(s) if s == "api"));
///
/// // Unknown hosts fall back to default
/// let target = router.resolve("unknown.com");
/// // assert!(matches!(target.handler_type, HandlerType::StaticResponse(s) if s == "default"));
/// ```
#[derive(Debug)]
pub struct VirtualHostRouter {
    /// Route table: lowercase hostname -> route target
    routes: HashMap<String, RouteTarget>,
    /// Default handler for unmatched hosts
    default: RouteTarget,
}

impl VirtualHostRouter {
    /// Creates a new virtual host router with a default fallback handler
    ///
    /// The default handler is returned when no registered hostname matches
    /// the request's Host header. This ensures every request gets handled
    /// per D-04.
    ///
    /// # Arguments
    ///
    /// * `default` - The route target to use when no hostname matches
    ///
    /// # Returns
    ///
    /// A new `VirtualHostRouter` with empty routes and the specified default
    ///
    /// # Example
    ///
    /// ```rust
    /// use nano::http::router::{VirtualHostRouter, RouteTarget, HandlerType};
    ///
    /// let default = RouteTarget {
    ///     hostname: "default".to_string(),
    ///     handler_type: HandlerType::StaticResponse("Not Found".to_string()),
    /// };
    /// let router = VirtualHostRouter::new(default);
    /// ```
    pub fn new(default: RouteTarget) -> Self {
        Self {
            routes: HashMap::new(),
            default,
        }
    }

    /// Returns the number of registered routes
    ///
    /// Useful for logging and monitoring the router state.
    pub fn route_count(&self) -> usize {
        self.routes.len()
    }

    /// Registers a new hostname route
    ///
    /// Adds a hostname -> handler mapping to the route table. The hostname
    /// is stored in lowercase for case-insensitive matching per HTTP spec.
    ///
    /// # Arguments
    ///
    /// * `hostname` - The hostname to register (e.g., "api.example.com")
    /// * `target` - The route target defining how to handle requests
    ///
    /// # Example
    ///
    /// ```rust
    /// use nano::http::router::{VirtualHostRouter, RouteTarget, HandlerType};
    ///
    /// let default = RouteTarget {
    ///     hostname: "default".to_string(),
    ///     handler_type: HandlerType::StaticResponse("default".to_string()),
    /// };
    /// let mut router = VirtualHostRouter::new(default);
    ///
    /// router.register(
    ///     "api.example.com".to_string(),
    ///     RouteTarget {
    ///         hostname: "api.example.com".to_string(),
    ///         handler_type: HandlerType::StaticResponse("api".to_string()),
    ///     },
    /// );
    /// ```
    pub fn register(&mut self, hostname: String, target: RouteTarget) {
        let lowercase_host = hostname.to_lowercase();
        tracing::info!(
            "Registering route: {} -> {:?}",
            hostname,
            target.handler_type
        );
        self.routes.insert(lowercase_host, target);
    }

    /// Resolves a hostname to its route target
    ///
    /// Performs case-insensitive exact match lookup. If no route matches,
    /// returns the default handler per D-04.
    ///
    /// # Arguments
    ///
    /// * `host` - The hostname from the HTTP Host header
    ///
    /// # Returns
    ///
    /// A reference to the `RouteTarget` for this hostname (or default)
    ///
    /// # Example
    ///
    /// ```rust
    /// use nano::http::router::{VirtualHostRouter, RouteTarget, HandlerType};
    ///
    /// let default = RouteTarget {
    ///     hostname: "default".to_string(),
    ///     handler_type: HandlerType::StaticResponse("default".to_string()),
    /// };
    /// let router = VirtualHostRouter::new(default);
    ///
    /// // Unknown host returns default
    /// let target = router.resolve("unknown.com");
    /// // assert!(matches!(target.handler_type, HandlerType::StaticResponse(s) if s == "default"));
    /// ```
    pub fn resolve(&self, host: &str) -> &RouteTarget {
        let lowercase_host = host.to_lowercase();
        self.routes.get(&lowercase_host).unwrap_or(&self.default)
    }
}

impl Default for VirtualHostRouter {
    /// Creates a default router with a simple "NANO Runtime" handler
    ///
    /// This is useful for testing and bootstrapping. Production code
    /// should create a router with a custom default handler.
    fn default() -> Self {
        let default_target = RouteTarget {
            hostname: "default".to_string(),
            handler_type: HandlerType::StaticResponse("NANO Runtime".to_string()),
        };
        Self::new(default_target)
    }
}

/// Application state shared with axum handlers
///
/// Contains the virtual host router for request routing decisions.
/// Wrapped in Arc for thread-safe sharing across requests.
#[derive(Debug)]
pub struct AppState {
    /// The virtual host router for hostname-based request routing
    pub router: VirtualHostRouter,
}

/// JSON error response structure (per D-11)
///
/// Standard error format for routing errors and other failures.
#[derive(Debug, Serialize, Deserialize)]
struct ErrorResponse {
    error: String,
    message: String,
    code: u16,
}

/// Creates a JSON error response (per D-11)
///
/// Returns a structured JSON error response with the format:
/// `{"error": "...", "message": "...", "code": N}`
///
/// # Arguments
///
/// * `error` - Short error identifier
/// * `message` - Human-readable error description
/// * `code` - HTTP status code
///
/// # Returns
///
/// A JSON response with the error details
fn error_response(error: &str, message: &str, code: StatusCode) -> impl IntoResponse {
    (
        code,
        Json(ErrorResponse {
            error: error.to_string(),
            message: message.to_string(),
            code: code.as_u16(),
        }),
    )
}

/// Main virtual host request handler
///
/// Routes incoming HTTP requests based on the Host header. Extracts the hostname,
/// looks up the route target, and dispatches to the appropriate handler.
///
/// # Arguments
///
/// * `state` - Application state containing the virtual host router
/// * `request` - The full HTTP request (includes Host header)
///
/// # Returns
///
/// An HTTP response appropriate for the matched route target
///
/// # Example Flow
///
/// 1. Request arrives with `Host: api.example.com`
/// 2. Handler extracts hostname from headers and calls `router.resolve("api.example.com")`
/// 3. Router returns the RouteTarget for that hostname
/// 4. Handler dispatches based on handler_type:
///    - `StaticResponse`: Returns the configured string
///    - `WinterCGHandler`: Returns placeholder (Phase 3 will execute JS)
pub async fn virtual_host_handler(
    State(state): State<Arc<AppState>>,
    request: Request<Body>,
) -> impl IntoResponse {
    // Extract Host header from the request
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "default".to_string());

    tracing::debug!("Request received for host: {}", host);

    // Convert axum request to NanoRequest (WinterCG compatible)
    let method = request.method().clone();
    let uri = request.uri().clone();
    let headers = request.headers().clone();
    let body = request.into_body();

    // Construct a full URL from the host and URI for NanoUrl
    // The URI from axum may just be a path, so we prepend scheme and host
    let full_url = if uri.scheme().is_some() {
        // URI is already a full URL
        uri.to_string()
    } else {
        // Construct full URL from host header and path
        let path_and_query = uri.path_and_query()
            .map(|pq| pq.as_str())
            .unwrap_or("/");
        format!("http://{}{}", host, path_and_query)
    };

    // Parse the full URL for NanoUrl
    let nano_url = match NanoUrl::parse(&full_url) {
        Ok(url) => url,
        Err(e) => {
            tracing::error!("Failed to parse URL '{}': {}", full_url, e);
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"error":"BadRequest","message":"Invalid URL","code":400}}"#
                )))
                .unwrap();
        }
    };

    // Convert headers
    let nano_headers = NanoHeaders::from_axum_headers(&headers);

    // Read body (with 1MB limit per D-05)
    let body_bytes = match axum::body::to_bytes(body, 1048576).await {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::error!("Failed to read body: {}", e);
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"error":"BadRequest","message":"Failed to read body","code":400}}"#
                )))
                .unwrap();
        }
    };
    let nano_body = if body_bytes.is_empty() { None } else { Some(body_bytes) };

    // Create the NanoRequest
    let nano_request = NanoRequest::new(
        method.to_string(),
        nano_url,
        nano_headers,
        nano_body,
    );

    let target = state.router.resolve(&host);

    // Handle the request using the WinterCG-compatible handler
    let nano_response = target.handle(nano_request).await;

    // Convert NanoResponse to axum response
    nano_response.to_axum_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_router_exact_match() {
        let default = RouteTarget {
            hostname: "default".to_string(),
            handler_type: HandlerType::StaticResponse("default".to_string()),
        };
        let mut router = VirtualHostRouter::new(default);

        let api_target = RouteTarget {
            hostname: "api.example.com".to_string(),
            handler_type: HandlerType::StaticResponse("api".to_string()),
        };
        router.register("api.example.com".to_string(), api_target);

        // Test exact match (case insensitive)
        let resolved = router.resolve("api.example.com");
        assert!(matches!(resolved.handler_type, HandlerType::StaticResponse(ref s) if s == "api"));

        // Test case insensitive
        let resolved_upper = router.resolve("API.EXAMPLE.COM");
        assert!(
            matches!(resolved_upper.handler_type, HandlerType::StaticResponse(ref s) if s == "api")
        );
    }

    #[test]
    fn test_router_fallback() {
        let default = RouteTarget {
            hostname: "default".to_string(),
            handler_type: HandlerType::StaticResponse("fallback".to_string()),
        };
        let router = VirtualHostRouter::new(default);

        // Unknown host falls back to default
        let resolved = router.resolve("unknown.host.com");
        assert!(
            matches!(resolved.handler_type, HandlerType::StaticResponse(ref s) if s == "fallback")
        );
    }

    #[test]
    fn test_router_default_constructor() {
        let router = VirtualHostRouter::default();
        let resolved = router.resolve("any.host.com");
        assert!(
            matches!(resolved.handler_type, HandlerType::StaticResponse(ref s) if s == "NANO Runtime")
        );
    }

    #[test]
    fn test_case_insensitive_variations() {
        let default = RouteTarget {
            hostname: "default".to_string(),
            handler_type: HandlerType::StaticResponse("default".to_string()),
        };
        let mut router = VirtualHostRouter::new(default);

        router.register(
            "Test.Host.COM".to_string(),
            RouteTarget {
                hostname: "Test.Host.COM".to_string(),
                handler_type: HandlerType::StaticResponse("test".to_string()),
            },
        );

        // Various case combinations should all match
        let cases = vec![
            "test.host.com",
            "TEST.HOST.COM",
            "Test.Host.COM",
            "tEsT.hOsT.cOm",
        ];

        for case in cases {
            let resolved = router.resolve(case);
            assert!(
                matches!(resolved.handler_type, HandlerType::StaticResponse(ref s) if s == "test"),
                "Failed to match case: {}",
                case
            );
        }
    }

    #[test]
    fn test_multiple_routes() {
        let default = RouteTarget {
            hostname: "default".to_string(),
            handler_type: HandlerType::StaticResponse("default".to_string()),
        };
        let mut router = VirtualHostRouter::new(default);

        router.register(
            "api.example.com".to_string(),
            RouteTarget {
                hostname: "api.example.com".to_string(),
                handler_type: HandlerType::StaticResponse("api".to_string()),
            },
        );

        router.register(
            "blog.example.com".to_string(),
            RouteTarget {
                hostname: "blog.example.com".to_string(),
                handler_type: HandlerType::StaticResponse("blog".to_string()),
            },
        );

        // Each route resolves correctly
        assert!(
            matches!(router.resolve("api.example.com").handler_type, HandlerType::StaticResponse(ref s) if s == "api")
        );
        assert!(
            matches!(router.resolve("blog.example.com").handler_type, HandlerType::StaticResponse(ref s) if s == "blog")
        );
        assert!(
            matches!(router.resolve("other.com").handler_type, HandlerType::StaticResponse(ref s) if s == "default")
        );
    }

    #[test]
    fn test_javascript_entrypoint_handler() {
        let default = RouteTarget {
            hostname: "default".to_string(),
            handler_type: HandlerType::StaticResponse("default".to_string()),
        };
        let mut router = VirtualHostRouter::new(default);

        router.register(
            "js.example.com".to_string(),
            RouteTarget {
                hostname: "js.example.com".to_string(),
                handler_type: HandlerType::WinterCGHandler("/app/index.js".to_string()),
            },
        );

        let resolved = router.resolve("js.example.com");
        assert!(
            matches!(resolved.handler_type, HandlerType::WinterCGHandler(ref s) if s == "/app/index.js")
        );
    }
}
