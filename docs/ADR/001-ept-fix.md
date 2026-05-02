# ADR-001: ExternalPointerTable (EPT) SIGSEGV Fix

**Status:** Accepted  
**Date:** 2026-04-19  
**Deciders:** Core Team  
**Technical Story:** EPT segment unmapping bug causes SIGSEGV on isolate teardown

---

## Context and Problem Statement

When creating and destroying V8 isolates rapidly (as in a multi-tenant edge runtime), we encountered random SIGSEGV crashes during isolate teardown. Investigation revealed this was caused by V8's ExternalPointerTable (EPT) segment unmapping bug, where the table's memory is freed while still being accessed.

The crash manifests as:
```
SIGSEGV at v8::internal::ExternalPointerTable::DeallocateEntries
```

This occurs because V8's ExternalPointerTable implementation has a race condition between:
1. Isolate teardown (which frees EPT memory)
2. Finalizer execution (which may still access EPT)

In a multi-tenant runtime creating/destroying isolates constantly, this race is frequently triggered.

---

## Decision Drivers

* **Stability** — Cannot have random crashes in production
* **V8 integration** — Must work with rusty_v8 crate (no V8 source modification)
* **Performance** — Fix must not add significant overhead
* **Safety** — Prefer Rust safety guarantees over C++ workarounds
* **Maintainability** — Solution must be understandable by future maintainers

---

## Considered Options

### Option 1: Strong Global Sentinel

Keep a `v8::Global<Value>` reference per isolate that prevents EPT cleanup until explicit drop.

### Option 2: Isolate Pool

Reuse isolates instead of creating/destroying. Doesn't fix root cause, just avoids trigger.

### Option 3: Delay Teardown

Add sleep/delay before isolate destruction. Hacky, unreliable, adds latency.

### Option 4: Patch rusty_v8

Modify V8 bindings to fix the bug. Requires maintaining a fork, upstream divergence.

---

## Decision Outcome

**Chosen option: "Strong Global Sentinel"**

We create a `v8::Global<Value>` at isolate creation and store it with the isolate. This global reference keeps the isolate's EPT segments alive until we explicitly drop it during controlled teardown, preventing the use-after-free that causes SIGSEGV.

**Rationale:**
- Fixes the crash at root cause (EPT lifecycle)
- No performance overhead (single pointer)
- Works with unmodified rusty_v8
- Clean, deterministic cleanup
- Well-understood mechanism (V8 Global handles)

---

## Implementation Details

### Code Location

`src/v8/platform.rs`

### Pattern

```rust
// Per-isolate sentinel to prevent EPT SIGSEGV
struct IsolateState {
    isolate: v8::OwnedIsolate,
    // This Global prevents EPT segment unmapping bug
    // See ADR-001 for full context
    sentinel: v8::Global<v8::Value>,
}
```

### Lifecycle

1. **Creation:**
   ```rust
   let isolate = v8::Isolate::new(params);
   let sentinel = v8::Global::new(&mut isolate, v8::undefined(&mut isolate));
   ```

2. **Normal Operation:**
   - Sentinel held alongside isolate
   - Reference count prevents EPT cleanup

3. **Controlled Teardown:**
   ```rust
   // Explicit drop order matters
   drop(sentinel);  // First: allow EPT cleanup
   drop(isolate);   // Second: destroy isolate
   ```

### Rationale for Global Type

We use `v8::Global<v8::Value>` (specifically `undefined`) because:
- Smallest/simplest JS value
- No external resources
- Just exists to hold the reference

---

## Positive Consequences

* **Eliminates random SIGSEGV crashes** — Root cause fixed
* **No performance overhead** — Single pointer storage
* **Works with unmodified rusty_v8** — No fork maintenance
* **Clean, deterministic cleanup** — Explicit drop order
* **Future-proof** — When V8 fixes the bug, we just remove the sentinel

---

## Negative Consequences

* **Slightly more complex isolate lifecycle** — Need to track sentinel
* **Must be documented** — Future maintainers need to understand why it's there
* **Technical debt** — Workaround for V8 bug (acceptance: waiting for upstream fix)
* **Slight memory overhead** — One Global handle per isolate (~8 bytes)

---

## Alternatives Rejected

### Option 2: Isolate Pool — Rejected

**Why:** Doesn't fix root cause, just avoids trigger. Crashes could still occur during pool resize or eviction. Also complicates memory management (when to recycle? how many to keep?).

### Option 3: Delay Teardown — Rejected

**Why:** Unreliable (race still possible), adds latency (even 1ms × thousands of isolates = seconds), magic number problem (how long to sleep?).

### Option 4: Patch rusty_v8 — Rejected

**Why:** Fork maintenance burden significant. Tracking upstream V8 updates requires constant merge effort. Prefer pure-Rust workaround.

---

## Related Decisions

* [ADR-002: Context Reset](002-context-reset.md) — Isolate lifecycle management
* [ADR-003: Thread-Local Isolates](003-thread-local-isolates.md) — When we create/destroy isolates

---

## Code References

- `src/v8/platform.rs` — EPT fix implementation
- `src/v8/isolate.rs` — Isolate lifecycle management
- `src/worker/pool.rs` — Worker pool isolate management

---

## Notes for Future Maintainers

**Why is there a random Global handle in isolate state?**

This sentinel prevents a V8 bug (ExternalPointerTable use-after-free). When V8 fixes the issue upstream, we can remove it. The explicit `drop(sentinel)` before `drop(isolate)` is load-bearing — changing the drop order will reintroduce the crash.

**Testing:**
- The bug manifests under high isolate churn (>100 isolates/second)
- Stress test: `cargo test --release test_isolate_churn`

---

*Last updated: 2026-04-19*
