"use strict";
// node:zlib — gzip/deflate/brotli over __nano_node_host (CONTRACT.md §4).
__nanoNodeRegister("zlib", function (module, exports, require) {
  const { makeError } = require("internal/errors");
  const { Transform } = require("stream");
  const host = globalThis.__nano_node_host;

  const constants = Object.freeze({
    Z_NO_FLUSH: 0, Z_PARTIAL_FLUSH: 1, Z_SYNC_FLUSH: 2, Z_FULL_FLUSH: 3,
    Z_FINISH: 4, Z_BLOCK: 5, Z_TREES: 6,
    Z_OK: 0, Z_STREAM_END: 1, Z_NEED_DICT: 2, Z_ERRNO: -1, Z_STREAM_ERROR: -2,
    Z_DATA_ERROR: -3, Z_MEM_ERROR: -4, Z_BUF_ERROR: -5, Z_VERSION_ERROR: -6,
    Z_NO_COMPRESSION: 0, Z_BEST_SPEED: 1, Z_BEST_COMPRESSION: 9, Z_DEFAULT_COMPRESSION: -1,
    Z_FILTERED: 1, Z_HUFFMAN_ONLY: 2, Z_RLE: 3, Z_FIXED: 4, Z_DEFAULT_STRATEGY: 0,
    Z_BINARY: 0, Z_TEXT: 1, Z_ASCII: 1, Z_UNKNOWN: 2,
    Z_DEFAULT_CHUNK: 16384, Z_MIN_WINDOWBITS: 8, Z_MAX_WINDOWBITS: 15,
    Z_DEFAULT_WINDOWBITS: 15, Z_MIN_CHUNK: 64, Z_MAX_CHUNK: Infinity,
    Z_MIN_MEMLEVEL: 1, Z_MAX_MEMLEVEL: 9, Z_DEFAULT_MEMLEVEL: 8,
    Z_MIN_LEVEL: -1, Z_MAX_LEVEL: 9,
    ZLIB_VERNUM: 4736,
    BROTLI_OPERATION_PROCESS: 0, BROTLI_OPERATION_FLUSH: 1,
    BROTLI_OPERATION_FINISH: 2, BROTLI_OPERATION_EMIT_METADATA: 3,
    BROTLI_MODE_GENERIC: 0, BROTLI_MODE_TEXT: 1, BROTLI_MODE_FONT: 2,
    BROTLI_DEFAULT_QUALITY: 11, BROTLI_MIN_QUALITY: 0, BROTLI_MAX_QUALITY: 11,
    BROTLI_DEFAULT_WINDOW: 22, BROTLI_MIN_WINDOW_BITS: 10, BROTLI_MAX_WINDOW_BITS: 24,
    BROTLI_LARGE_MAX_WINDOW_BITS: 30, BROTLI_MIN_INPUT_BLOCK_BITS: 16,
    BROTLI_MAX_INPUT_BLOCK_BITS: 24,
    BROTLI_PARAM_MODE: 0, BROTLI_PARAM_QUALITY: 1, BROTLI_PARAM_LGWIN: 2,
    BROTLI_PARAM_LGBLOCK: 3, BROTLI_PARAM_DISABLE_LITERAL_CONTEXT_MODELING: 4,
    BROTLI_PARAM_SIZE_HINT: 5, BROTLI_PARAM_LARGE_WINDOW: 6,
    BROTLI_PARAM_NPOSTFIX: 7, BROTLI_PARAM_NDIRECT: 8,
    BROTLI_DECODER_PARAM_DISABLE_RING_BUFFER_REALLOCATION: 0,
    BROTLI_DECODER_PARAM_LARGE_WINDOW: 1,
    BROTLI_DECODER_NO_ERROR: 0, BROTLI_DECODER_SUCCESS: 1,
    BROTLI_DECODER_NEEDS_MORE_INPUT: 2, BROTLI_DECODER_NEEDS_MORE_OUTPUT: 3,
  });

  function crc32Table() {
    const table = new Uint32Array(256);
    for (let n = 0; n < 256; n += 1) {
      let c = n;
      for (let k = 0; k < 8; k += 1) {
        c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
      }
      table[n] = c >>> 0;
    }
    return table;
  }
  const CRC_TABLE = crc32Table();

  function crc32(data, value) {
    const { Buffer } = require("buffer");
    const buf = typeof data === "string" ? Buffer.from(data) : Buffer.from(data.buffer ? new Uint8Array(data.buffer, data.byteOffset, data.byteLength) : data);
    let crc = (value === undefined ? 0 : value >>> 0) ^ 0xffffffff;
    for (let i = 0; i < buf.length; i += 1) {
      crc = CRC_TABLE[(crc ^ buf[i]) & 0xff] ^ (crc >>> 8);
    }
    return (crc ^ 0xffffffff) >>> 0;
  }

  function levelFromOptions(options) {
    if (options && typeof options.level === "number") return options.level;
    return -1;
  }

  function toBytes(data) {
    if (typeof data === "string") {
      return new TextEncoder().encode(data);
    }
    if (data instanceof ArrayBuffer) return new Uint8Array(data);
    if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
    throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", 'The "buffer" argument must be of type string or an instance of Buffer, TypedArray, or DataView');
  }

  function dataError(kind, e) {
    const err = new Error(String(e && e.message || e));
    err.code = "Z_DATA_ERROR";
    err.errno = -3;
    return err;
  }

  function syncOp(kind) {
    return function (data, options) {
      const { Buffer } = require("buffer");
      try {
        const out = host.zlibSync(kind, toBytes(data), levelFromOptions(options));
        return Buffer.from(out);
      } catch (e) {
        throw dataError(kind, e);
      }
    };
  }

  function asyncOp(kind) {
    const sync = syncOp(kind);
    return function (data, options, callback) {
      if (typeof options === "function") {
        callback = options;
        options = undefined;
      }
      if (typeof callback !== "function") {
        throw makeError(TypeError, "ERR_INVALID_CALLBACK", "Callback must be a function");
      }
      queueMicrotask(() => {
        try {
          const result = sync(data, options);
          callback(null, result);
        } catch (e) {
          callback(e);
        }
      });
    };
  }

  const KINDS = {
    Gzip: "gzip", Gunzip: "gunzip", Deflate: "deflate", Inflate: "inflate",
    DeflateRaw: "deflateRaw", InflateRaw: "inflateRaw", Unzip: "unzip",
    BrotliCompress: "brotliCompress", BrotliDecompress: "brotliDecompress",
  };

  class ZlibBase extends Transform {
    constructor(kind, options) {
      super(options);
      this._kind = kind;
      this._level = levelFromOptions(options);
      this._handle = host.zlibCreate(kind, this._level);
      this.bytesWritten = 0;
      this._closed = false;
    }
    _transform(chunk, encoding, callback) {
      try {
        const { Buffer } = require("buffer");
        const bytes = toBytes(chunk);
        this.bytesWritten += bytes.length;
        const out = host.zlibPush(this._handle, bytes, false);
        callback(null, out.length > 0 ? Buffer.from(out) : undefined);
      } catch (e) {
        callback(dataError(this._kind, e));
      }
    }
    _flush(callback) {
      try {
        const { Buffer } = require("buffer");
        const out = host.zlibPush(this._handle, new Uint8Array(0), true);
        this._closed = true;
        callback(null, out.length > 0 ? Buffer.from(out) : undefined);
      } catch (e) {
        callback(dataError(this._kind, e));
      }
    }
    flush(kindOrCallback, callback) {
      const cb = typeof kindOrCallback === "function" ? kindOrCallback : callback;
      if (typeof cb === "function") {
        queueMicrotask(cb);
      }
    }
    close(callback) {
      if (!this._closed) {
        host.zlibFree(this._handle);
        this._closed = true;
      }
      if (typeof callback === "function") queueMicrotask(callback);
    }
    reset() {
      if (!this._closed) {
        host.zlibFree(this._handle);
      }
      this._handle = host.zlibCreate(this._kind, this._level);
      this._closed = false;
    }
  }

  const streamClasses = {};
  for (const [name, kind] of Object.entries(KINDS)) {
    streamClasses[name] = class extends ZlibBase {
      constructor(options) {
        super(kind, options);
      }
    };
  }

  function createFactory(ClassCtor) {
    return (options) => new ClassCtor(options);
  }

  const zlibExports = {
    constants,
    crc32,
    Gzip: streamClasses.Gzip, Gunzip: streamClasses.Gunzip,
    Deflate: streamClasses.Deflate, Inflate: streamClasses.Inflate,
    DeflateRaw: streamClasses.DeflateRaw, InflateRaw: streamClasses.InflateRaw,
    Unzip: streamClasses.Unzip,
    BrotliCompress: streamClasses.BrotliCompress, BrotliDecompress: streamClasses.BrotliDecompress,
    createGzip: createFactory(streamClasses.Gzip),
    createGunzip: createFactory(streamClasses.Gunzip),
    createDeflate: createFactory(streamClasses.Deflate),
    createInflate: createFactory(streamClasses.Inflate),
    createDeflateRaw: createFactory(streamClasses.DeflateRaw),
    createInflateRaw: createFactory(streamClasses.InflateRaw),
    createUnzip: createFactory(streamClasses.Unzip),
    createBrotliCompress: createFactory(streamClasses.BrotliCompress),
    createBrotliDecompress: createFactory(streamClasses.BrotliDecompress),
    gzipSync: syncOp("gzip"),
    gunzipSync: syncOp("gunzip"),
    deflateSync: syncOp("deflate"),
    inflateSync: syncOp("inflate"),
    deflateRawSync: syncOp("deflateRaw"),
    inflateRawSync: syncOp("inflateRaw"),
    unzipSync: syncOp("unzip"),
    brotliCompressSync: syncOp("brotliCompress"),
    brotliDecompressSync: syncOp("brotliDecompress"),
    gzip: asyncOp("gzip"),
    gunzip: asyncOp("gunzip"),
    deflate: asyncOp("deflate"),
    inflate: asyncOp("inflate"),
    deflateRaw: asyncOp("deflateRaw"),
    inflateRaw: asyncOp("inflateRaw"),
    unzip: asyncOp("unzip"),
    brotliCompress: asyncOp("brotliCompress"),
    brotliDecompress: asyncOp("brotliDecompress"),
  };

  module.exports = zlibExports;
});
