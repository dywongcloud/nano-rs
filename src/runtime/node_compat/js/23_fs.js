"use strict";
// node:fs, node:fs/promises — over __nano_node_host VFS hooks (CONTRACT.md §4).
//
// Divergence (documented in docs/NODEJS_COMPAT.md): the VFS is a flat
// object-store namespace with no POSIX permission bits, symlinks, or hard
// links. chmod/chown/utimes succeed as no-ops after validating arguments;
// symlink/link throw ENOSYS; watch()/watchFile() poll stat() rather than
// using inotify.
__nanoNodeRegister("fs", function (module, exports, require) {
  const { makeError, uvError } = require("internal/errors");
  const EventEmitter = require("events");
  const host = globalThis.__nano_node_host;

  function normalize(p) {
    if (typeof p !== "string") {
      if (ArrayBuffer.isView(p)) {
        const { Buffer } = require("buffer");
        p = Buffer.from(p.buffer, p.byteOffset, p.byteLength).toString("utf8");
      } else if (p instanceof URL) {
        p = require("url").fileURLToPath(p);
      } else {
        throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", 'The "path" argument must be of type string, Buffer, or URL');
      }
    }
    let out = [];
    for (const seg of p.split("/")) {
      if (seg === "" || seg === ".") continue;
      if (seg === "..") out.pop();
      else out.push(seg);
    }
    return "/" + out.join("/");
  }

  function callHost(fn, syscall, path) {
    try {
      return fn();
    } catch (e) {
      if (e && typeof e.code === "string") throw e;
      throw uvError("EIO", syscall, path);
    }
  }

  // ---------------------------------------------------------------------
  // Stats
  // ---------------------------------------------------------------------
  class Stats {
    constructor(raw, bigintMode) {
      const wrap = bigintMode ? (n) => BigInt(Math.trunc(n)) : (n) => n;
      this.dev = wrap(0);
      this.mode = wrap(raw.isFile ? 0o100644 : 0o40755);
      this.nlink = wrap(1);
      this.uid = wrap(0);
      this.gid = wrap(0);
      this.rdev = wrap(0);
      this.blksize = wrap(4096);
      this.ino = wrap(0);
      this.size = wrap(raw.size);
      this.blocks = wrap(Math.ceil(raw.size / 512));
      this.atimeMs = wrap(raw.mtimeMs);
      this.mtimeMs = wrap(raw.mtimeMs);
      this.ctimeMs = wrap(raw.mtimeMs);
      this.birthtimeMs = wrap(raw.birthtimeMs);
      this.atime = new Date(raw.mtimeMs);
      this.mtime = new Date(raw.mtimeMs);
      this.ctime = new Date(raw.mtimeMs);
      this.birthtime = new Date(raw.birthtimeMs);
      this._isFile = raw.isFile;
      this._isDirectory = raw.isDirectory;
    }
    isFile() { return this._isFile; }
    isDirectory() { return this._isDirectory; }
    isBlockDevice() { return false; }
    isCharacterDevice() { return false; }
    isSymbolicLink() { return false; }
    isFIFO() { return false; }
    isSocket() { return false; }
  }

  class Dirent {
    constructor(name, raw) {
      this.name = name;
      this._isFile = raw.isFile;
      this._isDirectory = raw.isDirectory;
    }
    isFile() { return this._isFile; }
    isDirectory() { return this._isDirectory; }
    isBlockDevice() { return false; }
    isCharacterDevice() { return false; }
    isSymbolicLink() { return false; }
    isFIFO() { return false; }
    isSocket() { return false; }
  }

  // ---------------------------------------------------------------------
  // Encoding helpers
  // ---------------------------------------------------------------------
  function decodeResult(bytes, options) {
    const { Buffer } = require("buffer");
    const buf = Buffer.from(bytes);
    const encoding = typeof options === "string" ? options : (options && options.encoding);
    if (!encoding || encoding === "buffer") return buf;
    return buf.toString(encoding);
  }

  function toWriteBytes(data, options) {
    const { Buffer } = require("buffer");
    if (typeof data === "string") {
      const encoding = typeof options === "string" ? options : ((options && options.encoding) || "utf8");
      return new Uint8Array(Buffer.from(data, encoding));
    }
    if (ArrayBuffer.isView(data)) {
      return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
    }
    if (data instanceof ArrayBuffer) return new Uint8Array(data);
    throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", 'The "data" argument must be of type string or an instance of Buffer, TypedArray, or DataView');
  }

  // ---------------------------------------------------------------------
  // Sync API
  // ---------------------------------------------------------------------
  function readFileSync(path, options) {
    const p = normalize(path);
    const bytes = callHost(() => host.fsReadFile(p), "open", p);
    return decodeResult(bytes, options);
  }

  function writeFileSync(path, data, options) {
    const p = normalize(path);
    const flag = (typeof options === "object" && options && options.flag) || "w";
    let bytes = toWriteBytes(data, options);
    const exists = host.fsExists(p);
    if ((flag === "wx" || flag === "ax") && exists) {
      throw uvError("EEXIST", "open", p);
    }
    if ((flag === "a" || flag === "ax") && exists) {
      const existing = host.fsReadFile(p);
      const combined = new Uint8Array(existing.length + bytes.length);
      combined.set(existing, 0);
      combined.set(bytes, existing.length);
      bytes = combined;
    }
    callHost(() => host.fsWriteFile(p, bytes), "open", p);
  }

  function appendFileSync(path, data, options) {
    writeFileSync(path, data, { ...(typeof options === "object" ? options : {}), flag: "a" });
  }

  function existsSync(path) {
    try {
      return host.fsExists(normalize(path));
    } catch (_e) {
      return false;
    }
  }

  function unlinkSync(path) {
    const p = normalize(path);
    callHost(() => host.fsUnlink(p), "unlink", p);
  }

  function mkdirSync(path, options) {
    const p = normalize(path);
    const recursive = (typeof options === "object" && options && options.recursive) || false;
    callHost(() => host.fsMkdir(p, recursive), "mkdir", p);
    return recursive ? p : undefined;
  }

  function rmdirSync(path, options) {
    const p = normalize(path);
    if (options && options.recursive) {
      rmSyncRecursive(p);
      return;
    }
    callHost(() => host.fsRmdir(p), "rmdir", p);
  }

  function rmSyncRecursive(p) {
    let isDir;
    try {
      isDir = host.fsStat(p).isDirectory;
    } catch (e) {
      throw e;
    }
    if (isDir) {
      for (const name of host.fsReaddir(p)) {
        rmSyncRecursive(p === "/" ? "/" + name : p + "/" + name);
      }
      host.fsRmdir(p);
    } else {
      host.fsUnlink(p);
    }
  }

  function rmSync(path, options) {
    const p = normalize(path);
    const opts = options || {};
    try {
      const stat = host.fsStat(p);
      if (stat.isDirectory) {
        if (!opts.recursive) {
          throw uvError("ERR_FS_EISDIR", "rm", p);
        }
        rmSyncRecursive(p);
      } else {
        host.fsUnlink(p);
      }
    } catch (e) {
      if (opts.force && e.code === "ENOENT") return;
      throw e;
    }
  }

  function readdirSync(path, options) {
    const p = normalize(path);
    const names = callHost(() => host.fsReaddir(p), "scandir", p);
    const withFileTypes = typeof options === "object" && options && options.withFileTypes;
    const recursive = typeof options === "object" && options && options.recursive;

    function listOne(dir, names_) {
      if (!recursive) return names_.map((n) => (withFileTypes ? new Dirent(n, host.fsStat(dir === "/" ? "/" + n : dir + "/" + n)) : n));
      const out = [];
      for (const n of names_) {
        const full = dir === "/" ? "/" + n : dir + "/" + n;
        const stat = host.fsStat(full);
        const rel = full.slice(p.length + (p === "/" ? 0 : 1));
        out.push(withFileTypes ? new Dirent(rel, stat) : rel);
        if (stat.isDirectory) {
          out.push(...listOne(full, host.fsReaddir(full)));
        }
      }
      return out;
    }
    return listOne(p, names);
  }

  function statSync(path, options) {
    const p = normalize(path);
    const throwIfNoEntry = !(options && options.throwIfNoEntry === false);
    try {
      const raw = host.fsStat(p);
      return new Stats(raw, options && options.bigint);
    } catch (e) {
      if (!throwIfNoEntry && e.code === "ENOENT") return undefined;
      throw e;
    }
  }
  const lstatSync = statSync;

  function fstatSync(fd, options) {
    const entry = fdTable.get(fd);
    if (!entry) throw uvError("EBADF", "fstat", undefined);
    return statSync(entry.path, options);
  }

  function renameSync(oldPath, newPath) {
    const from = normalize(oldPath);
    const to = normalize(newPath);
    callHost(() => host.fsRename(from, to), "rename", from);
  }

  function copyFileSync(src, dest, mode) {
    const from = normalize(src);
    const to = normalize(dest);
    const COPYFILE_EXCL = 1;
    if (((mode || 0) & COPYFILE_EXCL) && host.fsExists(to)) {
      throw uvError("EEXIST", "copyfile", to);
    }
    callHost(() => host.fsCopyFile(from, to), "copyfile", from);
  }

  function truncateSync(path, len = 0) {
    const p = normalize(path);
    const bytes = host.fsExists(p) ? host.fsReadFile(p) : new Uint8Array(0);
    const out = new Uint8Array(len);
    out.set(bytes.subarray(0, Math.min(len, bytes.length)));
    host.fsWriteFile(p, out);
  }

  function ftruncateSync(fd, len = 0) {
    const entry = fdTable.get(fd);
    if (!entry) throw uvError("EBADF", "ftruncate", undefined);
    truncateSync(entry.path, len);
  }

  const F_OK = 0, R_OK = 4, W_OK = 2, X_OK = 1;
  function accessSync(path, mode = F_OK) {
    const p = normalize(path);
    if (!host.fsExists(p)) {
      throw uvError("ENOENT", "access", p);
    }
  }

  function chmodSync(path, mode) {
    const p = normalize(path);
    if (typeof mode !== "number" && typeof mode !== "string") {
      throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", 'The "mode" argument must be of type number or octal string');
    }
    if (!host.fsExists(p) && !host.fsStat) {
      throw uvError("ENOENT", "chmod", p);
    }
    host.fsStat(p); // validates existence; VFS has no permission bits to set
  }
  const lchmodSync = chmodSync;

  function chownSync(path, uid, gid) {
    const p = normalize(path);
    host.fsStat(p); // validates existence; VFS has no ownership to set
  }
  const lchownSync = chownSync;

  function utimesSync(path, atime, mtime) {
    const p = normalize(path);
    host.fsStat(p); // validates existence; VFS does not expose settable timestamps
  }
  const lutimesSync = utimesSync;

  function realpathSync(path) {
    const p = normalize(path);
    if (!host.fsExists(p)) {
      throw uvError("ENOENT", "lstat", p);
    }
    return p;
  }
  realpathSync.native = realpathSync;

  function mkdtempSync(prefix) {
    const { Buffer } = require("buffer");
    const suffix = Buffer.from(host.cryptoRandomBytes(6)).toString("hex").slice(0, 6);
    const dir = normalize(prefix + suffix);
    host.fsMkdir(dir, false);
    return dir;
  }

  function symlinkSync() {
    throw uvError("ENOSYS", "symlink", undefined);
  }
  function linkSync() {
    throw uvError("ENOSYS", "link", undefined);
  }
  function readlinkSync(path) {
    throw uvError("EINVAL", "readlink", normalize(path));
  }

  // ---------------------------------------------------------------------
  // File descriptor table (in-memory, backed by whole-file VFS reads)
  // ---------------------------------------------------------------------
  const fdTable = new Map();
  let nextFd = 3;

  function parseFlag(flag) {
    if (typeof flag === "number") return { read: true, write: true, create: true, append: false, truncate: false, exclusive: false };
    switch (flag) {
      case "r": return { read: true, write: false, create: false, append: false, truncate: false, exclusive: false };
      case "r+": return { read: true, write: true, create: false, append: false, truncate: false, exclusive: false };
      case "w": return { read: false, write: true, create: true, append: false, truncate: true, exclusive: false };
      case "wx": return { read: false, write: true, create: true, append: false, truncate: true, exclusive: true };
      case "w+": return { read: true, write: true, create: true, append: false, truncate: true, exclusive: false };
      case "wx+": return { read: true, write: true, create: true, append: false, truncate: true, exclusive: true };
      case "a": return { read: false, write: true, create: true, append: true, truncate: false, exclusive: false };
      case "ax": return { read: false, write: true, create: true, append: true, truncate: false, exclusive: true };
      case "a+": return { read: true, write: true, create: true, append: true, truncate: false, exclusive: false };
      case "ax+": return { read: true, write: true, create: true, append: true, truncate: false, exclusive: true };
      default:
        throw makeError(TypeError, "ERR_INVALID_ARG_VALUE", "Invalid flag: " + flag);
    }
  }

  function openSync(path, flags = "r", mode) {
    const p = normalize(path);
    const f = parseFlag(flags);
    const exists = host.fsExists(p);
    if (!exists && !f.create) {
      throw uvError("ENOENT", "open", p);
    }
    if (exists && f.exclusive) {
      throw uvError("EEXIST", "open", p);
    }
    let data = exists ? host.fsReadFile(p) : new Uint8Array(0);
    if (f.truncate) data = new Uint8Array(0);
    if (!exists || f.truncate) host.fsWriteFile(p, data);
    const fd = nextFd++;
    fdTable.set(fd, { path: p, data, position: f.append ? data.length : 0, flags: f });
    return fd;
  }

  function closeSync(fd) {
    const entry = fdTable.get(fd);
    if (!entry) throw uvError("EBADF", "close", undefined);
    fdTable.delete(fd);
  }

  function readSync(fd, buffer, offset, length, position) {
    const entry = fdTable.get(fd);
    if (!entry) throw uvError("EBADF", "read", undefined);
    const current = host.fsExists(entry.path) ? host.fsReadFile(entry.path) : new Uint8Array(0);
    const pos = position === null || position === undefined ? entry.position : position;
    const avail = Math.max(0, current.length - pos);
    const toRead = Math.min(length, avail);
    const target = new Uint8Array(buffer.buffer, buffer.byteOffset + offset, length);
    target.set(current.subarray(pos, pos + toRead));
    if (position === null || position === undefined) entry.position += toRead;
    return toRead;
  }

  function writeSync(fd, buffer, offset, length, position) {
    const entry = fdTable.get(fd);
    if (!entry) throw uvError("EBADF", "write", undefined);
    let bytes;
    let writeOffset;
    if (typeof buffer === "string") {
      bytes = toWriteBytes(buffer, offset); // (data, encoding)
      writeOffset = length; // position param shifted
    } else {
      bytes = new Uint8Array(buffer.buffer, buffer.byteOffset + (offset || 0), length === undefined ? buffer.byteLength - (offset || 0) : length);
      writeOffset = position;
    }
    const current = host.fsExists(entry.path) ? host.fsReadFile(entry.path) : new Uint8Array(0);
    const pos = writeOffset === null || writeOffset === undefined ? entry.position : writeOffset;
    const newLen = Math.max(current.length, pos + bytes.length);
    const combined = new Uint8Array(newLen);
    combined.set(current, 0);
    combined.set(bytes, pos);
    host.fsWriteFile(entry.path, combined);
    if (writeOffset === null || writeOffset === undefined) entry.position += bytes.length;
    return bytes.length;
  }

  function fsyncSync(fd) {
    if (!fdTable.has(fd)) throw uvError("EBADF", "fsync", undefined);
  }
  const fdatasyncSync = fsyncSync;

  // ---------------------------------------------------------------------
  // watch / watchFile (VFS has no inotify; watchFile polls stat)
  // ---------------------------------------------------------------------
  class StatWatcher extends EventEmitter {
    constructor(path, interval) {
      super();
      this._path = path;
      this._closed = false;
      this._prev = null;
      this._timer = setInterval(() => this._poll(), interval);
    }
    _poll() {
      if (this._closed) return;
      let stat;
      try {
        stat = statSync(this._path);
      } catch (_e) {
        stat = null;
      }
      if (this._prev !== null && stat !== null && this._prev.mtimeMs !== stat.mtimeMs) {
        this.emit("change", stat, this._prev);
      }
      this._prev = stat;
    }
    ref() { this._timer.ref(); return this; }
    unref() { this._timer.unref(); return this; }
    close() {
      this._closed = true;
      clearInterval(this._timer);
    }
  }

  const statWatchers = new Map();
  function watchFile(path, options, listener) {
    if (typeof options === "function") {
      listener = options;
      options = {};
    }
    const p = normalize(path);
    const interval = (options && options.interval) || 5007;
    let watcher = statWatchers.get(p);
    if (!watcher) {
      watcher = new StatWatcher(p, interval);
      statWatchers.set(p, watcher);
    }
    watcher.on("change", listener);
    return watcher;
  }
  function unwatchFile(path, listener) {
    const p = normalize(path);
    const watcher = statWatchers.get(p);
    if (!watcher) return;
    if (listener) {
      watcher.removeListener("change", listener);
    } else {
      watcher.removeAllListeners("change");
    }
    if (watcher.listenerCount("change") === 0) {
      watcher.close();
      statWatchers.delete(p);
    }
  }

  class FSWatcher extends EventEmitter {
    constructor() {
      super();
      this._closed = false;
    }
    close() {
      this._closed = true;
    }
    ref() { return this; }
    unref() { return this; }
  }
  function watch(path, options, listener) {
    if (typeof options === "function") {
      listener = options;
      options = {};
    }
    const watcher = new FSWatcher();
    if (listener) watcher.on("change", listener);
    // No inotify equivalent over the VFS: the watcher stays open and
    // closeable, but never fires (documented divergence).
    return watcher;
  }

  function globSync(pattern, options) {
    const path = require("path");
    const cwd = normalize((options && options.cwd) || "/");
    const results = [];
    function walk(dir) {
      let names;
      try {
        names = host.fsReaddir(dir);
      } catch (_e) {
        return;
      }
      for (const name of names) {
        const full = dir === "/" ? "/" + name : dir + "/" + name;
        const rel = full.slice(cwd.length + (cwd === "/" ? 0 : 1));
        if (path.matchesGlob(rel, pattern)) {
          results.push(rel);
        }
        const stat = host.fsStat(full);
        if (stat.isDirectory) walk(full);
      }
    }
    walk(cwd);
    return results;
  }

  // ---------------------------------------------------------------------
  // Callback (async) forms — deferred via queueMicrotask, sync internals
  // ---------------------------------------------------------------------
  function wrapAsync(syncFn, resultIndex) {
    return (...args) => {
      const callback = args.pop();
      if (typeof callback !== "function") {
        throw makeError(TypeError, "ERR_INVALID_CALLBACK", "Callback must be a function");
      }
      queueMicrotask(() => {
        try {
          const result = syncFn(...args);
          callback(null, result);
        } catch (e) {
          callback(e);
        }
      });
    };
  }

  const readFile = wrapAsync(readFileSync);
  const writeFile = (...args) => {
    const callback = args.pop();
    queueMicrotask(() => {
      try {
        writeFileSync(...args);
        callback(null);
      } catch (e) {
        callback(e);
      }
    });
  };
  const appendFile = (...args) => {
    const callback = args.pop();
    queueMicrotask(() => {
      try {
        appendFileSync(...args);
        callback(null);
      } catch (e) {
        callback(e);
      }
    });
  };
  function exists(path, callback) {
    queueMicrotask(() => callback(existsSync(path)));
  }
  const unlink = (...args) => {
    const callback = args.pop();
    queueMicrotask(() => {
      try { unlinkSync(...args); callback(null); } catch (e) { callback(e); }
    });
  };
  const mkdir = (...args) => {
    const callback = args.pop();
    queueMicrotask(() => {
      try { callback(null, mkdirSync(...args)); } catch (e) { callback(e); }
    });
  };
  const rmdir = (...args) => {
    const callback = args.pop();
    queueMicrotask(() => {
      try { rmdirSync(...args); callback(null); } catch (e) { callback(e); }
    });
  };
  const rm = (...args) => {
    const callback = args.pop();
    queueMicrotask(() => {
      try { rmSync(...args); callback(null); } catch (e) { callback(e); }
    });
  };
  const readdir = wrapAsync(readdirSync);
  const stat = wrapAsync(statSync);
  const lstat = wrapAsync(lstatSync);
  const fstat = wrapAsync(fstatSync);
  const rename = (...args) => {
    const callback = args.pop();
    queueMicrotask(() => {
      try { renameSync(...args); callback(null); } catch (e) { callback(e); }
    });
  };
  const copyFile = (...args) => {
    const callback = args.pop();
    queueMicrotask(() => {
      try { copyFileSync(...args); callback(null); } catch (e) { callback(e); }
    });
  };
  const truncate = (...args) => {
    const callback = args.pop();
    queueMicrotask(() => {
      try { truncateSync(...args); callback(null); } catch (e) { callback(e); }
    });
  };
  const ftruncate = (...args) => {
    const callback = args.pop();
    queueMicrotask(() => {
      try { ftruncateSync(...args); callback(null); } catch (e) { callback(e); }
    });
  };
  const access = (...args) => {
    const callback = args.pop();
    queueMicrotask(() => {
      try { accessSync(...args); callback(null); } catch (e) { callback(e); }
    });
  };
  const chmod = (...args) => {
    const callback = args.pop();
    queueMicrotask(() => {
      try { chmodSync(...args); callback(null); } catch (e) { callback(e); }
    });
  };
  const chown = (...args) => {
    const callback = args.pop();
    queueMicrotask(() => {
      try { chownSync(...args); callback(null); } catch (e) { callback(e); }
    });
  };
  const utimes = (...args) => {
    const callback = args.pop();
    queueMicrotask(() => {
      try { utimesSync(...args); callback(null); } catch (e) { callback(e); }
    });
  };
  const realpath = wrapAsync(realpathSync);
  const mkdtemp = wrapAsync(mkdtempSync);
  const open = wrapAsync(openSync);
  const close = (...args) => {
    const callback = args.pop();
    queueMicrotask(() => {
      try { closeSync(...args); callback(null); } catch (e) { callback(e); }
    });
  };
  const read = (fd, buffer, offset, length, position, callback) => {
    queueMicrotask(() => {
      try {
        const n = readSync(fd, buffer, offset, length, position);
        callback(null, n, buffer);
      } catch (e) {
        callback(e);
      }
    });
  };
  const write = (fd, ...rest) => {
    const callback = rest.pop();
    queueMicrotask(() => {
      try {
        const n = writeSync(fd, ...rest);
        callback(null, n, rest[0]);
      } catch (e) {
        callback(e);
      }
    });
  };
  const symlink = (...args) => {
    const callback = args.pop();
    queueMicrotask(() => { try { symlinkSync(...args); callback(null); } catch (e) { callback(e); } });
  };
  const link = (...args) => {
    const callback = args.pop();
    queueMicrotask(() => { try { linkSync(...args); callback(null); } catch (e) { callback(e); } });
  };
  const readlink = wrapAsync(readlinkSync);

  // ---------------------------------------------------------------------
  // Streams over the fs read/write primitives
  // ---------------------------------------------------------------------
  function createReadStream(path, options) {
    const { Readable } = require("stream");
    const opts = options || {};
    const p = normalize(path);
    const start = opts.start || 0;
    const highWaterMark = opts.highWaterMark || 65536;
    let position = start;
    let ended = false;
    const stream = new Readable({
      highWaterMark,
      encoding: typeof opts === "string" ? opts : opts.encoding,
      read() {
        if (ended) {
          this.push(null);
          return;
        }
        try {
          const data = host.fsReadFile(p);
          const end = opts.end !== undefined ? opts.end + 1 : data.length;
          const chunk = data.subarray(position, Math.min(end, data.length));
          position += chunk.length;
          ended = true;
          this.push(chunk.length > 0 ? Buffer_from(chunk) : null);
          if (chunk.length === 0) this.push(null);
        } catch (e) {
          this.destroy(e);
        }
      },
    });
    stream.path = p;
    stream.bytesRead = 0;
    return stream;
  }

  function Buffer_from(bytes) {
    const { Buffer } = require("buffer");
    return Buffer.from(bytes);
  }

  function createWriteStream(path, options) {
    const { Writable } = require("stream");
    const opts = options || {};
    const p = normalize(path);
    const flags = opts.flags || "w";
    if (flags === "w") {
      try { host.fsWriteFile(p, new Uint8Array(0)); } catch (_e) { /* created lazily on first write */ }
    }
    const stream = new Writable({
      write(chunk, encoding, callback) {
        try {
          const bytes = toWriteBytes(chunk, encoding);
          const existing = host.fsExists(p) ? host.fsReadFile(p) : new Uint8Array(0);
          const combined = new Uint8Array(existing.length + bytes.length);
          combined.set(existing, 0);
          combined.set(bytes, existing.length);
          host.fsWriteFile(p, combined);
          callback();
        } catch (e) {
          callback(e);
        }
      },
    });
    stream.path = p;
    stream.bytesWritten = 0;
    return stream;
  }

  const constants = Object.freeze({
    F_OK, R_OK, W_OK, X_OK,
    O_RDONLY: 0, O_WRONLY: 1, O_RDWR: 2, O_CREAT: 64, O_EXCL: 128,
    O_NOCTTY: 256, O_TRUNC: 512, O_APPEND: 1024, O_DIRECTORY: 65536,
    O_NOATIME: 262144, O_NOFOLLOW: 131072, O_SYNC: 1052672, O_DSYNC: 4096,
    O_SYMLINK: 0, O_DIRECT: 16384, O_NONBLOCK: 2048,
    S_IFMT: 61440, S_IFREG: 32768, S_IFDIR: 16384, S_IFCHR: 8192,
    S_IFBLK: 24576, S_IFIFO: 4096, S_IFLNK: 40960, S_IFSOCK: 49152,
    S_IRWXU: 448, S_IRUSR: 256, S_IWUSR: 128, S_IXUSR: 64,
    S_IRWXG: 56, S_IRGRP: 32, S_IWGRP: 16, S_IXGRP: 8,
    S_IRWXO: 7, S_IROTH: 4, S_IWOTH: 2, S_IXOTH: 1,
    COPYFILE_EXCL: 1, COPYFILE_FICLONE: 2, COPYFILE_FICLONE_FORCE: 4,
    UV_FS_SYMLINK_DIR: 1, UV_FS_SYMLINK_JUNCTION: 2,
    UV_FS_COPYFILE_EXCL: 1, UV_FS_COPYFILE_FICLONE: 2, UV_FS_COPYFILE_FICLONE_FORCE: 4,
  });

  const fsExports = {
    constants, Stats, Dirent,
    readFileSync, writeFileSync, appendFileSync, existsSync, unlinkSync,
    mkdirSync, rmdirSync, rmSync, readdirSync, statSync, lstatSync, fstatSync,
    renameSync, copyFileSync, truncateSync, ftruncateSync, accessSync,
    chmodSync, lchmodSync, chownSync, lchownSync, utimesSync, lutimesSync,
    realpathSync, mkdtempSync, symlinkSync, linkSync, readlinkSync,
    openSync, closeSync, readSync, writeSync, fsyncSync, fdatasyncSync,
    watchFile, unwatchFile, watch, globSync,
    readFile, writeFile, appendFile, exists, unlink, mkdir, rmdir, rm,
    readdir, stat, lstat, fstat, rename, copyFile, truncate, ftruncate,
    access, chmod, chown, utimes, realpath, mkdtemp, open, close, read, write,
    symlink, link, readlink,
    createReadStream, createWriteStream,
    ReadStream: undefined,
    WriteStream: undefined,
  };
  fsExports.ReadStream = createReadStream;
  fsExports.WriteStream = createWriteStream;

  Object.defineProperty(fsExports, "promises", {
    configurable: true,
    enumerable: true,
    get() {
      return require("fs/promises");
    },
  });

  module.exports = fsExports;
});

__nanoNodeRegister("fs/promises", function (module, exports, require) {
  const fs = require("fs");
  const { makeError } = require("internal/errors");

  function promisify1(syncFn) {
    return (...args) => {
      try {
        return Promise.resolve(syncFn(...args));
      } catch (e) {
        return Promise.reject(e);
      }
    };
  }

  class FileHandle {
    constructor(fd, path) {
      this.fd = fd;
      this._path = path;
    }
    async readFile(options) {
      return fs.readFileSync(this._path, options);
    }
    async writeFile(data, options) {
      fs.writeFileSync(this._path, data, options);
    }
    async appendFile(data, options) {
      fs.appendFileSync(this._path, data, options);
    }
    async read(buffer, offset, length, position) {
      const bytesRead = fs.readSync(this.fd, buffer, offset, length, position);
      return { bytesRead, buffer };
    }
    async write(buffer, offset, length, position) {
      const written = fs.writeSync(this.fd, buffer, offset, length, position);
      return { bytesWritten: written, buffer };
    }
    async stat(options) {
      return fs.fstatSync(this.fd, options);
    }
    async truncate(len = 0) {
      fs.ftruncateSync(this.fd, len);
    }
    async sync() {
      fs.fsyncSync(this.fd);
    }
    async datasync() {
      fs.fdatasyncSync(this.fd);
    }
    async close() {
      fs.closeSync(this.fd);
    }
    createReadStream(options) {
      return fs.createReadStream(this._path, options);
    }
    createWriteStream(options) {
      return fs.createWriteStream(this._path, options);
    }
    readableWebStream() {
      const { Readable } = require("stream");
      return Readable.toWeb(fs.createReadStream(this._path));
    }
    [Symbol.asyncDispose]() {
      return this.close();
    }
  }

  async function open(path, flags, mode) {
    const fd = fs.openSync(path, flags, mode);
    return new FileHandle(fd, path);
  }

  class Dir {
    constructor(path, entries) {
      this._path = path;
      this._entries = entries;
      this._index = 0;
      this._closed = false;
    }
    get path() {
      return this._path;
    }
    async read() {
      if (this._closed || this._index >= this._entries.length) return null;
      return this._entries[this._index++];
    }
    async close() {
      this._closed = true;
    }
    [Symbol.asyncIterator]() {
      return {
        next: async () => {
          const value = await this.read();
          return value === null ? { done: true, value: undefined } : { done: false, value };
        },
      };
    }
  }

  async function opendir(path) {
    const entries = fs.readdirSync(path, { withFileTypes: true });
    return new Dir(path, entries);
  }

  async function* glob(pattern, options) {
    for (const match of fs.globSync(pattern, options)) {
      yield match;
    }
  }

  async function watch(path, options) {
    const w = fs.watch(path, options);
    const signal = options && options.signal;
    return {
      [Symbol.asyncIterator]() {
        return {
          next: () => new Promise((resolve, reject) => {
            if (signal && signal.aborted) {
              w.close();
              resolve({ done: true, value: undefined });
              return;
            }
            const onAbort = () => {
              w.close();
              resolve({ done: true, value: undefined });
            };
            if (signal) signal.addEventListener("abort", onAbort, { once: true });
            w.once("change", (eventType, filename) => {
              if (signal) signal.removeEventListener("abort", onAbort);
              resolve({ done: false, value: { eventType, filename } });
            });
          }),
        };
      },
    };
  }

  module.exports = {
    FileHandle,
    open,
    readFile: promisify1(fs.readFileSync),
    writeFile: promisify1(fs.writeFileSync),
    appendFile: promisify1(fs.appendFileSync),
    unlink: promisify1(fs.unlinkSync),
    mkdir: promisify1(fs.mkdirSync),
    rmdir: promisify1(fs.rmdirSync),
    rm: promisify1(fs.rmSync),
    readdir: promisify1(fs.readdirSync),
    stat: promisify1(fs.statSync),
    lstat: promisify1(fs.lstatSync),
    rename: promisify1(fs.renameSync),
    copyFile: promisify1(fs.copyFileSync),
    truncate: promisify1(fs.truncateSync),
    access: promisify1(fs.accessSync),
    chmod: promisify1(fs.chmodSync),
    lchmod: promisify1(fs.lchmodSync),
    chown: promisify1(fs.chownSync),
    lchown: promisify1(fs.lchownSync),
    utimes: promisify1(fs.utimesSync),
    lutimes: promisify1(fs.lutimesSync),
    realpath: promisify1(fs.realpathSync),
    mkdtemp: promisify1(fs.mkdtempSync),
    symlink: promisify1(fs.symlinkSync),
    link: promisify1(fs.linkSync),
    readlink: promisify1(fs.readlinkSync),
    opendir,
    glob,
    watch,
    constants: fs.constants,
  };
});
