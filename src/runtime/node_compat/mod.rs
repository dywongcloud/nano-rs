//! Node.js compatibility layer.
//!
//! Architecture (see CONTRACT.md):
//! - Rust host hooks are exposed as `globalThis.__nano_node_host` — a flat
//!   object of synchronous functions (crypto, zlib, VFS fs, process/os/dns).
//! - The builtin modules themselves are JavaScript, embedded at compile time
//!   from `js/*.js`, registered through the loader in `js/00_prelude.js`,
//!   and instantiated lazily on first `require()`.
//! - `js/99_init.js` installs globals (process, Buffer, timers, web APIs)
//!   and the full `require` that supersedes the legacy fs-only one.
//!
//! Binding is idempotent per context: a marker global short-circuits
//! re-evaluation on contexts that are re-bound per request.

mod helpers;
mod hooks_crypto;
mod hooks_fs;
mod hooks_sys;
mod hooks_zlib;

pub use hooks_sys::{env_for_hostname, register_hostname_env, set_current_env, set_current_hostname};
pub(crate) use hooks_zlib::clear_streams as clear_zlib_streams;

/// Embedded JavaScript sources, evaluated in order. Registration order does
/// not matter (factories are lazy); `00_prelude` must come first and
/// `99_init` last.
const JS_SOURCES: &[(&str, &str)] = &[
    ("00_prelude.js", include_str!("js/00_prelude.js")),
    ("01_errors.js", include_str!("js/01_errors.js")),
    ("10_events.js", include_str!("js/10_events.js")),
    ("11_buffer.js", include_str!("js/11_buffer.js")),
    ("12_path.js", include_str!("js/12_path.js")),
    ("13_querystring.js", include_str!("js/13_querystring.js")),
    ("14_string_decoder.js", include_str!("js/14_string_decoder.js")),
    ("15_punycode.js", include_str!("js/15_punycode.js")),
    ("16_util.js", include_str!("js/16_util.js")),
    ("17_assert.js", include_str!("js/17_assert.js")),
    ("18_url.js", include_str!("js/18_url.js")),
    ("19_os.js", include_str!("js/19_os.js")),
    ("20_process.js", include_str!("js/20_process.js")),
    ("21_stream.js", include_str!("js/21_stream.js")),
    ("22_timers.js", include_str!("js/22_timers.js")),
    ("23_fs.js", include_str!("js/23_fs.js")),
    ("24_crypto.js", include_str!("js/24_crypto.js")),
    ("25_zlib.js", include_str!("js/25_zlib.js")),
    ("26_http.js", include_str!("js/26_http.js")),
    ("27_net.js", include_str!("js/27_net.js")),
    ("28_dns.js", include_str!("js/28_dns.js")),
    ("29_worker.js", include_str!("js/29_worker.js")),
    ("30_misc.js", include_str!("js/30_misc.js")),
    ("31_diag.js", include_str!("js/31_diag.js")),
    ("32_web.js", include_str!("js/32_web.js")),
    ("33_console.js", include_str!("js/33_console.js")),
    ("34_http2.js", include_str!("js/34_http2.js")),
    ("35_http_bridge.js", include_str!("js/35_http_bridge.js")),
    ("99_init.js", include_str!("js/99_init.js")),
];

/// Marker property that records a fully-initialized compat layer on a context.
const LOADED_MARKER: &str = "__nano_node_compat_loaded";

/// Bind the Node.js compatibility layer to a context.
///
/// Called from `RuntimeAPIs::bind_all` after all WinterTC APIs are bound and
/// before security hardening removes dynamic code generation.
pub fn bind_node_compat(
    scope: &mut v8::PinnedRef<v8::HandleScope<'_, ()>>,
    context: v8::Local<v8::Context>,
) {
    let global = context.global(scope);
    let mut ctx_scope = v8::ContextScope::new(scope, context);

    // Idempotence: skip when this context already carries the layer.
    if let Some(marker_key) = v8::String::new(&mut ctx_scope, LOADED_MARKER) {
        if let Some(existing) = global.get(&mut ctx_scope, marker_key.into()) {
            if existing.boolean_value(&mut ctx_scope) {
                return;
            }
        }
    }

    // Host hook object.
    let host = v8::Object::new(&mut ctx_scope);
    hooks_crypto::bind(&mut ctx_scope, host);
    hooks_zlib::bind(&mut ctx_scope, host);
    hooks_fs::bind(&mut ctx_scope, host);
    hooks_sys::bind(&mut ctx_scope, host);
    if let Some(host_key) = v8::String::new(&mut ctx_scope, "__nano_node_host") {
        global.set(&mut ctx_scope, host_key.into(), host.into());
    }

    // Evaluate the embedded JS layer.
    for (name, source) in JS_SOURCES {
        let Some(code) = v8::String::new(&mut ctx_scope, source) else {
            tracing::error!(file = name, "node_compat: failed to allocate source string");
            return;
        };
        let try_catch_storage = v8::TryCatch::new(&mut ctx_scope);
        let try_catch = std::pin::pin!(try_catch_storage);
        let mut try_catch = try_catch.init();
        let compiled = v8::Script::compile(&mut try_catch, code, None);
        let ran = compiled.and_then(|s| s.run(&mut try_catch));
        if ran.is_none() {
            let detail = try_catch
                .exception()
                .map(|e| e.to_rust_string_lossy(&mut try_catch))
                .unwrap_or_else(|| "unknown error".to_string());
            tracing::error!(file = name, error = %detail, "node_compat: JS layer evaluation failed");
            return;
        }
    }

    // Mark the context as initialized.
    if let Some(marker_key) = v8::String::new(&mut ctx_scope, LOADED_MARKER) {
        let true_val = v8::Boolean::new(&mut ctx_scope, true);
        global.set(&mut ctx_scope, marker_key.into(), true_val.into());
    }
}
