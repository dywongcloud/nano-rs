//! Process/os/dns host hooks for node:process, node:os, node:dns.
//!
//! Contract: CONTRACT.md §4 (process/os/dns sections).

use super::helpers::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::net::ToSocketAddrs;
use std::time::Instant;

thread_local! {
    /// Monotonic baseline for hrtime (per worker thread).
    static HRTIME_BASE: Instant = Instant::now();

    /// Per-app env vars for the current request (set alongside the VFS).
    static CURRENT_ENV: RefCell<Option<HashMap<String, String>>> = const { RefCell::new(None) };

    /// Tenant hostname for the current worker/request.
    static CURRENT_HOSTNAME: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Set the per-app environment variables visible as `process.env`.
pub fn set_current_env(env: Option<HashMap<String, String>>) {
    CURRENT_ENV.with(|cell| *cell.borrow_mut() = env);
}

/// Set the tenant hostname reported by `os.hostname()`.
pub fn set_current_hostname(hostname: Option<String>) {
    CURRENT_HOSTNAME.with(|cell| *cell.borrow_mut() = hostname);
}

lazy_static::lazy_static! {
    /// Registry of per-hostname env vars, populated once a tenant's `AppConfig`
    /// is resolved (see `WorkQueue::get_or_create_pool`) and consulted by each
    /// worker thread at isolate-creation time to seed `CURRENT_ENV` for that
    /// thread's requests.
    static ref HOSTNAME_ENV_VARS: dashmap::DashMap<String, HashMap<String, String>> = dashmap::DashMap::new();
}

/// Record the env vars configured for a hostname's app, for later pickup by
/// the worker thread(s) serving that hostname via `env_for_hostname`.
pub fn register_hostname_env(hostname: String, env: HashMap<String, String>) {
    HOSTNAME_ENV_VARS.insert(hostname, env);
}

/// Look up the env vars registered for a hostname, if any.
pub fn env_for_hostname(hostname: &str) -> Option<HashMap<String, String>> {
    HOSTNAME_ENV_VARS.get(hostname).map(|entry| entry.clone())
}

pub(super) fn bind(scope: &mut v8::PinnedRef<v8::HandleScope>, host: v8::Local<v8::Object>) {
    set_fn(scope, host, "hrtime", hrtime);
    set_fn(scope, host, "memoryUsage", memory_usage);
    set_fn(scope, host, "hostname", hostname);
    set_fn(scope, host, "availableParallelism", available_parallelism);
    set_fn(scope, host, "getEnv", get_env);
    set_fn(scope, host, "dnsLookup", dns_lookup);
}

fn hrtime(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    _args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let elapsed = HRTIME_BASE.with(|base| base.elapsed());
    let obj = v8::Object::new(scope);
    let sec_key = v8::String::new(scope, "sec").unwrap();
    let sec_val = v8::Number::new(scope, elapsed.as_secs() as f64);
    obj.set(scope, sec_key.into(), sec_val.into());
    let ns_key = v8::String::new(scope, "ns").unwrap();
    let ns_val = v8::Number::new(scope, elapsed.subsec_nanos() as f64);
    obj.set(scope, ns_key.into(), ns_val.into());
    retval.set(obj.into());
}

fn memory_usage(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    _args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let stats = scope.get_heap_statistics();
    let heap_total = stats.total_heap_size() as f64;
    let heap_used = stats.used_heap_size() as f64;
    let external = stats.external_memory() as f64;
    let obj = v8::Object::new(scope);
    let entries: [(&str, f64); 4] = [
        ("rss", heap_total + external),
        ("heapTotal", heap_total),
        ("heapUsed", heap_used),
        ("external", external),
    ];
    for (k, v) in entries {
        let key = v8::String::new(scope, k).unwrap();
        let val = v8::Number::new(scope, v);
        obj.set(scope, key.into(), val.into());
    }
    retval.set(obj.into());
}

fn hostname(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    _args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let name = CURRENT_HOSTNAME
        .with(|cell| cell.borrow().clone())
        .unwrap_or_else(|| "nano".to_string());
    let s = v8::String::new(scope, &name).unwrap_or_else(|| v8::String::empty(scope));
    retval.set(s.into());
}

fn available_parallelism(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    _args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let n = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    retval.set(v8::Number::new(scope, n as f64).into());
}

fn get_env(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    _args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let env = CURRENT_ENV.with(|cell| cell.borrow().clone()).unwrap_or_default();
    let obj = v8::Object::new(scope);
    for (k, v) in &env {
        let (Some(key), Some(val)) = (v8::String::new(scope, k), v8::String::new(scope, v)) else {
            continue;
        };
        obj.set(scope, key.into(), val.into());
    }
    if !env.contains_key("NODE_ENV") {
        let key = v8::String::new(scope, "NODE_ENV").unwrap();
        let val = v8::String::new(scope, "production").unwrap();
        obj.set(scope, key.into(), val.into());
    }
    retval.set(obj.into());
}

fn dns_lookup(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let Some(host) = str_arg(scope, &args, 0) else {
        return throw_bad_args(scope, "dnsLookup");
    };
    let family = num_arg(scope, &args, 1).unwrap_or(0.0) as u32;

    // Resolve via the platform resolver; port 0 is a placeholder.
    let addrs: Vec<(String, u32)> = match (host.as_str(), 0u16).to_socket_addrs() {
        Ok(iter) => iter
            .filter_map(|sa| match sa {
                std::net::SocketAddr::V4(a) if family == 0 || family == 4 => {
                    Some((a.ip().to_string(), 4))
                }
                std::net::SocketAddr::V6(a) if family == 0 || family == 6 => {
                    Some((a.ip().to_string(), 6))
                }
                _ => None,
            })
            .collect(),
        Err(_) => Vec::new(),
    };

    if addrs.is_empty() {
        return throw_coded_full(
            scope,
            "ENOTFOUND",
            &format!("getaddrinfo ENOTFOUND {}", host),
            Some("getaddrinfo"),
            None,
        );
    }

    let arr = v8::Array::new(scope, addrs.len() as i32);
    for (i, (address, fam)) in addrs.iter().enumerate() {
        let entry = v8::Object::new(scope);
        let addr_key = v8::String::new(scope, "address").unwrap();
        let addr_val = v8::String::new(scope, address).unwrap();
        entry.set(scope, addr_key.into(), addr_val.into());
        let fam_key = v8::String::new(scope, "family").unwrap();
        let fam_val = v8::Number::new(scope, *fam as f64);
        entry.set(scope, fam_key.into(), fam_val.into());
        arr.set_index(scope, i as u32, entry.into());
    }
    retval.set(arr.into());
}
