//! VFS-backed synchronous fs host hooks for node:fs.
//!
//! Contract: CONTRACT.md §4 (fs section). The VFS is a flat namespace
//! (object-store semantics). Directories are represented implicitly by
//! path prefixes; explicitly created empty directories persist via a
//! `.__nano_dir__` marker entry which is hidden from listings.

use super::helpers::*;
use crate::vfs::{IsolateVfs, VfsError};

const DIR_MARKER: &str = ".__nano_dir__";

/// Run an async VFS op synchronously (same pattern as fs_polyfill).
fn block_on<F, T>(fut: F) -> T
where
    F: std::future::Future<Output = T>,
{
    match tokio::runtime::Handle::try_current() {
        Ok(rt) => rt.block_on(fut),
        Err(_) => pollster::block_on(fut),
    }
}

fn with_vfs<F, R>(f: F) -> Result<R, VfsError>
where
    F: FnOnce(&IsolateVfs) -> Result<R, VfsError>,
{
    crate::runtime::vfs_bindings::with_current_vfs(|vfs| match vfs {
        Some(v) => f(v),
        None => Err(VfsError::IoError("No VFS available in this context".to_string())),
    })
}

fn normalize(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    format!("/{}", out.join("/"))
}

fn throw_vfs_error(scope: &mut v8::PinnedRef<v8::HandleScope>, err: &VfsError, syscall: &str, path: &str) {
    let code = match err {
        VfsError::NotFound { .. } => "ENOENT",
        VfsError::AlreadyExists { .. } => "EEXIST",
        VfsError::InvalidPath { .. } => "EINVAL",
        VfsError::PermissionDenied { .. } => "EACCES",
        VfsError::QuotaExceeded { .. } => "ENOSPC",
        _ => "EIO",
    };
    throw_coded_full(scope, code, &format!("{}: {} '{}'", code, syscall, path), Some(syscall), Some(path));
}

fn throw_uv(scope: &mut v8::PinnedRef<v8::HandleScope>, code: &str, syscall: &str, path: &str) {
    throw_coded_full(scope, code, &format!("{}: {} '{}'", code, syscall, path), Some(syscall), Some(path));
}

pub(super) fn bind(scope: &mut v8::PinnedRef<v8::HandleScope>, host: v8::Local<v8::Object>) {
    set_fn(scope, host, "fsReadFile", fs_read_file);
    set_fn(scope, host, "fsWriteFile", fs_write_file);
    set_fn(scope, host, "fsExists", fs_exists);
    set_fn(scope, host, "fsUnlink", fs_unlink);
    set_fn(scope, host, "fsMkdir", fs_mkdir);
    set_fn(scope, host, "fsRmdir", fs_rmdir);
    set_fn(scope, host, "fsReaddir", fs_readdir);
    set_fn(scope, host, "fsStat", fs_stat);
    set_fn(scope, host, "fsRename", fs_rename);
    set_fn(scope, host, "fsCopyFile", fs_copy_file);
}

fn file_exists(vfs: &IsolateVfs, path: &str) -> bool {
    block_on(vfs.exists(path)).unwrap_or(false)
}

/// List immediate children of `path` (marker entries hidden).
fn list_children(vfs: &IsolateVfs, path: &str) -> Result<Vec<String>, VfsError> {
    let entries = block_on(vfs.list_dir(path))?;
    let mut names: Vec<String> = entries
        .iter()
        .filter_map(|p| {
            let s = p.as_str();
            let name = s.rsplit('/').next().unwrap_or(s);
            if name == DIR_MARKER || name.is_empty() {
                None
            } else {
                Some(name.to_string())
            }
        })
        .collect();
    names.sort();
    names.dedup();
    Ok(names)
}

fn dir_exists(vfs: &IsolateVfs, path: &str) -> bool {
    if path == "/" {
        return true;
    }
    if file_exists(vfs, &format!("{}/{}", path, DIR_MARKER)) {
        return true;
    }
    matches!(list_children(vfs, path), Ok(names) if !names.is_empty())
}

fn fs_read_file(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let Some(path) = str_arg(scope, &args, 0).map(|p| normalize(&p)) else {
        return throw_bad_args(scope, "fsReadFile");
    };
    match with_vfs(|vfs| block_on(vfs.read(&path))) {
        Ok(bytes) => retval.set(make_uint8array(scope, bytes).into()),
        Err(e) => throw_vfs_error(scope, &e, "open", &path),
    }
}

fn fs_write_file(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    _retval: v8::ReturnValue,
) {
    let (Some(path), Some(data)) = (
        str_arg(scope, &args, 0).map(|p| normalize(&p)),
        bytes_arg(scope, &args, 1),
    ) else {
        return throw_bad_args(scope, "fsWriteFile");
    };
    let is_dir = match with_vfs(|vfs| Ok(dir_exists(vfs, &path))) {
        Ok(d) => d,
        Err(e) => return throw_vfs_error(scope, &e, "open", &path),
    };
    if is_dir {
        return throw_uv(scope, "EISDIR", "open", &path);
    }
    if let Err(e) = with_vfs(|vfs| block_on(vfs.write(&path, &data))) {
        throw_vfs_error(scope, &e, "open", &path);
    }
}

fn fs_exists(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let Some(path) = str_arg(scope, &args, 0).map(|p| normalize(&p)) else {
        return throw_bad_args(scope, "fsExists");
    };
    let exists = with_vfs(|vfs| Ok(file_exists(vfs, &path) || dir_exists(vfs, &path))).unwrap_or(false);
    retval.set(v8::Boolean::new(scope, exists).into());
}

fn fs_unlink(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    _retval: v8::ReturnValue,
) {
    let Some(path) = str_arg(scope, &args, 0).map(|p| normalize(&p)) else {
        return throw_bad_args(scope, "fsUnlink");
    };
    enum UnlinkFail {
        IsDir,
        Vfs(VfsError),
    }
    let result = with_vfs(|vfs| {
        if !file_exists(vfs, &path) {
            if dir_exists(vfs, &path) {
                return Ok(Err(UnlinkFail::IsDir));
            }
            return Ok(Err(UnlinkFail::Vfs(VfsError::NotFound { path: path.clone() })));
        }
        Ok(block_on(vfs.delete(&path)).map_err(UnlinkFail::Vfs))
    });
    match result {
        Ok(Ok(())) => {}
        Ok(Err(UnlinkFail::IsDir)) => throw_uv(scope, "EISDIR", "unlink", &path),
        Ok(Err(UnlinkFail::Vfs(e))) | Err(e) => throw_vfs_error(scope, &e, "unlink", &path),
    }
}

fn fs_mkdir(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    _retval: v8::ReturnValue,
) {
    let Some(path) = str_arg(scope, &args, 0).map(|p| normalize(&p)) else {
        return throw_bad_args(scope, "fsMkdir");
    };
    let recursive = bool_arg(scope, &args, 1);

    enum MkdirFail {
        Exists,
        NoParent,
        NotDirParent,
        Vfs(VfsError),
    }
    let result = with_vfs(|vfs| {
        if file_exists(vfs, &path) {
            return Ok(Err(MkdirFail::Exists));
        }
        if dir_exists(vfs, &path) {
            return Ok(if recursive { Ok(()) } else { Err(MkdirFail::Exists) });
        }
        if !recursive {
            let parent = match path.rfind('/') {
                Some(0) => "/".to_string(),
                Some(i) => path[..i].to_string(),
                None => "/".to_string(),
            };
            if file_exists(vfs, &parent) {
                return Ok(Err(MkdirFail::NotDirParent));
            }
            if !dir_exists(vfs, &parent) {
                return Ok(Err(MkdirFail::NoParent));
            }
        }
        match block_on(vfs.write(&format!("{}/{}", path, DIR_MARKER), b"")) {
            Ok(()) => Ok(Ok(())),
            Err(e) => Ok(Err(MkdirFail::Vfs(e))),
        }
    });
    match result {
        Ok(Ok(())) => {}
        Ok(Err(MkdirFail::Exists)) => throw_uv(scope, "EEXIST", "mkdir", &path),
        Ok(Err(MkdirFail::NoParent)) => throw_uv(scope, "ENOENT", "mkdir", &path),
        Ok(Err(MkdirFail::NotDirParent)) => throw_uv(scope, "ENOTDIR", "mkdir", &path),
        Ok(Err(MkdirFail::Vfs(e))) | Err(e) => throw_vfs_error(scope, &e, "mkdir", &path),
    }
}

fn fs_rmdir(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    _retval: v8::ReturnValue,
) {
    let Some(path) = str_arg(scope, &args, 0).map(|p| normalize(&p)) else {
        return throw_bad_args(scope, "fsRmdir");
    };

    enum RmdirFail {
        NotFound,
        NotDir,
        NotEmpty,
        Vfs(VfsError),
    }
    let result = with_vfs(|vfs| {
        if file_exists(vfs, &path) {
            return Ok(Err(RmdirFail::NotDir));
        }
        if !dir_exists(vfs, &path) {
            return Ok(Err(RmdirFail::NotFound));
        }
        match list_children(vfs, &path) {
            Ok(names) if !names.is_empty() => Ok(Err(RmdirFail::NotEmpty)),
            Ok(_) => {
                let marker = format!("{}/{}", path, DIR_MARKER);
                if file_exists(vfs, &marker) {
                    match block_on(vfs.delete(&marker)) {
                        Ok(()) => Ok(Ok(())),
                        Err(e) => Ok(Err(RmdirFail::Vfs(e))),
                    }
                } else {
                    Ok(Ok(()))
                }
            }
            Err(e) => Ok(Err(RmdirFail::Vfs(e))),
        }
    });
    match result {
        Ok(Ok(())) => {}
        Ok(Err(RmdirFail::NotFound)) => throw_uv(scope, "ENOENT", "rmdir", &path),
        Ok(Err(RmdirFail::NotDir)) => throw_uv(scope, "ENOTDIR", "rmdir", &path),
        Ok(Err(RmdirFail::NotEmpty)) => throw_uv(scope, "ENOTEMPTY", "rmdir", &path),
        Ok(Err(RmdirFail::Vfs(e))) | Err(e) => throw_vfs_error(scope, &e, "rmdir", &path),
    }
}

fn fs_readdir(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let Some(path) = str_arg(scope, &args, 0).map(|p| normalize(&p)) else {
        return throw_bad_args(scope, "fsReaddir");
    };

    enum ReaddirFail {
        NotFound,
        NotDir,
        Vfs(VfsError),
    }
    let result = with_vfs(|vfs| {
        if file_exists(vfs, &path) {
            return Ok(Err(ReaddirFail::NotDir));
        }
        if !dir_exists(vfs, &path) {
            return Ok(Err(ReaddirFail::NotFound));
        }
        match list_children(vfs, &path) {
            Ok(names) => Ok(Ok(names)),
            Err(e) => Ok(Err(ReaddirFail::Vfs(e))),
        }
    });
    match result {
        Ok(Ok(names)) => {
            let arr = v8::Array::new(scope, names.len() as i32);
            for (i, name) in names.iter().enumerate() {
                let s = v8::String::new(scope, name).unwrap();
                arr.set_index(scope, i as u32, s.into());
            }
            retval.set(arr.into());
        }
        Ok(Err(ReaddirFail::NotFound)) => throw_uv(scope, "ENOENT", "scandir", &path),
        Ok(Err(ReaddirFail::NotDir)) => throw_uv(scope, "ENOTDIR", "scandir", &path),
        Ok(Err(ReaddirFail::Vfs(e))) | Err(e) => throw_vfs_error(scope, &e, "scandir", &path),
    }
}

fn fs_stat(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let Some(path) = str_arg(scope, &args, 0).map(|p| normalize(&p)) else {
        return throw_bad_args(scope, "fsStat");
    };

    enum StatResult {
        File { size: f64, mtime_ms: f64, birthtime_ms: f64 },
        Dir,
        NotFound,
        Vfs(VfsError),
    }
    let result = with_vfs(|vfs| {
        if file_exists(vfs, &path) {
            return Ok(match block_on(vfs.metadata(&path)) {
                Ok(meta) => {
                    let mtime_ms = meta
                        .modified_at
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as f64)
                        .unwrap_or(0.0);
                    let birthtime_ms = meta
                        .created_at
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as f64)
                        .unwrap_or(0.0);
                    StatResult::File { size: meta.size as f64, mtime_ms, birthtime_ms }
                }
                Err(e) => StatResult::Vfs(e),
            });
        }
        if dir_exists(vfs, &path) {
            return Ok(StatResult::Dir);
        }
        Ok(StatResult::NotFound)
    });
    match result {
        Ok(StatResult::File { size, mtime_ms, birthtime_ms }) => {
            retval.set(stat_object(scope, size, mtime_ms, birthtime_ms, true).into());
        }
        Ok(StatResult::Dir) => {
            retval.set(stat_object(scope, 0.0, 0.0, 0.0, false).into());
        }
        Ok(StatResult::NotFound) => throw_uv(scope, "ENOENT", "stat", &path),
        Ok(StatResult::Vfs(e)) | Err(e) => throw_vfs_error(scope, &e, "stat", &path),
    }
}

fn stat_object<'s>(
    scope: &v8::PinScope<'s, '_>,
    size: f64,
    mtime_ms: f64,
    birthtime_ms: f64,
    is_file: bool,
) -> v8::Local<'s, v8::Object> {
    let obj = v8::Object::new(scope);
    let entries: [(&str, f64); 3] = [("size", size), ("mtimeMs", mtime_ms), ("birthtimeMs", birthtime_ms)];
    for (k, v) in entries {
        let key = v8::String::new(scope, k).unwrap();
        let val = v8::Number::new(scope, v);
        obj.set(scope, key.into(), val.into());
    }
    let k_file = v8::String::new(scope, "isFile").unwrap();
    let v_file = v8::Boolean::new(scope, is_file);
    obj.set(scope, k_file.into(), v_file.into());
    let k_dir = v8::String::new(scope, "isDirectory").unwrap();
    let v_dir = v8::Boolean::new(scope, !is_file);
    obj.set(scope, k_dir.into(), v_dir.into());
    obj
}

fn fs_rename(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    _retval: v8::ReturnValue,
) {
    let (Some(from), Some(to)) = (
        str_arg(scope, &args, 0).map(|p| normalize(&p)),
        str_arg(scope, &args, 1).map(|p| normalize(&p)),
    ) else {
        return throw_bad_args(scope, "fsRename");
    };
    if let Err(e) = with_vfs(|vfs| {
        let data = block_on(vfs.read(&from))?;
        block_on(vfs.write(&to, &data))?;
        block_on(vfs.delete(&from))
    }) {
        throw_vfs_error(scope, &e, "rename", &from);
    }
}

fn fs_copy_file(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    _retval: v8::ReturnValue,
) {
    let (Some(from), Some(to)) = (
        str_arg(scope, &args, 0).map(|p| normalize(&p)),
        str_arg(scope, &args, 1).map(|p| normalize(&p)),
    ) else {
        return throw_bad_args(scope, "fsCopyFile");
    };
    if let Err(e) = with_vfs(|vfs| {
        let data = block_on(vfs.read(&from))?;
        block_on(vfs.write(&to, &data))
    }) {
        throw_vfs_error(scope, &e, "copyfile", &from);
    }
}
