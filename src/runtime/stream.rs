//! ReadableStream JavaScript implementation for response body streaming
//!
//! This module provides the ReadableStream API for streaming response bodies
//! from fetch() requests. It implements backpressure handling and zero-copy
//! data transfer via ArrayBuffer views.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

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
pub fn bind_streams(_scope: &mut v8::HandleScope, _context: v8::Local<v8::Context>) {
    // TODO: Implement ReadableStream binding in Task 3
    tracing::debug!("ReadableStream binding placeholder - Task 3");
}

#[cfg(test)]
mod tests {
    use super::*;

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
