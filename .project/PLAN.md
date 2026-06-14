# PLAN.md

## Now

**State:** Phases 1–5 complete. `cargo check` clean, 668 unit tests pass. Security review done; critical finding (eval bypass in standalone path) fixed and committed. All changes in single atomic commit `c2101cfa`.

**Next:** Nothing — milestone complete. Remaining security findings (SSRF, timing attack, default bind addr, escape_json) filed for future work.

**Open questions:** None.

## Roadmap

### Phase 1 — pool.rs deslop (WS duplicate blocks)

- [x] Extract OOM pre-check from Text + Binary WS arms → `ws_oom_break!($label:lifetime)` macro
- [x] Extract CPU timeout guard: already arm-local, no further dedup possible
- [x] Merge Text + Binary `WS_MESSAGE_HANDLERS` dispatch loop → `ws_dispatch!($handlers, $event)` macro
- [x] `ws_dispatch!` also applied to Close and Disconnected arms (WS_CLOSE_HANDLERS, WS_ERROR_HANDLERS)
- [x] Narrating one-liner comments stripped from both pool loops
- [x] Over-verbose doc comments on `with_source` / `with_source_and_backend` slimmed
- [x] `cargo check` clean

### Phase 2 — Rust code quality audit (ds-rust-review / ds-code-quality-review)

- [x] Run `ds-rust-review` across `src/worker/`, `src/runtime/`, `src/http/` — log findings
- [x] Fix: `src/runtime/fetch.rs` — SAFETY comments on 3 unsafe sites; &'static GC invariant documented
- [x] Fix: `src/runtime/websocket.rs` — cast safety comment
- [x] Fix: `src/worker/pool.rs` — SAFETY comments on 7 unsafe sites; ws_oom_break! now logs OOM events
- [x] Fix: `src/worker/tenant_pool.rs` — reviewed, no new issues (has SAFETY comments at all unsafe sites)
- [x] Fix: `src/http/router.rs` — cosmetic only (empty doc comment), no correctness issues
- [x] Fix: `src/runtime/async_support.rs` — dead suppression code removed, no issues
- [x] Fix: `src/sliver/mod.rs` — dead re-exports removed, no issues
- [x] Fix: `src/wasm/js_api.rs` — dead function removed, no issues

### Phase 3 — Bug review

- [x] Run `ds-bug-review` across WS paths (`pool.rs`, `tenant_pool.rs`, `websocket.rs`)
- [x] Fix: `tenant_pool.rs:952` — `dispatch_ws` always spawns dedicated worker per connection; prune dead handles via `is_finished()`
- [x] Fix: `websocket.rs:228` — `ws_close_callback` now guards on `WS_ACCEPTED` (matches `ws_send_callback`)
- [x] Fix: `tenant_pool.rs:469,514` — OOM events now logged in ws_messages loop (was `_oom`, silenced)
- [x] Confirm `clear_ws_thread_locals()` called on all WS exit paths including OOM-close
- [x] Confirm isolate recycles correctly after WS connection (D-10b path)

### Phase 4 — Security review

- [x] Run `ds-security-review` across `src/runtime/`, `src/http/`, `src/admin/`
- [x] Confirm `set_allow_generation_from_strings(false)` — MISSING in `handler.rs` standalone path (Fixed in Phase 5)
- [x] Confirm no user-controlled input reaches `v8::Script::compile` — entrypoints are admin-configured, safe

**Findings (Phase 4):**
- CRITICAL: eval/new Function() usable in standalone path — `handler.rs` contexts never call `set_allow_generation_from_strings(false)` (fixed)
- HIGH: SSRF — fetch.rs only blocks schemes; private IPs (10.x, 172.16.x, 192.168.x, 127.x, 169.254.x) reachable
- HIGH: Non-constant-time API key comparison in `auth.rs:72` — timing oracle on network-accessible port
- HIGH: Admin server default binds `0.0.0.0` — should default to `127.0.0.1`
- HIGH: `escape_json` misses U+0000–U+001F control chars — malformed JSON possible
- HARDENING: Empty `api_key` silently allows all requests through
- HARDENING: `create_unix_socket_router_no_auth` is public, unauthenticated — dead code landmine

### Phase 5 — Final cleanup

- [ ] Cargo check + clippy clean (no new warnings)
- [ ] Run existing test suite — all green
- [ ] Commit pool.rs deslop as isolated atomic commit
