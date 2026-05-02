# ADR-003: Thread-Local Isolate Ownership

**Status:** Accepted  
**Date:** 2026-04-19  
**Deciders:** Core Team  
**Technical Story:** V8 isolates are NOT thread-safe, need affine execution model

---

## Context and Problem Statement

V8 isolates (the execution context for JavaScript) are explicitly **NOT thread-safe**. An isolate can only be used from the thread that created it. Attempting to use an isolate from multiple threads simultaneously causes:

1. **Data races** — Concurrent heap access
2. **Undefined behavior** — V8 internal state corruption
3. **Crashes** — SIGSEGV, assertion failures

In a multi-threaded Rust server handling concurrent requests, we need a pattern to:
1. Handle concurrent requests across multiple threads
2. Respect V8's thread-safety constraints
3. Avoid cross-thread isolate migration (which causes crashes)
4. Scale with CPU cores

---

## Decision Drivers

* **Safety** — Cannot violate V8 thread requirements
* **Performance** — Minimize thread synchronization overhead
* **Simplicity** — Avoid complex ownership tracking
* **Throughput** — Scale with CPU cores
* **Cache locality** — Prefer same thread using same isolates

---

## Considered Options

### Option 1: Thread-Local Ownership

Each worker thread owns its isolates permanently.

### Option 2: Mutex Pool

Protect isolate access with locks (deadlock risk, contention).

### Option 3: Message Passing

Send isolate to thread, use, send back (complex lifecycle).

### Option 4: Single-Threaded

One thread for all JS (bottleneck on multi-core).

---

## Decision Outcome

**Chosen option: "Thread-Local Ownership"**

Each worker thread in the WorkerPool owns its isolates for the thread's entire lifetime. Request dispatch routes to specific workers (not free-for-all), ensuring affine execution:

```
Thread 1: [Isolate A] [Isolate B] — owns both permanently
Thread 2: [Isolate C] [Isolate D] — owns both permanently
Request with hash % 4 → routed to specific thread/isolate
```

**Rationale:**
- Zero synchronization overhead (no locks on hot path)
- Respects V8 thread-safety constraints absolutely
- Simple mental model: thread owns isolate
- Cache-friendly (worker always uses same isolates)

---

## Implementation Details

### Code Location

`src/worker/pool.rs`, `src/worker/queue.rs`

### Thread Model

```rust
// Each Worker thread has thread_local isolates
thread_local! {
    static ISOLATES: RefCell<Vec<IsolateHandle>> = RefCell::new(Vec::new());
}

pub struct Worker {
    id: usize,
    thread: Thread,
    queue: WorkQueue,
}

impl Worker {
    fn run(&self) {
        // Initialize isolates ONCE, on this thread only
        ISOLATES.with(|isolates| {
            *isolates.borrow_mut() = create_isolates_for_worker();
        });
        
        // Main loop
        while let Some(request) = self.queue.recv() {
            ISOLATES.with(|isolates| {
                let isolate = select_isolate(&isolates.borrow(), &request);
                isolate.handle(request);
            });
        }
    }
}
```

### Dispatch Strategy

Affine routing by hostname hash:

```rust
fn route_request(request: HttpRequest, workers: &[Worker]) -> &Worker {
    let hash = hash_hostname(&request.host_header);
    &workers[hash % workers.len()]
}
```

This ensures:
- Same hostname → Same worker → Same isolates
- Cache locality for repeated requests
- No cross-thread isolate migration

### Worker Pool Structure

```
┌─────────────────────────────────────────┐
│           Main Thread                   │
│  (TCP accept, HTTP parsing, routing)    │
└──────────────┬──────────────────────────┘
               │
    ┌──────────┼──────────┐
    │          │          │
    ▼          ▼          ▼
┌───────┐ ┌───────┐ ┌───────┐
│Worker0│ │Worker1│ │Worker2│
│Thread │ │Thread │ │Thread │
├───────┤ ├───────┤ ├───────┤
│Iso 0A │ │Iso 1A │ │Iso 2A │
│Iso 0B │ │Iso 1B │ │Iso 2B │
└───────┘ └───────┘ └───────┘
```

---

## Positive Consequences

* **Zero synchronization overhead** — No locks on hot path
* **Respects V8 thread-safety constraints** — Cannot violate even by accident
* **Simple mental model** — Thread owns isolate, period
* **Cache-friendly** — Worker always uses same isolates (CPU cache warm)
* **Deterministic routing** — Same hostname always same isolates
* **No deadlock risk** — No cross-thread locking

---

## Negative Consequences

* **Cannot dynamically balance load** — Stuck with initial routing
* **Worker with slow request blocks that thread's isolates** — Head-of-line blocking
* **Requires careful worker pool sizing** — Too few = contention, too many = memory
* **Cold start per worker** — Each worker needs its own isolates
* **Less flexible than free-for-all** — Can't move work to idle threads

---

## Alternatives Rejected

### Option 2: Mutex Pool — Rejected

**Why:** Deadlock risk (isolate A needs B while B locked), contention (locks on every request), violates "zero overhead" principle.

### Option 3: Message Passing — Rejected

**Why:** Complex lifecycle (send isolate, use, return), high overhead (context switches), V8 doesn't support isolate serialization.

### Option 4: Single-Threaded — Rejected

**Why:** Bottleneck on multi-core servers. Would limit to ~1 CPU core for all JS execution.

---

## Scaling Considerations

### Worker Pool Sizing

**Formula:**
```
workers = min(cpu_cores * 2, expected_max_concurrent_requests / 10)
```

**Examples:**
- 4-core server, 100 RPS: 4-8 workers
- 64-core server, 10,000 RPS: 32-64 workers

### Memory Implications

Each worker has `N` isolates (where N = isolates per worker). Memory scales as:
```
Total memory = (workers × isolates_per_worker × memory_per_isolate) + runtime_overhead
```

**Example:**
- 8 workers × 4 isolates × 128MB = 4GB for isolates
- Plus ~500MB runtime overhead

### Load Balancing Workaround

Since we can't dynamically rebalance, we mitigate:

1. **Request timeout** — Prevents one slow request blocking worker forever
2. **Worker pool sizing** — More workers = more capacity
3. **Context reset (not new isolate)** — Fast continuation after request
4. **Per-app worker pools** — One slow app doesn't affect others

---

## Related Decisions

* [ADR-001: EPT Fix](001-ept-fix.md) — Isolate lifecycle safety
* [ADR-002: Context Reset](002-context-reset.md) — What happens in the isolate
* `src/worker/pool.rs` — Implementation

---

## Code References

- `src/worker/pool.rs` — WorkerPool with thread-local isolates
- `src/worker/queue.rs` — WorkQueue with bounded MPSC channel
- `src/worker/context.rs` — Context lifecycle management

---

## Debugging Thread Issues

### Detecting Cross-Thread Isolate Use

Debug builds have assertions:
```rust
debug_assert!(
    isolate.thread_id() == current_thread_id(),
    "Isolate used from wrong thread!"
);
```

### Common Bug Patterns

**Anti-pattern 1: Moving isolate between threads**
```rust
// WRONG
let isolate = thread1.get_isolate();
thread2.spawn(move || {
    isolate.execute(...);  // CRASH
});
```

**Anti-pattern 2: Shared isolate with Arc**
```rust
// WRONG
let isolate = Arc::new(isolate);
thread1.spawn({
    let iso = isolate.clone();
    move || iso.execute(...)
});
thread2.spawn({
    let iso = isolate.clone();
    move || iso.execute(...)  // CRASH
});
```

**Correct pattern:** Thread-local storage + affine dispatch (as implemented).

---

*Last updated: 2026-04-19*
