# V8 v147 API Upgrade Status

## Summary

Core V8 infrastructure has been updated for v147 API compatibility. The upgrade is substantial, affecting 1600+ compilation errors across the codebase due to the fundamental API changes in V8 v147.

## Key V147 API Changes

### 1. HandleScope Creation (Major Change)
**OLD (v139):**
```rust
let scope = &mut v8::HandleScope::new(isolate);
```

**NEW (v147):**
```rust
let scope = std::pin::pin!(v8::HandleScope::new(isolate));
let scope = scope.init();
// scope is now PinnedRef<'_, HandleScope<'_, ()>>
```

### 2. ContextScope (Major Change)
**OLD (v139):**
```rust
ContextScope<'a, HandleScope<'a>>
```

**NEW (v147):**
```rust
ContextScope<'borrow, 'scope, P> where P: ClearCachedContext
// After entering context, P becomes HandleScope<'_, Context>
```

Key points:
- ContextScope has 2 lifetime parameters now
- Does NOT need init() - not address-sensitive
- Implements Deref/DerefMut to PinnedRef<HandleScope>

### 3. V8 API Calls
**OLD (v139):**
```rust
v8::String::new(scope, "hello")
```

**NEW (v147):**
```rust
// ContextScope derefs to PinnedRef, which derefs to HandleScope
v8::String::new(&*scope, "hello")  // or just &**scope
```

### 4. Callback Signatures
**OLD (v139):**
```rust
fn callback(scope: &mut v8::HandleScope, args: FunctionCallbackArguments, rv: ReturnValue)
```

**NEW (v147):**
```rust
fn callback<'s>(scope: &mut PinnedRef<'s, HandleScope<'s, ()>>, args: FunctionCallbackArguments<'s>, rv: ReturnValue<'s>)
```

## Files Updated

### Core V8 Module (Complete)
- ✅ `src/v8/abstractions.rs` - New file with v147 helper functions
- ✅ `src/v8/context.rs` - Updated create_context function
- ✅ `src/v8/isolate.rs` - Updated EPT sentinel, create_context, snapshot_creator
- ✅ `src/v8/script.rs` - Updated execute_script and console.log binding
- ✅ `src/v8/module.rs` - Updated ESM execution with ContextScope types

### Runtime (Partial)
- ⚠️ `src/runtime/async_support.rs` - Updated with generic parameters (needs testing)
- ⚠️ `src/runtime/handler.rs` - Partially updated (needs completion)

## Remaining Work

### High Priority (Blocks compilation)
1. **runtime/handler.rs** - Complete v147 API updates throughout the file
2. **All files using V8 APIs** - Update all remaining call sites to use `&*scope` pattern
3. **Callback functions** - Update all V8 callback signatures

### Medium Priority
1. **Test updates** - Update all test code using V8 APIs
2. **Integration testing** - Verify ESM module loading works
3. **Performance testing** - Ensure no regressions with new API

### Files Requiring Updates (Estimated 30+ files)
Based on compilation errors, the following areas need updates:
- `src/runtime/` - All handlers and execution code
- `src/worker/` - Worker pool and queue processing
- `src/http/` - Request/response handling
- `src/admin/` - Admin interface handlers
- Tests throughout codebase

## Migration Pattern

For each V8 API call site, apply this pattern:

```rust
// 1. Create HandleScope with pin! + init
let scope = std::pin::pin!(v8::HandleScope::new(isolate));
let mut scope = scope.init();

// 2. Create context
let context = v8::Context::new(&scope, Default::default());

// 3. Create ContextScope (no init needed)
let mut scope = v8::ContextScope::new(&mut scope, context);

// 4. Use &*scope or &**scope for V8 APIs depending on expected type
let str = v8::String::new(&*scope, "hello").unwrap();
```

## Type Changes Reference

| Component | v139 | v147 |
|-----------|------|------|
| HandleScope creation | `HandleScope::new(isolate)` | `pin!(HandleScope::new(isolate)).init()` |
| ContextScope | `ContextScope<'a, HandleScope<'a>>` | `ContextScope<'a, 'b, HandleScope<'b, Context>>` |
| Scope for APIs | `&mut HandleScope` | `&PinnedRef<'_, HandleScope<'_, Context>>` |
| Callback scope | `&mut HandleScope` | `&mut PinnedRef<'_, HandleScope<'_, C>>` |

## Testing Recommendations

1. Start with unit tests in `src/v8/` module
2. Test isolate creation and basic script execution
3. Test ESM module loading
4. Test async promise resolution
5. Full integration tests with Hono.js/Next.js

## References

- rusty_v147 source: `~/.cargo/registry/src/*/v8-147.4.0/src/scope.rs`
- V147 scope pattern docs in scope.rs lines 1-100
- AGENTS.md for EPT fix and architecture notes

## Commit

Core fixes committed as: `fix(v8): Update core V8 infrastructure for v147 API compatibility`
