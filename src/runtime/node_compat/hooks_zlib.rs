//! zlib host hooks: one-shot and incremental gzip/deflate/brotli codecs.
//!
//! Contract: CONTRACT.md §4 (zlib section). Streaming handles are
//! thread-local (isolates are pinned to worker threads) and freed on
//! `finish` or via `zlibFree`.

use super::helpers::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write;

use flate2::write::{DeflateDecoder, DeflateEncoder, GzDecoder, GzEncoder, ZlibDecoder, ZlibEncoder};
use flate2::Compression;

enum ZStream {
    GzEnc(GzEncoder<Vec<u8>>),
    GzDec(GzDecoder<Vec<u8>>),
    ZlibEnc(ZlibEncoder<Vec<u8>>),
    ZlibDec(ZlibDecoder<Vec<u8>>),
    RawEnc(DeflateEncoder<Vec<u8>>),
    RawDec(DeflateDecoder<Vec<u8>>),
    BrotliEnc(brotli::CompressorWriter<Vec<u8>>),
    BrotliDec(brotli::DecompressorWriter<Vec<u8>>),
    /// "unzip": auto-detect gzip vs zlib once ≥2 header bytes are buffered.
    UnzipPending(Vec<u8>),
}

thread_local! {
    static ZSTREAMS: RefCell<HashMap<u32, ZStream>> = RefCell::new(HashMap::new());
    static ZNEXT: std::cell::Cell<u32> = const { std::cell::Cell::new(1) };
}

pub(super) fn bind(scope: &mut v8::PinnedRef<v8::HandleScope>, host: v8::Local<v8::Object>) {
    set_fn(scope, host, "zlibSync", zlib_sync);
    set_fn(scope, host, "zlibCreate", zlib_create);
    set_fn(scope, host, "zlibPush", zlib_push);
    set_fn(scope, host, "zlibFree", zlib_free);
}

fn compression(level: f64) -> Compression {
    if (0.0..=9.0).contains(&level) {
        Compression::new(level as u32)
    } else {
        Compression::default()
    }
}

fn one_shot(kind: &str, data: &[u8], level: f64) -> Result<Vec<u8>, String> {
    fn run<W: Write>(mut w: W, data: &[u8]) -> std::io::Result<W> {
        w.write_all(data)?;
        Ok(w)
    }
    match kind {
        "gzip" => run(GzEncoder::new(Vec::new(), compression(level)), data)
            .and_then(|e| e.finish())
            .map_err(|e| e.to_string()),
        "gunzip" => run(GzDecoder::new(Vec::new()), data)
            .and_then(|d| d.finish())
            .map_err(|e| e.to_string()),
        "deflate" => run(ZlibEncoder::new(Vec::new(), compression(level)), data)
            .and_then(|e| e.finish())
            .map_err(|e| e.to_string()),
        "inflate" => run(ZlibDecoder::new(Vec::new()), data)
            .and_then(|d| d.finish())
            .map_err(|e| e.to_string()),
        "deflateRaw" => run(DeflateEncoder::new(Vec::new(), compression(level)), data)
            .and_then(|e| e.finish())
            .map_err(|e| e.to_string()),
        "inflateRaw" => run(DeflateDecoder::new(Vec::new()), data)
            .and_then(|d| d.finish())
            .map_err(|e| e.to_string()),
        "unzip" => {
            if data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b {
                one_shot("gunzip", data, level)
            } else {
                one_shot("inflate", data, level)
            }
        }
        "brotliCompress" => {
            let mut out = Vec::new();
            let mut reader = std::io::Cursor::new(data);
            let params = brotli::enc::BrotliEncoderParams::default();
            brotli::BrotliCompress(&mut reader, &mut out, &params).map_err(|e| e.to_string())?;
            Ok(out)
        }
        "brotliDecompress" => {
            let mut out = Vec::new();
            let mut reader = std::io::Cursor::new(data);
            brotli::BrotliDecompress(&mut reader, &mut out).map_err(|e| e.to_string())?;
            Ok(out)
        }
        _ => Err(format!("unknown zlib kind: {}", kind)),
    }
}

fn zlib_sync(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let (Some(kind), Some(data)) = (str_arg(scope, &args, 0), bytes_arg(scope, &args, 1)) else {
        return throw_bad_args(scope, "zlibSync");
    };
    let level = num_arg(scope, &args, 2).unwrap_or(-1.0);
    match one_shot(&kind, &data, level) {
        Ok(out) => retval.set(make_uint8array(scope, out).into()),
        Err(e) => throw_coded(scope, "Z_DATA_ERROR", &format!("zlib {}: {}", kind, e)),
    }
}

fn new_stream(kind: &str, level: f64) -> Result<ZStream, String> {
    Ok(match kind {
        "gzip" => ZStream::GzEnc(GzEncoder::new(Vec::new(), compression(level))),
        "gunzip" => ZStream::GzDec(GzDecoder::new(Vec::new())),
        "deflate" => ZStream::ZlibEnc(ZlibEncoder::new(Vec::new(), compression(level))),
        "inflate" => ZStream::ZlibDec(ZlibDecoder::new(Vec::new())),
        "deflateRaw" => ZStream::RawEnc(DeflateEncoder::new(Vec::new(), compression(level))),
        "inflateRaw" => ZStream::RawDec(DeflateDecoder::new(Vec::new())),
        "brotliCompress" => ZStream::BrotliEnc(brotli::CompressorWriter::new(Vec::new(), 4096, 5, 22)),
        "brotliDecompress" => ZStream::BrotliDec(brotli::DecompressorWriter::new(Vec::new(), 4096)),
        "unzip" => ZStream::UnzipPending(Vec::new()),
        _ => return Err(format!("unknown zlib kind: {}", kind)),
    })
}

fn zlib_create(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let Some(kind) = str_arg(scope, &args, 0) else {
        return throw_bad_args(scope, "zlibCreate");
    };
    let level = num_arg(scope, &args, 1).unwrap_or(-1.0);
    match new_stream(&kind, level) {
        Ok(stream) => {
            let id = ZNEXT.with(|c| {
                let id = c.get();
                c.set(id.wrapping_add(1).max(1));
                id
            });
            ZSTREAMS.with(|m| m.borrow_mut().insert(id, stream));
            retval.set(v8::Number::new(scope, id as f64).into());
        }
        Err(e) => throw_coded(scope, "ERR_ZLIB_INITIALIZATION_FAILED", &e),
    }
}

/// Write a chunk into a stream, returning output produced so far. On
/// `finish`, the codec is finalized and the handle removed.
fn push_chunk(stream: &mut ZStream, chunk: &[u8]) -> std::io::Result<Vec<u8>> {
    macro_rules! drain {
        ($enc:expr) => {{
            $enc.write_all(chunk)?;
            $enc.flush()?;
            Ok(std::mem::take($enc.get_mut()))
        }};
    }
    match stream {
        ZStream::GzEnc(e) => drain!(e),
        ZStream::GzDec(e) => drain!(e),
        ZStream::ZlibEnc(e) => drain!(e),
        ZStream::ZlibDec(e) => drain!(e),
        ZStream::RawEnc(e) => drain!(e),
        ZStream::RawDec(e) => drain!(e),
        ZStream::BrotliEnc(e) => {
            e.write_all(chunk)?;
            e.flush()?;
            Ok(std::mem::take(e.get_mut()))
        }
        ZStream::BrotliDec(e) => {
            e.write_all(chunk)?;
            e.flush()?;
            Ok(std::mem::take(e.get_mut()))
        }
        ZStream::UnzipPending(buf) => {
            buf.extend_from_slice(chunk);
            Ok(Vec::new())
        }
    }
}

fn finish_stream(stream: ZStream) -> std::io::Result<Vec<u8>> {
    match stream {
        ZStream::GzEnc(e) => e.finish(),
        ZStream::GzDec(e) => e.finish(),
        ZStream::ZlibEnc(e) => e.finish(),
        ZStream::ZlibDec(e) => e.finish(),
        ZStream::RawEnc(e) => e.finish(),
        ZStream::RawDec(e) => e.finish(),
        ZStream::BrotliEnc(mut e) => {
            e.flush()?;
            Ok(e.into_inner())
        }
        ZStream::BrotliDec(e) => match e.into_inner() {
            Ok(v) => Ok(v),
            Err(v) => Ok(v),
        },
        ZStream::UnzipPending(buf) => {
            // Entire input buffered: auto-detect and decompress in one shot.
            one_shot("unzip", &buf, -1.0)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        }
    }
}

fn zlib_push(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let (Some(id), Some(chunk)) = (num_arg(scope, &args, 0), bytes_arg(scope, &args, 1)) else {
        return throw_bad_args(scope, "zlibPush");
    };
    let finish = bool_arg(scope, &args, 2);
    let id = id as u32;

    let result: Result<Vec<u8>, String> = ZSTREAMS.with(|m| {
        let mut map = m.borrow_mut();
        let Some(stream) = map.get_mut(&id) else {
            return Err("invalid zlib stream handle".to_string());
        };
        let mut out = push_chunk(stream, &chunk).map_err(|e| e.to_string())?;
        if finish {
            let stream = map.remove(&id).expect("stream present: checked above");
            let tail = finish_stream(stream).map_err(|e| e.to_string())?;
            out.extend_from_slice(&tail);
        }
        Ok(out)
    });

    match result {
        Ok(out) => retval.set(make_uint8array(scope, out).into()),
        Err(e) => {
            // Failed handles are poisoned: remove to avoid leaks.
            ZSTREAMS.with(|m| m.borrow_mut().remove(&id));
            throw_coded(scope, "Z_DATA_ERROR", &format!("zlib push: {}", e));
        }
    }
}

fn zlib_free(
    scope: &mut v8::PinnedRef<v8::HandleScope>,
    args: v8::FunctionCallbackArguments,
    _retval: v8::ReturnValue,
) {
    if let Some(id) = num_arg(scope, &args, 0) {
        ZSTREAMS.with(|m| m.borrow_mut().remove(&(id as u32)));
    }
}

/// Clear all live zlib streams (called between requests to stop leaks from
/// interrupted handlers).
pub(crate) fn clear_streams() {
    ZSTREAMS.with(|m| m.borrow_mut().clear());
}
