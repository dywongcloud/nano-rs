//! V8 argument extraction and value construction helpers for node_compat
//! host hooks.
//!
//! Conventions (CONTRACT.md §4): binary data crosses the boundary as
//! `Uint8Array`; failures throw JS `Error` objects carrying a Node-style
//! `.code` property.

/// Extract raw bytes from a callback argument accepting Uint8Array (or any
/// ArrayBufferView) and ArrayBuffer.
pub(super) fn bytes_arg(
    _scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: &v8::FunctionCallbackArguments,
    index: i32,
) -> Option<Vec<u8>> {
    if args.length() <= index {
        return None;
    }
    let arg = args.get(index);
    if let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(arg) {
        let len = view.byte_length();
        let mut buf = vec![0u8; len];
        view.copy_contents(&mut buf);
        return Some(buf);
    }
    if let Ok(ab) = v8::Local::<v8::ArrayBuffer>::try_from(arg) {
        let store = ab.get_backing_store();
        let len = ab.byte_length();
        let bytes: Vec<u8> = (0..len)
            .filter_map(|i| store.get(i).map(|cell| cell.get()))
            .collect();
        return Some(bytes);
    }
    None
}

/// Extract bytes like [`bytes_arg`], but treat `null`/`undefined`/missing as
/// an explicit "absent" (`Ok(None)`), and non-binary values as an error.
pub(super) fn opt_bytes_arg(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: &v8::FunctionCallbackArguments,
    index: i32,
) -> Result<Option<Vec<u8>>, ()> {
    if args.length() <= index {
        return Ok(None);
    }
    let arg = args.get(index);
    if arg.is_null_or_undefined() {
        return Ok(None);
    }
    match bytes_arg(scope, args, index) {
        Some(b) => Ok(Some(b)),
        None => Err(()),
    }
}

/// Extract a UTF-8 string argument.
pub(super) fn str_arg(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: &v8::FunctionCallbackArguments,
    index: i32,
) -> Option<String> {
    if args.length() <= index {
        return None;
    }
    let arg = args.get(index);
    if !arg.is_string() {
        return None;
    }
    arg.to_string(scope).map(|s| s.to_rust_string_lossy(scope))
}

/// Extract a finite number argument as f64.
pub(super) fn num_arg(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: &v8::FunctionCallbackArguments,
    index: i32,
) -> Option<f64> {
    if args.length() <= index {
        return None;
    }
    let v = args.get(index).to_number(scope)?.value();
    if v.is_finite() {
        Some(v)
    } else {
        None
    }
}

/// Extract a boolean argument (JS truthiness).
pub(super) fn bool_arg(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: &v8::FunctionCallbackArguments,
    index: i32,
) -> bool {
    if args.length() <= index {
        return false;
    }
    args.get(index).boolean_value(scope)
}

/// Build a `Uint8Array` from owned bytes (zero-copy backing store).
pub(super) fn make_uint8array<'s>(
    scope: &v8::PinScope<'s, '_>,
    bytes: Vec<u8>,
) -> v8::Local<'s, v8::Uint8Array> {
    let len = bytes.len();
    let store = v8::ArrayBuffer::new_backing_store_from_vec(bytes).make_shared();
    let ab = v8::ArrayBuffer::with_backing_store(scope, &store);
    v8::Uint8Array::new(scope, ab, 0, len)
        .unwrap_or_else(|| v8::Uint8Array::new(scope, v8::ArrayBuffer::new(scope, 0), 0, 0).unwrap())
}

/// Throw a JS Error carrying a Node-style `.code` (and optional
/// `syscall`/`path`) and return.
pub(super) fn throw_coded(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    code: &str,
    message: &str,
) {
    throw_coded_full(scope, code, message, None, None);
}

pub(super) fn throw_coded_full(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    code: &str,
    message: &str,
    syscall: Option<&str>,
    path: Option<&str>,
) {
    let msg = v8::String::new(scope, message)
        .unwrap_or_else(|| v8::String::empty(scope));
    let err = v8::Exception::error(scope, msg);
    if let Some(obj) = err.to_object(scope) {
        let code_key = v8::String::new(scope, "code").unwrap();
        let code_val = v8::String::new(scope, code).unwrap();
        obj.set(scope, code_key.into(), code_val.into());
        if let Some(sc) = syscall {
            let k = v8::String::new(scope, "syscall").unwrap();
            let v = v8::String::new(scope, sc).unwrap();
            obj.set(scope, k.into(), v.into());
        }
        if let Some(p) = path {
            let k = v8::String::new(scope, "path").unwrap();
            let v = v8::String::new(scope, p).unwrap();
            obj.set(scope, k.into(), v.into());
        }
        if let Some(errno) = errno_for_code(code) {
            let k = v8::String::new(scope, "errno").unwrap();
            let v = v8::Number::new(scope, errno as f64);
            obj.set(scope, k.into(), v.into());
        }
    }
    scope.throw_exception(err);
}

/// libuv-style negative errno values for the codes the host hooks emit.
fn errno_for_code(code: &str) -> Option<i32> {
    match code {
        "EPERM" => Some(-1),
        "ENOENT" => Some(-2),
        "EIO" => Some(-5),
        "EACCES" => Some(-13),
        "EEXIST" => Some(-17),
        "ENOTDIR" => Some(-20),
        "EISDIR" => Some(-21),
        "EINVAL" => Some(-22),
        "ENOSPC" => Some(-28),
        "ENOSYS" => Some(-38),
        "ENOTEMPTY" => Some(-39),
        "ENOTFOUND" => Some(-3008),
        _ => None,
    }
}

/// Throw ERR_INVALID_ARG_TYPE with a uniform message.
pub(super) fn throw_bad_args(scope: &mut v8::PinnedRef<v8::HandleScope>, hook: &str) {
    throw_coded(
        scope,
        "ERR_INVALID_ARG_TYPE",
        &format!("__nano_node_host.{}: invalid argument types", hook),
    );
}

/// Set a named function property on `obj`.
pub(super) fn set_fn(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    obj: v8::Local<v8::Object>,
    name: &str,
    cb: impl v8::MapFnTo<v8::FunctionCallback>,
) {
    if let Some(f) = v8::Function::new(scope, cb) {
        let key = v8::String::new(scope, name).unwrap();
        obj.set(scope, key.into(), f.into());
    }
}
