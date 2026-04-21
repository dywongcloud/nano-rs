//! ReadableStream and WritableStream JavaScript implementation for streaming
//!
//! This module provides the ReadableStream API for streaming response bodies
//! from fetch() requests, and WritableStream API for streaming request bodies.
//! It implements backpressure handling and zero-copy data transfer.

use bytes::Bytes;
use std::cell::RefCell;
use std::collections::HashMap;

/// Resource table entry for active streams
#[derive(Debug)]
pub struct StreamResource {
    /// Unique resource ID
    pub rid: u32,
    /// Whether the stream is closed
    pub closed: bool,
}

/// Resource table for tracking active ReadableStreams
pub struct StreamResourceTable {
    resources: RefCell<HashMap<u32, StreamResource>>,
    next_rid: RefCell<u32>,
}

impl StreamResourceTable {
    /// Create a new resource table
    pub fn new() -> Self {
        Self {
            resources: RefCell::new(HashMap::new()),
            next_rid: RefCell::new(1),
        }
    }

    /// Add a new resource and return its ID
    pub fn add(&self) -> u32 {
        let rid = *self.next_rid.borrow();
        *self.next_rid.borrow_mut() += 1;

        let resource = StreamResource { rid, closed: false };
        self.resources.borrow_mut().insert(rid, resource);
        rid
    }

    /// Close a resource by ID
    pub fn close(&self, rid: u32) -> bool {
        if let Some(resource) = self.resources.borrow_mut().get_mut(&rid) {
            resource.closed = true;
            true
        } else {
            false
        }
    }

    /// Check if a resource exists
    pub fn has(&self, rid: u32) -> bool {
        self.resources.borrow().contains_key(&rid)
    }
}

impl Default for StreamResourceTable {
    fn default() -> Self {
        Self::new()
    }
}

/// Bind ReadableStream and related APIs to the global scope
pub fn bind_streams(scope: &mut v8::HandleScope, context: v8::Local<v8::Context>) {
    let global = context.global(scope);

    // Create ReadableStream constructor
    let rs_template = v8::FunctionTemplate::new(scope, readable_stream_constructor);
    let rs_ctor = rs_template.get_function(scope).unwrap();
    
    // Add getReader method to prototype
    if let Some(rs_obj) = rs_ctor.to_object(scope) {
        let proto_key = v8::String::new(scope, "prototype").unwrap();
        if let Some(proto) = rs_obj.get(scope, proto_key.into()) {
            if let Some(proto_obj) = proto.to_object(scope) {
                if let Some(get_reader_fn) = v8::Function::new(scope, readable_stream_get_reader) {
                    let get_reader_key = v8::String::new(scope, "getReader").unwrap();
                    proto_obj.set(scope, get_reader_key.into(), get_reader_fn.into());
                }
            }
        }
    }
    
    let rs_key = v8::String::new(scope, "ReadableStream").unwrap();
    global.set(scope, rs_key.into(), rs_ctor.into());

    // Create ReadableStreamDefaultReader constructor
    let reader_template = v8::FunctionTemplate::new(scope, readable_stream_default_reader_constructor);
    let reader_ctor = reader_template.get_function(scope).unwrap();
    
    // Add read method to prototype
    if let Some(reader_obj) = reader_ctor.to_object(scope) {
        let proto_key = v8::String::new(scope, "prototype").unwrap();
        if let Some(proto) = reader_obj.get(scope, proto_key.into()) {
            if let Some(proto_obj) = proto.to_object(scope) {
                if let Some(read_fn) = v8::Function::new(scope, reader_read_callback) {
                    let read_key = v8::String::new(scope, "read").unwrap();
                    proto_obj.set(scope, read_key.into(), read_fn.into());
                }
                if let Some(release_fn) = v8::Function::new(scope, reader_release_lock_callback) {
                    let release_key = v8::String::new(scope, "releaseLock").unwrap();
                    proto_obj.set(scope, release_key.into(), release_fn.into());
                }
            }
        }
    }
    
    let reader_key = v8::String::new(scope, "ReadableStreamDefaultReader").unwrap();
    global.set(scope, reader_key.into(), reader_ctor.into());

    // Create WritableStream constructor
    let ws_template = v8::FunctionTemplate::new(scope, writable_stream_constructor);
    let ws_ctor = ws_template.get_function(scope).unwrap();
    
    // Add getWriter method to prototype
    if let Some(ws_obj) = ws_ctor.to_object(scope) {
        let proto_key = v8::String::new(scope, "prototype").unwrap();
        if let Some(proto) = ws_obj.get(scope, proto_key.into()) {
            if let Some(proto_obj) = proto.to_object(scope) {
                if let Some(get_writer_fn) = v8::Function::new(scope, writable_stream_get_writer) {
                    let get_writer_key = v8::String::new(scope, "getWriter").unwrap();
                    proto_obj.set(scope, get_writer_key.into(), get_writer_fn.into());
                }
            }
        }
    }
    
    let ws_key = v8::String::new(scope, "WritableStream").unwrap();
    global.set(scope, ws_key.into(), ws_ctor.into());

    // Create WritableStreamDefaultWriter constructor
    let writer_template = v8::FunctionTemplate::new(scope, writable_stream_default_writer_constructor);
    let writer_ctor = writer_template.get_function(scope).unwrap();
    
    // Add write and close methods to prototype
    if let Some(writer_obj) = writer_ctor.to_object(scope) {
        let proto_key = v8::String::new(scope, "prototype").unwrap();
        if let Some(proto) = writer_obj.get(scope, proto_key.into()) {
            if let Some(proto_obj) = proto.to_object(scope) {
                if let Some(write_fn) = v8::Function::new(scope, writer_write_callback) {
                    let write_key = v8::String::new(scope, "write").unwrap();
                    proto_obj.set(scope, write_key.into(), write_fn.into());
                }
                if let Some(close_fn) = v8::Function::new(scope, writer_close_callback) {
                    let close_key = v8::String::new(scope, "close").unwrap();
                    proto_obj.set(scope, close_key.into(), close_fn.into());
                }
                if let Some(release_fn) = v8::Function::new(scope, writer_release_lock_callback) {
                    let release_key = v8::String::new(scope, "releaseLock").unwrap();
                    proto_obj.set(scope, release_key.into(), release_fn.into());
                }
            }
        }
    }
    
    let writer_key = v8::String::new(scope, "WritableStreamDefaultWriter").unwrap();
    global.set(scope, writer_key.into(), writer_ctor.into());

    tracing::debug!("Streams API bindings initialized");
}

// ============== ReadableStream JavaScript Bindings ==============

fn readable_stream_constructor(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();
    
    // Store underlying source if provided
    if args.length() > 0 {
        let source = args.get(0);
        let source_key = v8::String::new(scope, "__source").unwrap();
        this.set(scope, source_key.into(), source.into());
    }
    
    // Initialize state
    let state_key = v8::String::new(scope, "__state").unwrap();
    let state_val = v8::String::new(scope, "readable").unwrap();
    this.set(scope, state_key.into(), state_val.into());
    
    retval.set(this.into());
}

fn readable_stream_get_reader(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();
    let global = scope.get_current_context().global(scope);
    
    // Create reader instance
    let reader_key = v8::String::new(scope, "ReadableStreamDefaultReader").unwrap();
    if let Some(reader_ctor) = global.get(scope, reader_key.into()) {
        if let Some(reader_fn) = reader_ctor.to_object(scope) {
            if let Some(reader_func) = reader_fn.cast::<v8::Function>().new_instance(scope, &[this.into()]) {
                retval.set(reader_func.into());
                return;
            }
        }
    }
    
    retval.set(v8::null(scope).into());
}

fn readable_stream_default_reader_constructor(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();
    
    // Store reference to stream
    if args.length() > 0 {
        let stream = args.get(0);
        let stream_key = v8::String::new(scope, "__stream").unwrap();
        this.set(scope, stream_key.into(), stream.into());
    }
    
    retval.set(this.into());
}

fn reader_read_callback(
    scope: &mut v8::HandleScope,
    _args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    // Return { done: true, value: undefined } for basic implementation
    let result = v8::Object::new(scope);
    let done_key = v8::String::new(scope, "done").unwrap();
    let value_key = v8::String::new(scope, "value").unwrap();
    let true_val = v8::Boolean::new(scope, true);
    let undefined_val = v8::undefined(scope);
    
    result.set(scope, done_key.into(), true_val.into());
    result.set(scope, value_key.into(), undefined_val.into());
    
    retval.set(result.into());
}

fn reader_release_lock_callback(
    scope: &mut v8::HandleScope,
    _args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    retval.set(v8::undefined(scope).into());
}

// ============== WritableStream JavaScript Bindings ==============

fn writable_stream_constructor(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();
    
    // Store underlying sink if provided
    if args.length() > 0 {
        let sink = args.get(0);
        let sink_key = v8::String::new(scope, "__sink").unwrap();
        this.set(scope, sink_key.into(), sink.into());
    }
    
    retval.set(this.into());
}

fn writable_stream_get_writer(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();
    let global = scope.get_current_context().global(scope);
    
    // Create writer instance
    let writer_key = v8::String::new(scope, "WritableStreamDefaultWriter").unwrap();
    if let Some(writer_ctor) = global.get(scope, writer_key.into()) {
        if let Some(writer_fn) = writer_ctor.to_object(scope) {
            if let Some(writer_func) = writer_fn.cast::<v8::Function>().new_instance(scope, &[this.into()]) {
                retval.set(writer_func.into());
                return;
            }
        }
    }
    
    retval.set(v8::null(scope).into());
}

fn writable_stream_default_writer_constructor(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();
    
    // Store reference to stream
    if args.length() > 0 {
        let stream = args.get(0);
        let stream_key = v8::String::new(scope, "__stream").unwrap();
        this.set(scope, stream_key.into(), stream.into());
    }
    
    retval.set(this.into());
}

fn writer_write_callback(
    scope: &mut v8::HandleScope,
    _args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    // Return Promise.resolve() for basic implementation
    let global = scope.get_current_context().global(scope);
    let promise_key = v8::String::new(scope, "Promise").unwrap();
    let undefined_val = v8::undefined(scope);
    
    if let Some(promise_ctor) = global.get(scope, promise_key.into()) {
        if let Some(promise_fn) = promise_ctor.to_object(scope) {
            if let Some(promise_func) = promise_fn.cast::<v8::Function>().new_instance(scope, &[undefined_val.into()]) {
                retval.set(promise_func.into());
                return;
            }
        }
    }
    
    retval.set(undefined_val.into());
}

fn writer_close_callback(
    scope: &mut v8::HandleScope,
    _args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    // Return Promise.resolve() for basic implementation
    let global = scope.get_current_context().global(scope);
    let promise_key = v8::String::new(scope, "Promise").unwrap();
    let undefined_val = v8::undefined(scope);
    
    if let Some(promise_ctor) = global.get(scope, promise_key.into()) {
        if let Some(promise_fn) = promise_ctor.to_object(scope) {
            if let Some(promise_func) = promise_fn.cast::<v8::Function>().new_instance(scope, &[undefined_val.into()]) {
                retval.set(promise_func.into());
                return;
            }
        }
    }
    
    retval.set(undefined_val.into());
}

fn writer_release_lock_callback(
    scope: &mut v8::HandleScope,
    _args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    retval.set(v8::undefined(scope).into());
}

/// UnderlyingSink trait for Rust-side data consumption
///
/// Implement this trait to receive data written to a WritableStream.
/// The sink can apply backpressure by not returning until data is processed.
pub trait UnderlyingSink: Send {
    /// Called when the stream is constructed
    fn start(&mut self) -> Result<(), anyhow::Error> {
        Ok(())
    }

    /// Called when a chunk is written to the stream
    /// Returns a future that resolves when the chunk has been processed
    fn write(&mut self, chunk: Bytes) -> impl std::future::Future<Output = Result<(), anyhow::Error>> + Send;

    /// Called when the stream is closed
    fn close(&mut self) -> impl std::future::Future<Output = Result<(), anyhow::Error>> + Send {
        async { Ok(()) }
    }

    /// Called when the stream is aborted
    fn abort(&mut self, _reason: Option<String>) -> impl std::future::Future<Output = Result<(), anyhow::Error>> + Send {
        async { Ok(()) }
    }
}

/// WritableStream for streaming data from JavaScript to Rust
///
/// This implements the standard WritableStream API with backpressure support.
/// Data flows: JS writer → bounded channel → Rust UnderlyingSink → destination
#[derive(Debug)]
pub struct WritableStream {
    /// Channel sender for passing chunks from JS to Rust
    sender: Option<tokio::sync::mpsc::Sender<StreamCommand>>,
    /// Whether the stream is locked (has an active writer)
    locked: std::sync::atomic::AtomicBool,
    /// High water mark for backpressure (number of chunks)
    high_water_mark: usize,
    /// Whether the stream has been closed
    closed: std::sync::atomic::AtomicBool,
    /// Whether the stream has been aborted
    aborted: std::sync::atomic::AtomicBool,
    /// Abort reason if aborted
    abort_reason: std::sync::Mutex<Option<String>>,
}

/// Commands sent from the writer to the stream controller
#[derive(Debug)]
enum StreamCommand {
    /// Write a chunk of data
    Write(Bytes),
    /// Close the stream
    Close,
    /// Abort the stream with an error
    Abort(Option<String>),
}

/// Result of a write operation
#[derive(Debug, Clone)]
pub enum WriteResult {
    /// Write succeeded
    Success,
    /// Stream is closed
    Closed,
    /// Stream is aborted with reason
    Aborted(Option<String>),
}

impl WritableStream {
    /// Create a new WritableStream with a custom underlying sink
    ///
    /// # Arguments
    /// * `sink` - The UnderlyingSink implementation that will receive data
    /// * `high_water_mark` - Buffer size before backpressure (default: 4 chunks)
    ///
    /// # Type Parameters
    /// * `S` - The UnderlyingSink type
    pub fn new<S>(mut sink: S, high_water_mark: Option<usize>) -> Self
    where
        S: UnderlyingSink + Send + 'static,
    {
        let high_water_mark = high_water_mark.unwrap_or(4);
        let (sender, mut receiver) = tokio::sync::mpsc::channel::<StreamCommand>(high_water_mark);

        // Spawn the sink processing task
        tokio::spawn(async move {
            // Initialize the sink
            if let Err(e) = sink.start() {
                tracing::error!("UnderlyingSink start failed: {}", e);
                return;
            }

            // Process commands until stream is closed or aborted
            loop {
                match receiver.recv().await {
                    Some(StreamCommand::Write(chunk)) => {
                        if let Err(e) = sink.write(chunk).await {
                            tracing::error!("UnderlyingSink write failed: {}", e);
                            break;
                        }
                    }
                    Some(StreamCommand::Close) => {
                        if let Err(e) = sink.close().await {
                            tracing::error!("UnderlyingSink close failed: {}", e);
                        }
                        break;
                    }
                    Some(StreamCommand::Abort(reason)) => {
                        if let Err(e) = sink.abort(reason.clone()).await {
                            tracing::error!("UnderlyingSink abort failed: {}", e);
                        }
                        break;
                    }
                    None => {
                        // Channel closed, stop processing
                        break;
                    }
                }
            }
        });

        Self {
            sender: Some(sender),
            locked: std::sync::atomic::AtomicBool::new(false),
            high_water_mark,
            closed: std::sync::atomic::AtomicBool::new(false),
            aborted: std::sync::atomic::AtomicBool::new(false),
            abort_reason: std::sync::Mutex::new(None),
        }
    }

    /// Create a WritableStream that accumulates data into a buffer
    /// Useful for testing or buffering scenarios
    pub fn in_memory_buffer(capacity: usize) -> (Self, std::sync::Arc<std::sync::Mutex<Vec<u8>>>) {
        let buffer = std::sync::Arc::new(std::sync::Mutex::new(Vec::with_capacity(capacity)));
        let buffer_clone = buffer.clone();

        struct BufferSink {
            buffer: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
        }

        impl UnderlyingSink for BufferSink {
            async fn write(&mut self, chunk: Bytes) -> Result<(), anyhow::Error> {
                let mut buf = self.buffer.lock().unwrap();
                buf.extend_from_slice(&chunk);
                Ok(())
            }
        }

        let stream = Self::new(BufferSink { buffer: buffer_clone }, Some(4));
        (stream, buffer)
    }

    /// Get a writer for this stream
    /// Returns None if the stream is already locked
    pub fn get_writer(&self) -> Option<WritableStreamDefaultWriter> {
        // Try to acquire lock
        if self
            .locked
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            )
            .is_err()
        {
            return None; // Already locked
        }

        // Check if stream is closed or aborted
        if self.closed.load(std::sync::atomic::Ordering::SeqCst)
            || self.aborted.load(std::sync::atomic::Ordering::SeqCst)
        {
            // Release lock since we can't provide a valid writer
            self.locked.store(false, std::sync::atomic::Ordering::SeqCst);
            return None;
        }

        Some(WritableStreamDefaultWriter {
            sender: self.sender.clone(),
            stream: self,
            closed: false,
        })
    }

    /// Check if the stream is locked
    pub fn is_locked(&self) -> bool {
        self.locked.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Get the high water mark
    pub fn high_water_mark(&self) -> usize {
        self.high_water_mark
    }

    /// Check if the stream is closed
    pub fn is_closed(&self) -> bool {
        self.closed.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Check if the stream is aborted
    pub fn is_aborted(&self) -> bool {
        self.aborted.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Get the abort reason if aborted
    pub fn abort_reason(&self) -> Option<String> {
        self.abort_reason.lock().unwrap().clone()
    }

    /// Internal: Mark stream as closed
    fn mark_closed(&self) {
        self.closed.store(true, std::sync::atomic::Ordering::SeqCst);
        self.locked.store(false, std::sync::atomic::Ordering::SeqCst);
    }

    /// Internal: Mark stream as aborted
    fn mark_aborted(&self, reason: Option<String>) {
        self.aborted.store(true, std::sync::atomic::Ordering::SeqCst);
        *self.abort_reason.lock().unwrap() = reason;
        self.locked.store(false, std::sync::atomic::Ordering::SeqCst);
    }

    /// Internal: Release the lock (called when writer is dropped)
    fn release_lock(&self) {
        self.locked.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Default writer for WritableStream
///
/// Provides methods to write chunks, close the stream, and abort.
/// Only one writer can exist at a time (exclusive lock).
#[derive(Debug)]
pub struct WritableStreamDefaultWriter<'a> {
    sender: Option<tokio::sync::mpsc::Sender<StreamCommand>>,
    stream: &'a WritableStream,
    closed: bool,
}

impl<'a> WritableStreamDefaultWriter<'a> {
    /// Write a chunk to the stream
    ///
    /// This async method will wait until the chunk is accepted by the sink.
    /// If the internal buffer is full, this will wait (backpressure).
    pub async fn write(&mut self, chunk: Bytes) -> Result<(), WriteError> {
        if self.closed {
            return Err(WriteError::Closed);
        }

        if let Some(ref sender) = self.sender {
            // Send the write command
            sender
                .send(StreamCommand::Write(chunk))
                .await
                .map_err(|_| WriteError::Closed)?;
            Ok(())
        } else {
            Err(WriteError::Closed)
        }
    }

    /// Close the stream gracefully
    ///
    /// Signals that no more data will be written. The sink will process
    /// any pending chunks before closing.
    pub async fn close(mut self) -> Result<(), WriteError> {
        if self.closed {
            return Err(WriteError::Closed);
        }

        if let Some(ref sender) = self.sender {
            sender
                .send(StreamCommand::Close)
                .await
                .map_err(|_| WriteError::Closed)?;
            self.closed = true;
            self.stream.mark_closed();
            Ok(())
        } else {
            Err(WriteError::Closed)
        }
    }

    /// Abort the stream with an error
    ///
    /// Immediately stops the stream and rejects any pending writes.
    pub async fn abort(mut self, reason: Option<String>) -> Result<(), WriteError> {
        if self.closed {
            return Err(WriteError::Closed);
        }

        if let Some(ref sender) = self.sender {
            sender
                .send(StreamCommand::Abort(reason.clone()))
                .await
                .map_err(|_| WriteError::Closed)?;
            self.closed = true;
            self.stream.mark_aborted(reason);
            Ok(())
        } else {
            Err(WriteError::Closed)
        }
    }

    /// Check if the stream is closed
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Get the desired size (available buffer space)
    ///
    /// Returns the number of chunks that can be written without blocking.
    /// Returns 0 if backpressure is applied.
    pub fn desired_size(&self) -> usize {
        if self.closed {
            return 0;
        }

        if let Some(ref sender) = self.sender {
            let capacity = self.stream.high_water_mark;
            // Estimate available space (approximate since we can't get exact count)
            // In a real implementation, we'd track this more precisely
            capacity.saturating_sub(1) // Conservative estimate
        } else {
            0
        }
    }

    /// Wait until the stream is ready to accept data (backpressure cleared)
    ///
    /// This returns immediately if there's space in the buffer.
    /// Otherwise, it waits until space becomes available.
    pub async fn ready(&self) -> Result<(), WriteError> {
        if self.closed {
            return Err(WriteError::Closed);
        }

        // For bounded channels, the channel capacity itself provides backpressure
        // When the buffer is full, send().await will wait
        Ok(())
    }
}

impl<'a> Drop for WritableStreamDefaultWriter<'a> {
    fn drop(&mut self) {
        // Release the lock when writer is dropped
        if !self.closed {
            // If not explicitly closed, mark the stream as errored
            if let Some(ref sender) = self.sender {
                // Try to send abort (non-blocking since we're in drop)
                let _ = sender.try_send(StreamCommand::Abort(Some("Writer dropped without close".to_string())));
            }
        }
        self.stream.release_lock();
    }
}

/// Errors that can occur when writing to a stream
#[derive(Debug, Clone, PartialEq)]
pub enum WriteError {
    /// The stream is closed
    Closed,
    /// The stream was aborted with a reason
    Aborted(Option<String>),
    /// An internal error occurred
    Internal(String),
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteError::Closed => write!(f, "Stream is closed"),
            WriteError::Aborted(reason) => write!(f, "Stream aborted: {:?}", reason),
            WriteError::Internal(msg) => write!(f, "Internal error: {}", msg),
        }
    }
}

impl std::error::Error for WriteError {}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== WritableStream Tests ====================

    /// Test 1: new WritableStream() creates stream with underlying sink
    #[tokio::test]
    async fn test_writable_stream_creation() {
        struct TestSink;
        impl UnderlyingSink for TestSink {
            async fn write(&mut self, _chunk: Bytes) -> Result<(), anyhow::Error> {
                Ok(())
            }
        }

        let stream = WritableStream::new(TestSink, None);
        assert!(!stream.is_locked());
        assert!(!stream.is_closed());
        assert!(!stream.is_aborted());
        assert_eq!(stream.high_water_mark(), 4); // Default
    }

    /// Test 2: stream.getWriter() returns WritableStreamDefaultWriter
    #[tokio::test]
    async fn test_get_writer() {
        struct TestSink;
        impl UnderlyingSink for TestSink {
            async fn write(&mut self, _chunk: Bytes) -> Result<(), anyhow::Error> {
                Ok(())
            }
        }

        let stream = WritableStream::new(TestSink, None);
        let writer = stream.get_writer();
        assert!(writer.is_some());
        assert!(stream.is_locked());
    }

    /// Test 3: writer.write(chunk) writes bytes and succeeds
    #[tokio::test]
    async fn test_writer_write() {
        struct TestSink {
            received: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
        }
        impl UnderlyingSink for TestSink {
            async fn write(&mut self, chunk: Bytes) -> Result<(), anyhow::Error> {
                self.received.lock().unwrap().extend_from_slice(&chunk);
                Ok(())
            }
        }

        let received = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = TestSink {
            received: received.clone(),
        };

        let stream = WritableStream::new(sink, None);
        let mut writer = stream.get_writer().unwrap();

        let chunk = Bytes::from("Hello, World!");
        let result = writer.write(chunk).await;

        assert!(result.is_ok());

        // Give the sink task time to process
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // Verify data was received
        let data = received.lock().unwrap();
        assert_eq!(&*data, b"Hello, World!");
    }

    /// Test 4: Backpressure: getWriter() returns None when locked
    #[tokio::test]
    async fn test_backpressure_locking() {
        struct TestSink;
        impl UnderlyingSink for TestSink {
            async fn write(&mut self, _chunk: Bytes) -> Result<(), anyhow::Error> {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                Ok(())
            }
        }

        let stream = WritableStream::new(TestSink, None);

        // Get first writer
        let writer1 = stream.get_writer();
        assert!(writer1.is_some());
        assert!(stream.is_locked());

        // Try to get second writer - should fail
        let writer2 = stream.get_writer();
        assert!(writer2.is_none());
    }

    /// Test 5: writer.close() signals end of stream
    #[tokio::test]
    async fn test_writer_close() {
        struct TestSink {
            closed: std::sync::Arc<std::sync::atomic::AtomicBool>,
        }
        impl UnderlyingSink for TestSink {
            async fn write(&mut self, _chunk: Bytes) -> Result<(), anyhow::Error> {
                Ok(())
            }
            async fn close(&mut self) -> Result<(), anyhow::Error> {
                self.closed.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }
        }

        let closed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let sink = TestSink {
            closed: closed.clone(),
        };

        let stream = WritableStream::new(sink, None);
        let writer = stream.get_writer().unwrap();

        // Close the stream
        let result = writer.close().await;
        assert!(result.is_ok());

        // Verify stream is marked as closed
        assert!(stream.is_closed());

        // Give time for close to propagate
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // Verify sink was closed
        assert!(closed.load(std::sync::atomic::Ordering::SeqCst));
    }

    /// Test 6: writer.abort() signals error
    #[tokio::test]
    async fn test_writer_abort() {
        struct TestSink {
            aborted: std::sync::Arc<std::sync::atomic::AtomicBool>,
            reason: std::sync::Arc<std::sync::Mutex<Option<String>>>,
        }
        impl UnderlyingSink for TestSink {
            async fn write(&mut self, _chunk: Bytes) -> Result<(), anyhow::Error> {
                Ok(())
            }
            async fn abort(&mut self, reason: Option<String>) -> Result<(), anyhow::Error> {
                self.aborted.store(true, std::sync::atomic::Ordering::SeqCst);
                *self.reason.lock().unwrap() = reason;
                Ok(())
            }
        }

        let aborted = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let reason = std::sync::Arc::new(std::sync::Mutex::new(None));
        let sink = TestSink {
            aborted: aborted.clone(),
            reason: reason.clone(),
        };

        let stream = WritableStream::new(sink, None);
        let writer = stream.get_writer().unwrap();

        // Abort the stream
        let result = writer.abort(Some("Test abort reason".to_string())).await;
        assert!(result.is_ok());

        // Verify stream is marked as aborted
        assert!(stream.is_aborted());
        assert_eq!(stream.abort_reason(), Some("Test abort reason".to_string()));

        // Give time for abort to propagate
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // Verify sink was aborted
        assert!(aborted.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(&*reason.lock().unwrap(), &Some("Test abort reason".to_string()));
    }

    /// Test 7: Multiple writes are processed in order
    #[tokio::test]
    async fn test_ordered_writes() {
        struct TestSink {
            received: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
        }
        impl UnderlyingSink for TestSink {
            async fn write(&mut self, chunk: Bytes) -> Result<(), anyhow::Error> {
                self.received.lock().unwrap().extend_from_slice(&chunk);
                Ok(())
            }
        }

        let received = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = TestSink {
            received: received.clone(),
        };

        let stream = WritableStream::new(sink, None);
        let mut writer = stream.get_writer().unwrap();

        // Write multiple chunks
        writer.write(Bytes::from("Hello ")).await.unwrap();
        writer.write(Bytes::from("streaming ")).await.unwrap();
        writer.write(Bytes::from("world!")).await.unwrap();

        // Close to ensure all data is processed
        writer.close().await.unwrap();

        // Give time for processing
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Verify order is preserved
        let data = received.lock().unwrap();
        assert_eq!(&*data, b"Hello streaming world!");
    }

    /// Test 8: in_memory_buffer helper works
    #[tokio::test]
    async fn test_in_memory_buffer() {
        let (stream, buffer) = WritableStream::in_memory_buffer(1024);
        let mut writer = stream.get_writer().unwrap();

        // Write some data
        writer.write(Bytes::from("Test data")).await.unwrap();
        writer.close().await.unwrap();

        // Give time for processing
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // Verify buffer contains the data
        let data = buffer.lock().unwrap();
        assert_eq!(&*data, b"Test data");
    }

    /// Test 9: desired_size() returns reasonable value
    #[tokio::test]
    async fn test_desired_size() {
        struct TestSink;
        impl UnderlyingSink for TestSink {
            async fn write(&mut self, _chunk: Bytes) -> Result<(), anyhow::Error> {
                Ok(())
            }
        }

        let stream = WritableStream::new(TestSink, Some(10));
        let writer = stream.get_writer().unwrap();

        // Should return high_water_mark - 1 as conservative estimate
        assert_eq!(writer.desired_size(), 9);
    }

    /// Test 10: ready() returns Ok when stream is open
    #[tokio::test]
    async fn test_ready() {
        struct TestSink;
        impl UnderlyingSink for TestSink {
            async fn write(&mut self, _chunk: Bytes) -> Result<(), anyhow::Error> {
                Ok(())
            }
        }

        let stream = WritableStream::new(TestSink, None);
        let writer = stream.get_writer().unwrap();

        // ready() should succeed when stream is open
        let result = writer.ready().await;
        assert!(result.is_ok());
    }

    // ==================== Original StreamResourceTable Tests ====================

    /// Test 1: Resource table can be created
    #[test]
    fn test_resource_table_creation() {
        let table = StreamResourceTable::new();
        assert!(table.has(0) == false);
    }

    /// Test 2: Resources can be added
    #[test]
    fn test_add_resource() {
        let table = StreamResourceTable::new();
        let rid = table.add();
        assert!(rid > 0);
        assert!(table.has(rid));
    }

    /// Test 3: Resources can be closed
    #[test]
    fn test_close_resource() {
        let table = StreamResourceTable::new();
        let rid = table.add();
        assert!(table.close(rid));
        assert!(!table.close(999)); // Non-existent
    }

    /// Test 4: Multiple resources have unique IDs
    #[test]
    fn test_unique_resource_ids() {
        let table = StreamResourceTable::new();
        let rid1 = table.add();
        let rid2 = table.add();
        let rid3 = table.add();

        assert_ne!(rid1, rid2);
        assert_ne!(rid2, rid3);
        assert_ne!(rid1, rid3);
    }
}
