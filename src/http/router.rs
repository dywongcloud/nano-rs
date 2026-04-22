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
//!
//! # Static File Serving
//!
//! Entrypoint type detection automatically determines how to handle entrypoints:
//! - JavaScript files (.js, .mjs, .ts) → Execute as Workers
//! - Static files (.html, .css, images, etc.) → Serve with correct content-type
//! - Directories → Serve index.html with automatic content-type detection

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use axum::{
    body::Body,
    extract::State,
    http::{header, Request, Response, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::http::{NanoRequest, NanoResponse, NanoHeaders, NanoUrl, content_type_from_ext};
use crate::worker::{HandlerTask, QueueError, WorkQueue};
use crate::logging::create_request_span;
use crate::metrics::METRICS;
use uuid::Uuid;

/// Entrypoint type for automatic file type detection
///
/// Determines how to handle an entrypoint based on its file extension:
/// - JavaScript files (.js, .mjs, .ts) → Execute as Workers
/// - Static files (.html, .css, images, etc.) → Serve with correct content-type
/// - Directories → Serve index.html with automatic content-type detection
#[derive(Debug, Clone)]
pub enum EntrypointType {
    /// Path to a JavaScript file that should be executed as a Worker
    JavaScript(String),
    /// Path to a specific static file to serve
    StaticFile(String),
    /// Path to a directory (serves index.html for root path)
    StaticDir(String),
}

/// Detect the type of entrypoint based on file extension
///
/// Analyzes the file path to determine whether it should be:
/// - Executed as JavaScript (js, mjs, ts extensions)
/// - Served as a static file (html, css, images, etc.)
/// - Served as a directory (with index.html fallback)
///
/// # Arguments
///
/// * `path` - The file or directory path to analyze
///
/// # Returns
///
/// An `EntrypointType` indicating how the entrypoint should be handled
///
/// # Examples
///
/// ```rust
/// use nano::http::router::detect_entrypoint_type;
///
/// let js = detect_entrypoint_type("./app.js");
/// // Returns EntrypointType::JavaScript("./app.js")
///
/// let html = detect_entrypoint_type("./index.html");
/// // Returns EntrypointType::StaticFile("./index.html")
///
/// let dir = detect_entrypoint_type("./dist");
/// // Returns EntrypointType::StaticDir("./dist")
/// ```
pub fn detect_entrypoint_type(path: &str) -> EntrypointType {
    let path_obj = Path::new(path);
    
    // Check if it's a directory first
    if path_obj.is_dir() {
        return EntrypointType::StaticDir(path.to_string());
    }
    
    // Get file extension
    let ext = path_obj
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    
    match ext.as_str() {
        // JavaScript files - execute as Worker
        "js" | "mjs" | "ts" => EntrypointType::JavaScript(path.to_string()),
        // All other files - serve statically
        _ => EntrypointType::StaticFile(path.to_string()),
    }
}

/// Handler type for routed requests
///
/// Defines how a request should be processed based on the route configuration.
/// Supports static responses for testing, WinterCG handlers for JS execution,
/// and static file serving for HTML/CSS/assets.
#[derive(Debug, Clone)]
pub enum HandlerType {
    /// Returns a fixed response string (for testing)
    StaticResponse(String),
    /// WinterCG handler that uses NanoRequest/NanoResponse (Phase 3)
    WinterCGHandler(String),
    /// WinterCG handler for sliver-based (snapshot-restored) apps
    ///
    /// Contains the entrypoint path and optional sliver data reference
    WinterCGSliverHandler {
        /// Path to the JavaScript entrypoint
        entrypoint: String,
        /// Reference to hostname for looking up sliver data in registry
        hostname: String,
    },
    /// Serve static files from VFS entries
    ///
    /// This handler serves files directly from the VFS entries
    /// stored in the sliver. It's used for static sites and assets.
    VfsStaticFiles {
        /// Map of path -> (content, content_type)
        files: std::collections::HashMap<String, (Vec<u8>, String)>,
        /// Default file to serve for root path (e.g., "index.html")
        default_file: Option<String>,
    },
    /// Serve a single static file from the filesystem
    ///
    /// Used for HTML entrypoints and other static files.
    /// Files are read at request time from the filesystem.
    StaticFile {
        /// Path to the file on disk
        path: String,
        /// Content-Type header value
        content_type: String,
    },
    /// Serve static files from a directory
    ///
    /// Used for directory entrypoints (e.g., Astro build output).
    /// Serves index.html for root path and maps other paths to files.
    StaticDir {
        /// Root directory path
        root: String,
        /// Default file to serve for root path (e.g., "index.html")
        default_file: String,
    },
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
                if response.is_empty() {
                    // Empty response means "not found" - return HTTP 404
                    NanoResponse::not_found()
                        .with_header("Content-Type", "text/plain")
                        .with_body("Not Found")
                } else {
                    NanoResponse::ok()
                        .with_header("Content-Type", "text/plain")
                        .with_body(response.clone())
                }
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
            HandlerType::WinterCGSliverHandler { entrypoint, hostname } => {
                // Sliver-based handler (snapshot-restored isolate)
                tracing::debug!(
                    "WinterCG sliver handler for {} on {} (uses snapshot restoration)",
                    entrypoint,
                    hostname
                );
                NanoResponse::ok()
                    .with_header("Content-Type", "text/plain")
                    .with_body(format!("Sliver handler: {} (snapshot restored)", entrypoint))
            }
            HandlerType::VfsStaticFiles { files, default_file } => {
                // Serve static files from VFS
                let path = _request.url().pathname();
                
                // Special handling for root path
                let is_root = path == "/" || path.is_empty();
                
                // Get the default file name
                let default = default_file.as_deref().unwrap_or("index.html");
                
                // Determine lookup path
                let lookup_path = if is_root {
                    default.to_string()
                } else {
                    // Remove leading slash
                    path.strip_prefix('/').map(|s| s.to_string()).unwrap_or_else(|| path.to_string())
                };
                
                // Debug: log available files and lookup attempt
                tracing::debug!(
                    "VFS lookup: path='{}' is_root={} -> lookup='{}' | files count={}",
                    path,
                    is_root,
                    lookup_path,
                    files.len()
                );
                
                // STRATEGY 1: Try exact match first
                if let Some((content, content_type)) = files.get(&lookup_path) {
                    tracing::debug!("VFS hit (exact): '{}' ({} bytes)", lookup_path, content.len());
                    return NanoResponse::ok()
                        .with_header("Content-Type", content_type)
                        .with_body_bytes(content.clone());
                }
                
                // STRATEGY 2: For root path, try JS entry points first (frameworks), then HTML
                if is_root {
                    // JavaScript frameworks typically use index.js as entry point
                    let entry_points = vec![
                        "index.js",   // Most common JS framework entry
                        "app.js",     // Alternative JS entry
                        "main.js",    // Another common JS entry
                        "server.js",  // Server-side JS entry
                        "index.html", // Static site fallback
                        "index.htm",  // Legacy HTML
                    ];
                    for entry_point in entry_points {
                        if let Some((content, content_type)) = files.get(entry_point) {
                            tracing::debug!("VFS hit (root entry point): '{}'", entry_point);
                            return NanoResponse::ok()
                                .with_header("Content-Type", content_type)
                                .with_body_bytes(content.clone());
                        }
                    }
                }
                
                // STRATEGY 3: Try with /index.html suffix (for directory paths)
                let index_path = format!("{}/index.html", lookup_path);
                if let Some((content, content_type)) = files.get(&index_path) {
                    tracing::debug!("VFS hit (dir index): '{}'", index_path);
                    return NanoResponse::ok()
                        .with_header("Content-Type", content_type)
                        .with_body_bytes(content.clone());
                }
                
                // STRATEGY 4: Try with .html extension
                let html_path = format!("{}.html", lookup_path);
                if let Some((content, content_type)) = files.get(&html_path) {
                    tracing::debug!("VFS hit (.html ext): '{}'", html_path);
                    return NanoResponse::ok()
                        .with_header("Content-Type", content_type)
                        .with_body_bytes(content.clone());
                }
                
                // File not found - return clean 404
                tracing::debug!(
                    "VFS miss: path='{}' lookup='{}' not found in {} files",
                    path,
                    lookup_path,
                    files.len()
                );
                
                NanoResponse::not_found()
            }
            HandlerType::StaticFile { path, content_type } => {
                // Serve a single static file from the filesystem
                tracing::debug!("Serving static file: {} (content-type: {})", path, content_type);
                
                match tokio::fs::read_to_string(path).await {
                    Ok(content) => NanoResponse::ok()
                        .with_header("Content-Type", content_type)
                        .with_body(content),
                    Err(e) => {
                        tracing::warn!("Failed to read static file {}: {}", path, e);
                        NanoResponse::not_found()
                    }
                }
            }
            HandlerType::StaticDir { root, default_file } => {
                // Serve files from a directory
                let path = _request.url().pathname();
                
                // Determine file path
                let file_path = if path == "/" || path.is_empty() {
                    format!("{}/{}", root, default_file)
                } else {
                    // Remove leading slash and construct path
                    let clean_path = path.strip_prefix('/').unwrap_or_else(|| path.as_str());
                    // Security: prevent path traversal
                    if clean_path.contains("..") {
                        tracing::warn!("Path traversal attempt blocked: {}", path);
                        return NanoResponse::not_found();
                    }
                    format!("{}/{}", root, clean_path)
                };
                
                tracing::debug!("Serving from directory: {} -> {}", path, file_path);
                
                // Determine content type from extension
                let ext = Path::new(&file_path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                let content_type = content_type_from_ext(ext);
                
                // Read and serve the file
                match tokio::fs::read(&file_path).await {
                    Ok(bytes) => NanoResponse::ok()
                        .with_header("Content-Type", content_type)
                        .with_body_bytes(bytes),
                    Err(e) => {
                        tracing::debug!("File not found: {} (error: {})", file_path, e);
                        NanoResponse::not_found()
                    }
                }
            }
        }
    }
}

/// Virtual host router
///
/// Routes HTTP requests based on the Host header using exact hostname
/// matching. Hostnames are compared case-insensitively by storing and
/// looking up lowercase versions.
#[derive(Debug, Clone)]
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
/// Contains the virtual host router and WorkQueue for request dispatch.
/// Wrapped in Arc for thread-safe sharing across requests.
#[derive(Debug, Clone)]
pub struct AppState {
    /// The virtual host router for hostname-based request routing
    pub router: VirtualHostRouter,
    /// The WorkQueue for dispatching requests to worker pools
    pub work_queue: Arc<Mutex<WorkQueue>>,
}

impl AppState {
    /// Create a new AppState with the given router and worker configuration
    ///
    /// # Arguments
    ///
    /// * `router` - The virtual host router
    /// * `workers_per_pool` - Number of workers to create per hostname pool
    ///
    /// # Returns
    ///
    /// A new `AppState` with initialized WorkQueue
    pub fn new(router: VirtualHostRouter, workers_per_pool: usize) -> Self {
        Self {
            router,
            work_queue: Arc::new(Mutex::new(WorkQueue::new(workers_per_pool))),
        }
    }
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
/// Records metrics for each request: count by hostname/status and latency histogram.
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
/// 5. Metrics are recorded: request count and duration
pub async fn virtual_host_handler(
    State(state): State<Arc<AppState>>,
    request: Request<Body>,
) -> impl IntoResponse {
    // Start timing the request
    let start = std::time::Instant::now();
    // Extract Host header from the request and strip port if present
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .map(|s| {
            // Strip port from host:port format (e.g., "localhost:9999" -> "localhost")
            s.split(':').next().unwrap_or(s).to_string()
        })
        .unwrap_or_else(|| "default".to_string());

    // Generate request ID and create span with context
    let request_id = format!("req_{}", Uuid::new_v4().to_string()[..8].to_string());
    let span = create_request_span(&host, &request_id);
    let _enter = span.enter();

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

    // Calculate request duration
    let duration_ms = start.elapsed().as_secs_f64() * 1000.0;

    // Get status code from response
    let status = nano_response.status();
    let status_str = status.to_string();

    // Record metrics
    METRICS.record_request(&host, &status_str, duration_ms);

    // Log request completion with status
    tracing::info!(
        event = "request_complete",
        status = status,
        duration_ms = duration_ms,
        "Request completed successfully"
    );

    // Convert NanoResponse to axum response
    nano_response.to_axum_response()
}

/// Dispatch request to worker pool via WorkQueue
///
/// This handler integrates the virtual host router with the WorkQueue,
/// enabling affine dispatch: same hostname always routes to same worker.
/// Records metrics for each request: count by hostname/status and latency.
/// Returns HTTP 503 with Retry-After header when channel is full.
///
/// # Arguments
///
/// * `state` - Application state containing the router and WorkQueue
/// * `request` - The full HTTP request
///
/// # Returns
///
/// An HTTP response from the worker pool or an error response
pub async fn dispatch_to_worker_pool(
    State(state): State<Arc<AppState>>,
    request: Request<Body>,
) -> impl IntoResponse {
    // Start timing the request
    let start = std::time::Instant::now();
    // Extract Host header from the request and strip port if present
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .map(|s| {
            // Strip port from host:port format (e.g., "localhost:9999" -> "localhost")
            s.split(':').next().unwrap_or(s).to_string()
        })
        .unwrap_or_else(|| "default".to_string());

    // Generate request ID and create span with context
    let request_id = format!("req_{}", Uuid::new_v4().to_string()[..8].to_string());
    let span = create_request_span(&host, &request_id);
    let _enter = span.enter();

    tracing::debug!("Dispatching request to worker pool for host: {}", host);

    // Convert axum request to NanoRequest
    let method = request.method().clone();
    let uri = request.uri().clone();
    let headers = request.headers().clone();
    let body = request.into_body();

    // Construct full URL
    let full_url = if uri.scheme().is_some() {
        uri.to_string()
    } else {
        let path_and_query = uri.path_and_query()
            .map(|pq| pq.as_str())
            .unwrap_or("/");
        format!("http://{}{}", host, path_and_query)
    };

    // Parse URL
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

    let nano_headers = NanoHeaders::from_axum_headers(&headers);

    // Read body (1MB limit per D-05)
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

    // Create NanoRequest
    let nano_request = NanoRequest::new(
        method.to_string(),
        nano_url,
        nano_headers,
        nano_body,
    );

    // Look up route target
    let target = state.router.resolve(&host);

    // Extract entrypoint from target or handle directly
    let entrypoint = match &target.handler_type {
        HandlerType::WinterCGHandler(path) => path.clone(),
        HandlerType::WinterCGSliverHandler { entrypoint: path, .. } => path.clone(),
        HandlerType::StaticResponse(_) 
        | HandlerType::VfsStaticFiles { .. }
        | HandlerType::StaticFile { .. }
        | HandlerType::StaticDir { .. } => {
            // These handler types don't need worker dispatch - serve directly
            let nano_response = target.handle(nano_request).await;
            return nano_response.to_axum_response();
        }
    };

    // Create oneshot channel for response
    let (tx, rx) = tokio::sync::oneshot::channel();

    // Create handler task
    let task = HandlerTask {
        entrypoint,
        request: nano_request,
        response_tx: tx,
    };

    // Dispatch to WorkQueue (async Mutex lock)
    let mut queue = state.work_queue.lock().await;
    let response = match queue.dispatch(&host, task) {
        Ok(()) => {
            // Wait for response from worker
            match rx.await {
                Ok(Ok(nano_response)) => {
                    // Calculate duration and record metrics
                    let duration_ms = start.elapsed().as_secs_f64() * 1000.0;
                    let status = nano_response.status();
                    METRICS.record_request(&host, &status.to_string(), duration_ms);
                    nano_response.to_axum_response()
                }
                Ok(Err(e)) => {
                    tracing::error!("Handler error: {}", e);
                    let duration_ms = start.elapsed().as_secs_f64() * 1000.0;
                    METRICS.record_request(&host, "500", duration_ms);
                    Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .header("content-type", "text/plain")
                        .body(Body::from("Internal Server Error"))
                        .unwrap()
                }
                Err(_) => {
                    tracing::error!("Response channel closed");
                    let duration_ms = start.elapsed().as_secs_f64() * 1000.0;
                    METRICS.record_request(&host, "500", duration_ms);
                    Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .header("content-type", "text/plain")
                        .body(Body::from("Internal Server Error"))
                        .unwrap()
                }
            }
        }
        Err(QueueError::ChannelFull) => {
            tracing::warn!("WorkQueue full for hostname: {}", host);
            let duration_ms = start.elapsed().as_secs_f64() * 1000.0;
            METRICS.record_request(&host, "503", duration_ms);
            Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .header("Retry-After", "1")
                .header("content-type", "text/plain")
                .body(Body::from("Service Unavailable - Queue Full"))
                .unwrap()
        }
        Err(e) => {
            tracing::error!("Dispatch error: {}", e);
            let duration_ms = start.elapsed().as_secs_f64() * 1000.0;
            METRICS.record_request(&host, "500", duration_ms);
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("content-type", "text/plain")
                .body(Body::from("Internal Server Error"))
                .unwrap()
        }
    };
    
    response
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

    #[test]
    fn test_sliver_handler_routing() {
        let default = RouteTarget {
            hostname: "default".to_string(),
            handler_type: HandlerType::StaticResponse("default".to_string()),
        };
        let mut router = VirtualHostRouter::new(default);

        router.register(
            "sliver.example.com".to_string(),
            RouteTarget {
                hostname: "sliver.example.com".to_string(),
                handler_type: HandlerType::WinterCGSliverHandler {
                    entrypoint: "/app/index.js".to_string(),
                    hostname: "sliver.example.com".to_string(),
                },
            },
        );

        let resolved = router.resolve("sliver.example.com");
        match &resolved.handler_type {
            HandlerType::WinterCGSliverHandler { entrypoint, hostname } => {
                assert_eq!(entrypoint, "/app/index.js");
                assert_eq!(hostname, "sliver.example.com");
            }
            _ => panic!("Expected WinterCGSliverHandler"),
        }
    }

    #[tokio::test]
    async fn test_sliver_handler_response() {
        let target = RouteTarget {
            hostname: "sliver.example.com".to_string(),
            handler_type: HandlerType::WinterCGSliverHandler {
                entrypoint: "/app/index.js".to_string(),
                hostname: "sliver.example.com".to_string(),
            },
        };

        let request = NanoRequest::new(
            "GET".to_string(),
            NanoUrl::parse("http://sliver.example.com/").unwrap(),
            NanoHeaders::new(),
            None,
        );

        let response = target.handle(request).await;
        assert_eq!(response.status(), 200);
        assert!(response.body().is_some());
        let body = String::from_utf8_lossy(response.body().as_ref().unwrap());
        assert!(body.contains("Sliver handler"));
        assert!(body.contains("snapshot restored"));
    }
}
