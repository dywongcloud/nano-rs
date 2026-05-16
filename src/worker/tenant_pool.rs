//! Tenant-specific worker pool for multi-tenant isolate management
//!
//! This module implements Option 3 architecture: each tenant (hostname) gets
//! dedicated isolates that persist across requests. Contexts are never reset
//! within an isolate - instead, the entire isolate is recycled after a
//! configured number of requests or on OOM.
//!
//! Architecture:
//! ```
//! HTTP Request (hostname)
//!     ↓
//! WorkQueue routes by hostname
//!     ↓
//! TenantPool (dedicated to this hostname)
//!     ↓
//! Warm Isolate with Persistent Context
//!     ↓
//! Execute (NO context reset)
//! ```
//!
//! Benefits:
//! - Zero cold start latency after first request
//! - True tenant isolation (no shared workers)
//! - V8-compatible (no context reset issues)
//! - Matches Cloudflare Workers/Deno Deploy architecture

#[allow(unused_imports)]
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
#[allow(unused_imports)]
use std::sync::Arc;
use std::thread;
#[allow(unused_imports)]
use std::time::Duration;

use anyhow::{anyhow, Result};
use tracing::{error, info, warn};

use crate::control_plane::ControlPlane;
use crate::vfs::{IsolateVfs, VfsBackendEnum, VfsNamespace};
use crate::worker::context::ContextManager;
use crate::worker::eviction::{EvictionManager, IsolateMetadata};

use crate::worker::oom::OomMonitorBuilder;
use crate::worker::HandlerTask;
use crate::data_plane::{execute_with_context_manager, set_worker_runtime};
use crate::v8::NanoIsolate;

/// Maximum requests before recycling an isolate
const MAX_REQUESTS_PER_ISOLATE: u32 = 100;

/// Maximum idle time before recycling an isolate
const MAX_IDLE_SECONDS: u64 = 300;

/// A pool of isolates dedicated to a single tenant (hostname)
pub struct TenantPool {
    hostname: String,
    workers: Vec<TenantWorker>,
    next_worker: AtomicU64,
    vfs_backend: VfsBackendEnum,
    control_plane: Option<ControlPlane>,
}

struct TenantWorker {
    task_tx: mpsc::Sender<HandlerTask>,
    thread: Option<thread::JoinHandle<()>>,
}

impl TenantPool {
    /// Create a new tenant pool for the given hostname
    pub fn new(
        hostname: String,
        worker_count: u32,
        memory_limit_mb: u32,
        vfs_backend: VfsBackendEnum,
        control_plane: Option<ControlPlane>,
    ) -> Result<Self> {
        let mut workers = Vec::with_capacity(worker_count as usize);

        for id in 0..worker_count {
            let worker = Self::spawn_worker(
                id,
                hostname.clone(),
                memory_limit_mb,
                vfs_backend.clone(),
            )?;
            workers.push(worker);
        }

        info!(
            "Created tenant pool for '{}' with {} workers",
            hostname, worker_count
        );

        Ok(Self {
            hostname,
            workers,
            next_worker: AtomicU64::new(0),
            vfs_backend,
            control_plane,
        })
    }

    /// Spawn a worker thread with its own isolate
    fn spawn_worker(
        id: u32,
        hostname: String,
        memory_limit_mb: u32,
        vfs_backend: VfsBackendEnum,
    ) -> Result<TenantWorker> {
        let (task_tx, task_rx): (mpsc::Sender<HandlerTask>, mpsc::Receiver<HandlerTask>) =
            mpsc::channel();

        let thread = thread::spawn(move || {
            Self::worker_loop(id, hostname, memory_limit_mb, vfs_backend, task_rx);
        });

        Ok(TenantWorker {
            task_tx,
            thread: Some(thread),
        })
    }

    /// Worker event loop - owns isolate for this tenant
    fn worker_loop(
        id: u32,
        hostname: String,
        memory_limit_mb: u32,
        vfs_backend: VfsBackendEnum,
        task_rx: mpsc::Receiver<HandlerTask>,
    ) {
        info!("Tenant worker {} for '{}' starting", id, hostname);

        // Set up worker runtime for async operations
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                error!("Failed to create tokio runtime: {}", e);
                return;
            }
        };

        // Store runtime handle in thread-local for async operations
        let rt_handle = rt.handle().clone();
        set_worker_runtime(rt_handle);
        
        // Run the worker event loop
        Self::run_worker(id, hostname, memory_limit_mb, vfs_backend, task_rx);
    }

    fn run_worker(
        id: u32,
        hostname: String,
        memory_limit_mb: u32,
        vfs_backend: VfsBackendEnum,
        task_rx: mpsc::Receiver<HandlerTask>,
    ) {
        // Create OOM monitor
        let oom_monitor = if memory_limit_mb > 0 {
            Some(
                OomMonitorBuilder::new(format!("tenant_{}_{}", hostname, id))
                    .with_limit_mb(memory_limit_mb)
                    .for_hostname(&hostname)
                    .build(),
            )
        } else {
            None
        };

        // Create VFS for this worker
        let vfs = IsolateVfs::new(
            VfsNamespace::from_hostname(&hostname),
            vfs_backend,
        );

        // Create initial isolate and context (clone vfs for later recycling)
        let mut context_manager = Self::create_fresh_isolate(vfs.clone());
        let mut isolate_id = context_manager.isolate_id().clone();
        let mut request_count = 0u32;

        let mut eviction_manager = EvictionManager::new();
        eviction_manager.register_isolate(
            isolate_id.clone(),
            IsolateMetadata::new(&hostname, id),
        );

        info!(
            "Tenant worker {} for '{}' ready (isolate: {})",
            id, hostname, isolate_id
        );

        // Event loop
        loop {
            match task_rx.recv() {
                Ok(task) => {
                    // Check if isolate should be recycled
                    if request_count >= MAX_REQUESTS_PER_ISOLATE {
                        info!(
                            "Tenant worker {} recycling isolate after {} requests",
                            id, request_count
                        );
                        context_manager = Self::recycle_isolate(context_manager, vfs.clone(), id, &hostname);
                        isolate_id = context_manager.isolate_id().clone();
                        request_count = 0;
                    }

                    // Check OOM before request
                    if let Some(ref monitor) = oom_monitor {
                        if let Err(oom_error) = monitor.check(context_manager.isolate_mut().isolate()) {
                            warn!("OOM detected, recycling isolate");
                            monitor.log_oom_event(&oom_error, &task.request_id);
                            let _ = task.response_tx.send(Ok(monitor.create_oom_response(&oom_error)));
                            
                            context_manager = Self::recycle_isolate(context_manager, vfs.clone(), id, &hostname);
                            isolate_id = context_manager.isolate_id().clone();
                            request_count = 0;
                            continue;
                        }
                    }

                    // Execute request (NO context reset)
                    request_count += 1;
                    eviction_manager.mark_active(&isolate_id);

                    let handler_ctx = crate::runtime::HandlerContext {
                        entrypoint: task.entrypoint,
                        request: task.request,
                        memory_limit_mb: task.memory_limit_mb,
                        hostname: hostname.clone(),
                    };

                    let result = execute_with_context_manager(
                        &mut context_manager,
                        &handler_ctx,
                        task.cpu_time_limit_ms,
                    );

                    eviction_manager.mark_complete(&isolate_id);
                    let _ = task.response_tx.send(result);
                }
                Err(_) => {
                    info!("Tenant worker {} channel closed, exiting", id);
                    break;
                }
            }
        }
    }

    fn create_fresh_isolate(vfs: IsolateVfs) -> ContextManager {
        match NanoIsolate::new_with_vfs(vfs) {
            Ok(isolate) => {
                let mut manager = ContextManager::new(isolate);
                if let Err(e) = manager.create_initial_context() {
                    error!("Failed to create initial context: {}", e);
                    panic!("Cannot create isolate");
                }
                manager
            }
            Err(e) => {
                error!("Failed to create isolate: {}", e);
                panic!("Cannot create isolate");
            }
        }
    }

    fn recycle_isolate(
        old_manager: ContextManager,
        vfs: IsolateVfs,
        worker_id: u32,
        hostname: &str,
    ) -> ContextManager {
        // Drop old isolate (disposes entire isolate, not just context)
        drop(old_manager);
        info!("Tenant worker {} recycled isolate for '{}'", worker_id, hostname);
        
        // Create fresh isolate
        Self::create_fresh_isolate(vfs)
    }

    /// Dispatch a task to this tenant's pool
    pub fn dispatch(&self, task: HandlerTask) -> Result<()> {
        // Round-robin to next worker
        let worker_idx = (self.next_worker.fetch_add(1, Ordering::Relaxed) as usize)
            % self.workers.len();
        
        self.workers[worker_idx]
            .task_tx
            .send(task)
            .map_err(|_| anyhow!("Worker channel closed"))
    }

    /// Get number of workers in this pool
    pub fn worker_count(&self) -> usize {
        self.workers.len()
    }

    /// Get hostname this pool serves
    pub fn hostname(&self) -> &str {
        &self.hostname
    }
}

impl Drop for TenantPool {
    fn drop(&mut self) {
        info!("Dropping tenant pool for '{}'", self.hostname);
        // Channels are dropped, signaling workers to exit
        for worker in &mut self.workers {
            if let Some(thread) = worker.thread.take() {
                let _ = thread.join();
            }
        }
    }
}


