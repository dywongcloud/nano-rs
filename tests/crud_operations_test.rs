//! CRUD Operations Integration Tests
//!
//! Tests Create, Read, Update, Delete operations via HTTP handlers.
//! These tests verify full REST API functionality.

use std::sync::Arc;
use tokio::sync::oneshot;

use nano::http::{NanoHeaders, NanoRequest, NanoResponse, NanoUrl};
use nano::v8::initialize_platform;
use nano::worker::{HandlerTask, WorkQueue};
use nano::vfs::{IsolateVfs, MemoryBackend, VfsNamespace, VfsBackendEnum};

/// Test CREATE operation - POST request creates a resource
#[tokio::test]
async fn test_crud_create() {
    let _ = initialize_platform();
    
    let mut queue = WorkQueue::new(1);
    let hostname = "crud-test.local";
    
    // Handler with in-memory storage
    let js_code = r#"
        const storage = new Map();
        let nextId = 1;
        
        export default {
            async fetch(request) {
                const url = new URL(request.url);
                
                if (request.method === 'POST' && url.pathname === '/items') {
                    const body = await request.json();
                    const id = nextId++;
                    const item = { id, ...body, created: Date.now() };
                    storage.set(id, item);
                    
                    return new Response(JSON.stringify(item), {
                        status: 201,
                        headers: { 'Content-Type': 'application/json' }
                    });
                }
                
                return new Response('Not Found', { status: 404 });
            }
        };
    "#;
    
    // Setup VFS
    let vfs = IsolateVfs::new(
        VfsNamespace::from_hostname(hostname),
        VfsBackendEnum::memory(MemoryBackend::default()),
    );
    vfs.write("/app.js", js_code.as_bytes()).await.unwrap();
    
    // Create request
    let (tx, rx) = oneshot::channel();
    let url = NanoUrl::parse(&format!("http://{}/items", hostname)).unwrap();
    let mut headers = NanoHeaders::new();
    headers.set("Content-Type", "application/json");
    
    let request = NanoRequest::new(
        "POST".to_string(),
        url,
        headers,
        Some(r#"{"name":"Test Item","value":42}"#.as_bytes().to_vec().into()),
    );
    
    let task = HandlerTask::new_with_request_id(
        "/app.js".to_string(),
        request,
        tx,
        hostname.to_string(),
        "req_crud_create_001".to_string(),
    );
    
    queue.dispatch(hostname, task).await.unwrap();
    let response = rx.await.unwrap().unwrap();
    
    assert_eq!(response.status(), 201, "CREATE should return 201 Created");
    
    let body = response.body().map(|b| String::from_utf8_lossy(b).to_string()).unwrap_or_default();
    assert!(body.contains("id"), "Response should contain created item with id");
    assert!(body.contains("Test Item"), "Response should contain item name");
    assert!(body.contains("42"), "Response should contain item value");
    
    println!("✅ CRUD CREATE test passed!");
}

/// Test READ operation - GET request retrieves resources
#[tokio::test]
async fn test_crud_read() {
    let _ = initialize_platform();
    
    let mut queue = WorkQueue::new(1);
    let hostname = "crud-read-test.local";
    
    let js_code = r#"
        const items = new Map([
            [1, { id: 1, name: 'Item 1', value: 100 }],
            [2, { id: 2, name: 'Item 2', value: 200 }]
        ]);
        
        export default {
            async fetch(request) {
                const url = new URL(request.url);
                const match = url.pathname.match(/\/items\/(\d+)/);
                
                if (request.method === 'GET' && url.pathname === '/items') {
                    // List all
                    const all = Array.from(items.values());
                    return Response.json(all);
                }
                
                if (request.method === 'GET' && match) {
                    // Get one
                    const id = parseInt(match[1]);
                    const item = items.get(id);
                    
                    if (item) {
                        return Response.json(item);
                    }
                    return new Response('Not Found', { status: 404 });
                }
                
                return new Response('Not Found', { status: 404 });
            }
        };
    "#;
    
    let vfs = IsolateVfs::new(
        VfsNamespace::from_hostname(hostname),
        VfsBackendEnum::memory(MemoryBackend::default()),
    );
    vfs.write("/app.js", js_code.as_bytes()).await.unwrap();

    // Test READ ALL
    let (tx, rx) = oneshot::channel();
    let url = NanoUrl::parse(&format!("http://{}/items", hostname)).unwrap();
    let request = NanoRequest::new(
        "GET".to_string(),
        url,
        NanoHeaders::new(),
        None,
    );
    
    let task = HandlerTask::new_with_request_id(
        "/app.js".to_string(),
        request,
        tx,
        hostname.to_string(),
        "req_crud_read_all".to_string(),
    );
    
    queue.dispatch(hostname, task).await.unwrap();
    let response = rx.await.unwrap().unwrap();
    
    assert_eq!(response.status(), 200);
    let body = response.body().map(|b| String::from_utf8_lossy(b).to_string()).unwrap_or_default();
    assert!(body.contains("Item 1"));
    assert!(body.contains("Item 2"));
    
    // Test READ ONE
    let (tx, rx) = oneshot::channel();
    let url = NanoUrl::parse(&format!("http://{}/items/1", hostname)).unwrap();
    let request = NanoRequest::new(
        "GET".to_string(),
        url,
        NanoHeaders::new(),
        None,
    );
    
    let task = HandlerTask::new_with_request_id(
        "/app.js".to_string(),
        request,
        tx,
        hostname.to_string(),
        "req_crud_read_one".to_string(),
    );
    
    queue.dispatch(hostname, task).await.unwrap();
    let response = rx.await.unwrap().unwrap();
    
    assert_eq!(response.status(), 200);
    let body = response.body().map(|b| String::from_utf8_lossy(b).to_string()).unwrap_or_default();
    assert!(body.contains("Item 1"));
    assert!(!body.contains("Item 2")); // Should only have one item
    
    println!("✅ CRUD READ test passed!");
}

/// Test UPDATE operation - PUT/PATCH modifies resources
#[tokio::test]
async fn test_crud_update() {
    let _ = initialize_platform();
    
    let mut queue = WorkQueue::new(1);
    let hostname = "crud-update-test.local";
    
    let js_code = r#"
        const items = new Map([
            [1, { id: 1, name: 'Original', value: 100 }]
        ]);
        
        export default {
            async fetch(request) {
                const url = new URL(request.url);
                const match = url.pathname.match(/\/items\/(\d+)/);
                
                if (request.method === 'PUT' && match) {
                    const id = parseInt(match[1]);
                    const existing = items.get(id);
                    
                    if (!existing) {
                        return new Response('Not Found', { status: 404 });
                    }
                    
                    const body = await request.json();
                    const updated = { ...existing, ...body, id, updated: Date.now() };
                    items.set(id, updated);
                    
                    return Response.json(updated);
                }
                
                return new Response('Not Found', { status: 404 });
            }
        };
    "#;

    let vfs = IsolateVfs::new(
        VfsNamespace::from_hostname(hostname),
        VfsBackendEnum::memory(MemoryBackend::default()),
    );
    vfs.write("/app.js", js_code.as_bytes()).await.unwrap();

    // Test UPDATE
    let (tx, rx) = oneshot::channel();
    let url = NanoUrl::parse(&format!("http://{}/items/1", hostname)).unwrap();
    let mut headers = NanoHeaders::new();
    headers.set("Content-Type", "application/json");
    
    let request = NanoRequest::new(
        "PUT".to_string(),
        url,
        headers,
        Some(r#"{"name":"Updated","value":999}"#.as_bytes().to_vec().into()),
    );
    
    let task = HandlerTask::new_with_request_id(
        "/app.js".to_string(),
        request,
        tx,
        hostname.to_string(),
        "req_crud_update".to_string(),
    );
    
    queue.dispatch(hostname, task).await.unwrap();
    let response = rx.await.unwrap().unwrap();
    
    assert_eq!(response.status(), 200);
    let body = response.body().map(|b| String::from_utf8_lossy(b).to_string()).unwrap_or_default();
    assert!(body.contains("Updated"), "Response should contain updated name");
    assert!(body.contains("999"), "Response should contain updated value");
    assert!(body.contains("updated"), "Response should have updated timestamp");
    
    println!("✅ CRUD UPDATE test passed!");
}

/// Test DELETE operation - DELETE removes resources
#[tokio::test]
async fn test_crud_delete() {
    let _ = initialize_platform();
    
    let mut queue = WorkQueue::new(1);
    let hostname = "crud-delete-test.local";
    
    let js_code = r#"
        const items = new Map([
            [1, { id: 1, name: 'To Delete' }]
        ]);
        
        export default {
            async fetch(request) {
                const url = new URL(request.url);
                const match = url.pathname.match(/\/items\/(\d+)/);
                
                if (request.method === 'DELETE' && match) {
                    const id = parseInt(match[1]);
                    
                    if (items.has(id)) {
                        items.delete(id);
                        return new Response(null, { status: 204 });
                    }
                    
                    return new Response('Not Found', { status: 404 });
                }
                
                if (request.method === 'GET' && url.pathname === '/items/count') {
                    return Response.json({ count: items.size });
                }
                
                return new Response('Not Found', { status: 404 });
            }
        };
    "#;

    let vfs = IsolateVfs::new(
        VfsNamespace::from_hostname(hostname),
        VfsBackendEnum::memory(MemoryBackend::default()),
    );
    vfs.write("/app.js", js_code.as_bytes()).await.unwrap();

    // First verify item exists (count = 1)
    let (tx, rx) = oneshot::channel();
    let url = NanoUrl::parse(&format!("http://{}/items/count", hostname)).unwrap();
    let request = NanoRequest::new(
        "GET".to_string(),
        url,
        NanoHeaders::new(),
        None,
    );
    
    let task = HandlerTask::new_with_request_id(
        "/app.js".to_string(),
        request,
        tx,
        hostname.to_string(),
        "req_crud_count_before".to_string(),
    );
    
    queue.dispatch(hostname, task).await.unwrap();
    let response = rx.await.unwrap().unwrap();
    let body = response.body().map(|b| String::from_utf8_lossy(b).to_string()).unwrap_or_default();
    assert!(body.contains("1"), "Should have 1 item before delete");
    
    // Test DELETE
    let (tx, rx) = oneshot::channel();
    let url = NanoUrl::parse(&format!("http://{}/items/1", hostname)).unwrap();
    let request = NanoRequest::new(
        "DELETE".to_string(),
        url,
        NanoHeaders::new(),
        None,
    );
    
    let task = HandlerTask::new_with_request_id(
        "/app.js".to_string(),
        request,
        tx,
        hostname.to_string(),
        "req_crud_delete".to_string(),
    );
    
    queue.dispatch(hostname, task).await.unwrap();
    let response = rx.await.unwrap().unwrap();
    
    assert_eq!(response.status(), 204, "DELETE should return 204 No Content");
    
    // Verify item deleted (count = 0)
    let (tx, rx) = oneshot::channel();
    let url = NanoUrl::parse(&format!("http://{}/items/count", hostname)).unwrap();
    let request = NanoRequest::new(
        "GET".to_string(),
        url,
        NanoHeaders::new(),
        None,
    );
    
    let task = HandlerTask::new_with_request_id(
        "/app.js".to_string(),
        request,
        tx,
        hostname.to_string(),
        "req_crud_count_after".to_string(),
    );
    
    queue.dispatch(hostname, task).await.unwrap();
    let response = rx.await.unwrap().unwrap();
    let body = response.body().map(|b| String::from_utf8_lossy(b).to_string()).unwrap_or_default();
    assert!(body.contains("0"), "Should have 0 items after delete");
    
    println!("✅ CRUD DELETE test passed!");
}

/// Test full CRUD cycle in one handler
#[tokio::test]
async fn test_crud_full_cycle() {
    let _ = initialize_platform();
    
    let mut queue = WorkQueue::new(1);
    let hostname = "crud-full-test.local";
    
    let js_code = r#"
        const items = new Map();
        let nextId = 1;
        
        export default {
            async fetch(request) {
                const url = new URL(request.url);
                
                // CREATE
                if (request.method === 'POST' && url.pathname === '/items') {
                    const body = await request.json();
                    const id = nextId++;
                    const item = { id, ...body, created: Date.now() };
                    items.set(id, item);
                    return new Response(JSON.stringify(item), {
                        status: 201,
                        headers: { 'Content-Type': 'application/json' }
                    });
                }
                
                // READ ALL
                if (request.method === 'GET' && url.pathname === '/items') {
                    return Response.json(Array.from(items.values()));
                }
                
                // READ/UPDATE/DELETE single
                const match = url.pathname.match(/\/items\/(\d+)/);
                if (match) {
                    const id = parseInt(match[1]);
                    
                    if (request.method === 'GET') {
                        const item = items.get(id);
                        return item ? Response.json(item) : new Response('Not Found', { status: 404 });
                    }
                    
                    if (request.method === 'PUT') {
                        const existing = items.get(id);
                        if (!existing) return new Response('Not Found', { status: 404 });
                        const body = await request.json();
                        const updated = { ...existing, ...body, id, updated: Date.now() };
                        items.set(id, updated);
                        return Response.json(updated);
                    }
                    
                    if (request.method === 'DELETE') {
                        if (items.has(id)) {
                            items.delete(id);
                            return new Response(null, { status: 204 });
                        }
                        return new Response('Not Found', { status: 404 });
                    }
                }
                
                return new Response('Not Found', { status: 404 });
            }
        };
    "#;

    let vfs = IsolateVfs::new(
        VfsNamespace::from_hostname(hostname),
        VfsBackendEnum::memory(MemoryBackend::default()),
    );
    vfs.write("/app.js", js_code.as_bytes()).await.unwrap();

    // Step 1: CREATE
    let (tx, rx) = oneshot::channel();
    let url = NanoUrl::parse(&format!("http://{}/items", hostname)).unwrap();
    let mut headers = NanoHeaders::new();
    headers.set("Content-Type", "application/json");
    
    let request = NanoRequest::new(
        "POST".to_string(),
        url,
        headers.clone(),
        Some(r#"{"name":"Test","value":42}"#.as_bytes().to_vec().into()),
    );
    
    let task = HandlerTask::new_with_request_id(
        "/app.js".to_string(),
        request,
        tx,
        hostname.to_string(),
        "req_crud_full_001".to_string(),
    );
    
    queue.dispatch(hostname, task).await.unwrap();
    let response = rx.await.unwrap().unwrap();
    assert_eq!(response.status(), 201);
    
    // Step 2: READ
    let (tx, rx) = oneshot::channel();
    let url = NanoUrl::parse(&format!("http://{}/items/1", hostname)).unwrap();
    let request = NanoRequest::new(
        "GET".to_string(),
        url,
        NanoHeaders::new(),
        None,
    );
    
    let task = HandlerTask::new_with_request_id(
        "/app.js".to_string(),
        request,
        tx,
        hostname.to_string(),
        "req_crud_full_002".to_string(),
    );
    
    queue.dispatch(hostname, task).await.unwrap();
    let response = rx.await.unwrap().unwrap();
    assert_eq!(response.status(), 200);
    
    // Step 3: UPDATE
    let (tx, rx) = oneshot::channel();
    let url = NanoUrl::parse(&format!("http://{}/items/1", hostname)).unwrap();
    let request = NanoRequest::new(
        "PUT".to_string(),
        url,
        headers.clone(),
        Some(r#"{"name":"Updated"}"#.as_bytes().to_vec().into()),
    );
    
    let task = HandlerTask::new_with_request_id(
        "/app.js".to_string(),
        request,
        tx,
        hostname.to_string(),
        "req_crud_full_003".to_string(),
    );
    
    queue.dispatch(hostname, task).await.unwrap();
    let response = rx.await.unwrap().unwrap();
    assert_eq!(response.status(), 200);
    let body = response.body().map(|b| String::from_utf8_lossy(b).to_string()).unwrap_or_default();
    assert!(body.contains("Updated"));
    
    // Step 4: DELETE
    let (tx, rx) = oneshot::channel();
    let url = NanoUrl::parse(&format!("http://{}/items/1", hostname)).unwrap();
    let request = NanoRequest::new(
        "DELETE".to_string(),
        url,
        NanoHeaders::new(),
        None,
    );
    
    let task = HandlerTask::new_with_request_id(
        "/app.js".to_string(),
        request,
        tx,
        hostname.to_string(),
        "req_crud_full_004".to_string(),
    );
    
    queue.dispatch(hostname, task).await.unwrap();
    let response = rx.await.unwrap().unwrap();
    assert_eq!(response.status(), 204);
    
    println!("✅ CRUD FULL CYCLE test passed!");
}
