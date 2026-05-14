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
//! # WinterTC Integration
//!
//! This module integrates with WinterTC types (NanoRequest/NanoResponse)
//! to enable JavaScript handler execution.
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
use crate::app::registry::AppRegistry;
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
/// Supports static responses for testing, WinterTC handlers for JS execution,
/// and static file serving for HTML/CSS/assets.
#[derive(Debug, Clone)]
pub enum HandlerType {
    /// Returns a fixed response string (for testing)
    StaticResponse(String),
    /// WinterTC handler that uses NanoRequest/NanoResponse
    WinterTCHandler(String),
    /// WinterTC handler for sliver-based (snapshot-restored) apps
    ///
    /// Contains the entrypoint path and optional sliver data reference
    WinterTCSliverHandler {
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

/// Execute a JavaScript handler standalone using a fresh V8 isolate
///
/// This helper creates a new V8 isolate on a blocking thread and executes
/// the entrypoint with the given request. It handles V8 platform initialization
/// and returns proper error responses on failure.
async fn execute_js_standalone(entrypoint: String, request: NanoRequest) -> NanoResponse {
    match tokio::task::spawn_blocking(move || {
        if let Err(e) = crate::v8::platform::initialize_platform() {
            return Err(format!("V8 platform initialization failed: {}", e));
        }
        
        let mut isolate = crate::v8::NanoIsolate::new()
            .map_err(|e| format!("Failed to create isolate: {}", e))?;
        
        let context = crate::runtime::HandlerContext {
            entrypoint,
            request,
        };
        
        crate::runtime::execute_handler(&mut isolate, context)
            .map_err(|e| format!("Handler execution failed: {}", e))
    }).await {
        Ok(Ok(response)) => response,
        Ok(Err(err_msg)) => {
            tracing::error!("Standalone JS handler error: {}", err_msg);
            NanoResponse::with_status(500)
                .with_header("Content-Type", "application/json")
                .with_body(format!(r#"{{"error":"InternalServerError","message":"{}","code":500}}"#, err_msg))
        }
        Err(e) => {
            tracing::error!("Standalone JS handler task failed: {}", e);
            NanoResponse::with_status(500)
                .with_header("Content-Type", "application/json")
                .with_body(r#"{"error":"InternalServerError","message":"Task execution failed","code":500}"#)
        }
    }
}

impl RouteTarget {
    /// Handle a request and return a WinterTC-compatible response
    ///
    /// This method processes a NanoRequest through the configured handler
    /// and returns a NanoResponse. It supports static responses and WinterTC
    /// handlers with standalone JavaScript execution.
    ///
    /// # Arguments
    ///
    /// * `request` - The WinterTC Request to process
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
            HandlerType::WinterTCHandler(_path) => {
                execute_js_standalone(_path.clone(), _request.clone()).await
            }
            HandlerType::WinterTCSliverHandler { entrypoint, .. } => {
                // Note: True snapshot restoration requires AppRegistry access.
                // In standalone mode we create a fresh isolate and execute the entrypoint.
                execute_js_standalone(entrypoint.clone(), _request.clone()).await
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
#[derive(Clone)]
pub struct AppState {
    /// The virtual host router for hostname-based request routing
    pub router: VirtualHostRouter,
    /// The WorkQueue for dispatching requests to worker pools
    pub work_queue: Arc<Mutex<WorkQueue>>,
    /// Optional AppRegistry for looking up app limits
    app_registry: Option<Arc<AppRegistry>>,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("router", &self.router)
            .field("work_queue", &"<WorkQueue>")
            .field("has_app_registry", &self.app_registry.is_some())
            .finish()
    }
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
    /// A new `AppState` with initialized WorkQueue (uses memory VFS backend)
    pub fn new(router: VirtualHostRouter, workers_per_pool: u32) -> Self {
        Self::with_vfs_config(router, workers_per_pool, None, None)
    }

    /// Create a new AppState with VFS disk backend configuration
    ///
    /// # Arguments
    ///
    /// * `router` - The virtual host router
    /// * `workers_per_pool` - Number of workers to create per hostname pool
    /// * `vfs_disk_config` - Optional disk backend configuration for VFS
    /// * `app_registry` - Optional AppRegistry for looking up app limits
    ///
    /// # Returns
    ///
    /// A new `AppState` with configured WorkQueue
    pub fn with_vfs_config(
        router: VirtualHostRouter,
        workers_per_pool: u32,
        vfs_disk_config: Option<crate::config::VfsDiskConfig>,
        app_registry: Option<Arc<AppRegistry>>,
    ) -> Self {
        Self {
            router,
            work_queue: Arc::new(Mutex::new(WorkQueue::with_vfs_config(
                workers_per_pool,
                vfs_disk_config,
                app_registry.clone(),
            ))),
            app_registry,
        }
    }

    /// Get CPU time limit for a hostname from the app registry
    ///
    /// Returns the configured CPU time limit in milliseconds if the app
    /// is found and CPU time tracking is enabled. Returns 0 if disabled
    /// or app not found (no limit).
    fn get_cpu_time_limit_ms(&self, hostname: &str) -> u32 {
        match &self.app_registry {
            None => 0,
            Some(registry) => {
                match registry.get(hostname) {
                    None => 0,
                    Some(app_config) => {
                        if app_config.limits.cpu_time_enabled {
                            app_config.limits.cpu_time_ms
                        } else {
                            0
                        }
                    }
                }
            }
        }
    }
}

/// JSON error response structure (per D-11)
///
/// Standard error format for routing errors and other failures.
/// 
/// Note: Defined for API completeness but currently unused. Will be used when
/// structured error responses are implemented for the virtual host router.
#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
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
/// Note: Currently unused but kept for future error response standardization.
#[allow(dead_code)]
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
    ///    - `WinterTCHandler`: Executes JavaScript standalone in a V8 isolate
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

    // Convert axum request to NanoRequest (WinterTC compatible)
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

    // Handle the request using the WinterTC-compatible handler
    let nano_response = target.handle(nano_request).await;

    // Calculate request duration
    let duration_ms = start.elapsed().as_secs_f64() * 1000.0;

    // Get status code from response
    let status = nano_response.status();
    let status_str = status.to_string();

    // Record metrics
    METRICS.record_request(&host, &status_str, duration_ms);

    // Get request path for access log
    let path = uri.path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");

    // HTTP Access Log - single line per request with all key info
    // Format: METHOD path host status duration_ms response_size
    tracing::info!(
        method = %method,
        path = %path,
        host = %host,
        status = status,
        duration_ms = format!("{:.2}", duration_ms),
        "HTTP {} {} - {} in {}ms",
        method,
        path,
        status,
        format!("{:.2}", duration_ms)
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
        HandlerType::WinterTCHandler(path) => path.clone(),
        HandlerType::WinterTCSliverHandler { entrypoint: path, .. } => path.clone(),
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

    // Get CPU time limit from app registry (0 means no limit)
    let cpu_time_limit_ms = state.get_cpu_time_limit_ms(&host);

    // Create handler task with hostname and request_id for distributed tracing
    let task = HandlerTask {
        entrypoint,
        request: nano_request,
        response_tx: tx,
        hostname: host.clone(),
        start_time: std::time::Instant::now(),
        cpu_time_limit_ms,
        request_id: request_id.clone(),
    };

    // Get request path for access log
    let path = uri.path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");

    // Dispatch to WorkQueue (async Mutex lock)
    let mut queue = state.work_queue.lock().await;

    // Validate through control plane before dispatching
    if let Some(ref control_plane) = queue.control_plane {
        if let Err(e) = control_plane.validate_request_ref(&task) {
            tracing::warn!("Control plane validation failed: {}", e);
            let response = Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("content-type", "text/plain")
                .body(Body::from(format!("Validation error: {}", e)))
                .unwrap();
            return response;
        }
    }

    let (response, status_code, worker_id, isolate_id) = match queue.dispatch(&host, task).await {
        Ok(()) => {
            // Wait for response from worker
            match rx.await {
                Ok(Ok(nano_response)) => {
                    let status = nano_response.status();
                    let worker_id = nano_response.worker_id();
                    let isolate_id = nano_response.isolate_id().map(|s| s.to_string());
                    (nano_response.to_axum_response(), status, worker_id, isolate_id)
                }
                Ok(Err(e)) => {
                    tracing::error!("Handler error: {}", e);
                    let response = Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .header("content-type", "text/plain")
                        .body(Body::from("Internal Server Error"))
                        .unwrap();
                    (response, 500, None, None)
                }
                Err(_) => {
                    tracing::error!("Response channel closed");
                    let response = Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .header("content-type", "text/plain")
                        .body(Body::from("Internal Server Error"))
                        .unwrap();
                    (response, 500, None, None)
                }
            }
        }
        Err(QueueError::ChannelFull) => {
            tracing::warn!("WorkQueue full for hostname: {}", host);
            let response = Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .header("Retry-After", "1")
                .header("content-type", "text/plain")
                .body(Body::from("Service Unavailable - Queue Full"))
                .unwrap();
            (response, 503, None, None)
        }
        Err(e) => {
            tracing::error!("Dispatch error: {}", e);
            let response = Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("content-type", "text/plain")
                .body(Body::from("Internal Server Error"))
                .unwrap();
            (response, 500, None, None)
        }
    };

    // Calculate duration and record metrics
    let duration_ms = start.elapsed().as_secs_f64() * 1000.0;
    METRICS.record_request(&host, &status_code.to_string(), duration_ms);

    // HTTP Access Log - single line per request with all key info
    // Include worker_id and isolate_id when available to show which worker/isolate processed the request
    match (worker_id, isolate_id) {
        (Some(wid), Some(iso)) => {
            let worker_id_u64 = wid as u64;
            tracing::info!(
                method = %method,
                path = %path,
                host = %host,
                status = status_code,
                worker_id = worker_id_u64,
                isolate_id = %iso,
                duration_ms = format!("{:.2}", duration_ms),
                "HTTP {} {} - {} in {}ms (worker: {}, isolate: {})",
                method,
                path,
                status_code,
                format!("{:.2}", duration_ms),
                wid,
                iso
            );
        }
        (Some(wid), None) => {
            let worker_id_u64 = wid as u64;
            tracing::info!(
                method = %method,
                path = %path,
                host = %host,
                status = status_code,
                worker_id = worker_id_u64,
                duration_ms = format!("{:.2}", duration_ms),
                "HTTP {} {} - {} in {}ms (worker: {})",
                method,
                path,
                status_code,
                format!("{:.2}", duration_ms),
                wid
            );
        }
        _ => {
            tracing::info!(
                method = %method,
                path = %path,
                host = %host,
                status = status_code,
                duration_ms = format!("{:.2}", duration_ms),
                "HTTP {} {} - {} in {}ms",
                method,
                path,
                status_code,
                format!("{:.2}", duration_ms)
            );
        }
    }

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
                handler_type: HandlerType::WinterTCHandler("/app/index.js".to_string()),
            },
        );

        let resolved = router.resolve("js.example.com");
        assert!(
            matches!(resolved.handler_type, HandlerType::WinterTCHandler(ref s) if s == "/app/index.js")
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
                handler_type: HandlerType::WinterTCSliverHandler {
                    entrypoint: "/app/index.js".to_string(),
                    hostname: "sliver.example.com".to_string(),
                },
            },
        );

        let resolved = router.resolve("sliver.example.com");
        match &resolved.handler_type {
            HandlerType::WinterTCSliverHandler { entrypoint, hostname } => {
                assert_eq!(entrypoint, "/app/index.js");
                assert_eq!(hostname, "sliver.example.com");
            }
            _ => panic!("Expected WinterTCSliverHandler"),
        }
    }

    #[tokio::test]
    async fn test_wintertc_handler_response() {
        crate::v8::platform::initialize_platform().expect("Failed to initialize V8 platform");
        
        let dynamic_token = format!("nanotest-{}", uuid::Uuid::new_v4());
        
        let temp_dir = tempfile::tempdir().unwrap();
        let js_path = temp_dir.path().join("index.js");
        let js_code = format!(r#"
export default {{
    fetch() {{
        return {{ status: 200, headers: {{ "Content-Type": "text/plain" }}, body: "ECHO:{}" }};
    }}
}};
"#, dynamic_token);
        std::fs::write(&js_path, js_code).unwrap();

        let target = RouteTarget {
            hostname: "js.example.com".to_string(),
            handler_type: HandlerType::WinterTCHandler(js_path.to_str().unwrap().to_string()),
        };

        let request = NanoRequest::new(
            "GET".to_string(),
            NanoUrl::parse("http://js.example.com/").unwrap(),
            NanoHeaders::new(),
            None,
        );

        let response = target.handle(request).await;
        assert_eq!(response.status(), 200);
        assert!(response.body().is_some());
        let body = String::from_utf8_lossy(response.body().as_ref().unwrap());
        assert!(body.contains(&format!("ECHO:{}", dynamic_token)), 
            "Response must contain dynamic token from JS execution, got: {}", body);
    }

    #[tokio::test]
    async fn test_sliver_handler_response() {
        crate::v8::platform::initialize_platform().expect("Failed to initialize V8 platform");
        
        let dynamic_token = format!("nanotest-{}", uuid::Uuid::new_v4());
        
        let temp_dir = tempfile::tempdir().unwrap();
        let js_path = temp_dir.path().join("index.js");
        let js_code = format!(r#"
export default {{
    fetch() {{
        return {{ status: 200, headers: {{ "Content-Type": "text/plain" }}, body: "SLIVER:{}" }};
    }}
}};
"#, dynamic_token);
        std::fs::write(&js_path, js_code).unwrap();

        let target = RouteTarget {
            hostname: "sliver.example.com".to_string(),
            handler_type: HandlerType::WinterTCSliverHandler {
                entrypoint: js_path.to_str().unwrap().to_string(),
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
        assert!(body.contains(&format!("SLIVER:{}", dynamic_token)), 
            "Response must contain dynamic sliver token from JS execution, got: {}", body);
    }
}
