"use strict";
// node:process — the process object, EventEmitter-based (Node v22 semantics).
//
// Sandbox divergences (documented in docs/NODEJS_COMPAT.md):
// - exit()/abort()/kill() throw a catchable ERR_PROCESS_EXIT that the
//   runtime maps to request termination; they never touch the host process.
// - chdir/setuid/dlopen and friends throw EPERM per the isolate policy.
// - stdin is an immediate-EOF stream; stdout/stderr route to the console.
__nanoNodeRegister("process", function (module, exports, require) {
  const EventEmitter = require("events");
  const { codes, notPermitted, unsupported, makeError } = require("internal/errors");
  const host = globalThis.__nano_node_host;

  const startTime = host.hrtime();

  function hrtimeNow() {
    const t = host.hrtime();
    let sec = t.sec - startTime.sec;
    let ns = t.ns - startTime.ns;
    if (ns < 0) {
      sec -= 1;
      ns += 1e9;
    }
    return [sec, ns];
  }

  function hrtime(prev) {
    const now = hrtimeNow();
    if (prev === undefined) {
      return now;
    }
    if (!Array.isArray(prev) || prev.length !== 2) {
      throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", 'The "time" argument must be an instance of Array. Received ' + typeof prev);
    }
    let sec = now[0] - prev[0];
    let ns = now[1] - prev[1];
    if (ns < 0) {
      sec -= 1;
      ns += 1e9;
    }
    return [sec, ns];
  }
  hrtime.bigint = function bigint() {
    const [sec, ns] = hrtimeNow();
    return BigInt(sec) * 1000000000n + BigInt(ns);
  };

  // --- stdout / stderr: line-buffered writers over the bound console ---
  function makeStdioWriter(fd, sink) {
    let buffered = "";
    const decoder = new TextDecoder("utf-8");
    const stream = new EventEmitter();
    stream.fd = fd;
    stream.isTTY = false;
    stream.writable = true;
    stream.columns = undefined;
    stream.rows = undefined;
    stream.writableHighWaterMark = 16384;
    stream.writableLength = 0;
    stream.bytesWritten = 0;
    stream.write = function write(chunk, encoding, callback) {
      if (typeof encoding === "function") {
        callback = encoding;
        encoding = undefined;
      }
      let text;
      if (typeof chunk === "string") {
        text = chunk;
      } else if (ArrayBuffer.isView(chunk)) {
        text = decoder.decode(new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength));
      } else {
        throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", 'The "chunk" argument must be of type string or an instance of Buffer or Uint8Array');
      }
      stream.bytesWritten += text.length;
      buffered += text;
      let idx = buffered.lastIndexOf("\n");
      if (idx !== -1) {
        const complete = buffered.slice(0, idx);
        buffered = buffered.slice(idx + 1);
        for (const line of complete.split("\n")) {
          sink(line);
        }
      }
      if (typeof callback === "function") {
        queueMicrotask(() => callback());
      }
      return true;
    };
    stream.end = function end(chunk, encoding, callback) {
      if (chunk !== undefined && typeof chunk !== "function") {
        stream.write(chunk, encoding);
      }
      if (buffered.length > 0) {
        sink(buffered);
        buffered = "";
      }
      const cb = typeof chunk === "function" ? chunk : typeof encoding === "function" ? encoding : callback;
      if (typeof cb === "function") {
        queueMicrotask(() => cb());
      }
      return stream;
    };
    stream.cork = () => {};
    stream.uncork = () => {};
    stream.destroy = () => stream;
    stream.hasColors = () => false;
    stream.getColorDepth = () => 1;
    stream.cursorTo = (x, y, cb) => { if (typeof cb === "function") queueMicrotask(cb); return true; };
    stream.moveCursor = (dx, dy, cb) => { if (typeof cb === "function") queueMicrotask(cb); return true; };
    stream.clearLine = (dir, cb) => { if (typeof cb === "function") queueMicrotask(cb); return true; };
    stream.clearScreenDown = (cb) => { if (typeof cb === "function") queueMicrotask(cb); return true; };
    stream._flushForTest = () => {
      if (buffered.length > 0) {
        sink(buffered);
        buffered = "";
      }
    };
    return stream;
  }

  // Keep references to the native console transports before any upgrades.
  const nativeLog = console.log.bind(console);
  const nativeError = console.error.bind(console);

  function makeStdin() {
    const stream = new EventEmitter();
    stream.fd = 0;
    stream.isTTY = false;
    stream.readable = false;
    stream.read = () => null;
    stream.setEncoding = () => stream;
    stream.setRawMode = () => stream;
    stream.pause = () => stream;
    stream.resume = () => stream;
    stream.pipe = (dest) => dest;
    stream.unpipe = () => stream;
    stream.destroy = () => stream;
    stream.ref = () => stream;
    stream.unref = () => stream;
    const origOn = stream.on.bind(stream);
    stream.on = (name, fn) => {
      origOn(name, fn);
      if (name === "end") {
        queueMicrotask(() => stream.emit("end"));
      }
      return stream;
    };
    return stream;
  }

  class Process extends EventEmitter {
    constructor() {
      super();
      this.env = { ...host.getEnv() };
      this.argv = ["node", "/handler.js"];
      this.argv0 = "node";
      this.execPath = "/usr/bin/node";
      this.execArgv = [];
      this.platform = "linux";
      this.arch = "x64";
      this.version = "v22.0.0";
      this.versions = {
        node: "22.0.0",
        nano: "2.1.0",
        v8: "14.7.0",
        uv: "1.48.0",
        zlib: "1.3.1",
        brotli: "1.1.0",
        ares: "1.29.0",
        modules: "127",
        nghttp2: "1.62.0",
        napi: "9",
        llhttp: "9.2.0",
        openssl: "3.0.0",
        cldr: "45.0",
        icu: "75.1",
        tz: "2024a",
        unicode: "15.1",
      };
      this.pid = 1;
      this.ppid = 0;
      this.title = "nano";
      this.exitCode = undefined;
      this.stdout = makeStdioWriter(1, nativeLog);
      this.stderr = makeStdioWriter(2, nativeError);
      this.stdin = makeStdin();
      this.allowedNodeEnvironmentFlags = new Set();
      this.features = Object.freeze({
        inspector: false,
        debug: false,
        uv: false,
        ipv6: true,
        tls_alpn: true,
        tls_sni: true,
        tls_ocsp: false,
        tls: true,
        cached_builtins: true,
        require_module: false,
        typescript: false,
      });
      this.release = Object.freeze({
        name: "node",
        sourceUrl: "",
        headersUrl: "",
        libUrl: "",
      });
      this.config = Object.freeze({ target_defaults: {}, variables: {} });
      this.report = {
        compact: false,
        directory: "",
        filename: "",
        signal: "SIGUSR2",
        reportOnFatalError: false,
        reportOnSignal: false,
        reportOnUncaughtException: false,
        getReport: () => {
          throw unsupported("process.report.getReport");
        },
        writeReport: () => {
          throw unsupported("process.report.writeReport");
        },
      };
      this.permission = { has: () => true };
      this.mainModule = undefined;
      this.domain = null;
      this._exiting = false;
      this.channel = undefined;
      this.connected = undefined;
      this.debugPort = 0;
      this.sourceMapsEnabled = false;
    }

    hrtime(prev) {
      return hrtime(prev);
    }

    nextTick(callback, ...args) {
      if (typeof callback !== "function") {
        throw makeError(TypeError, "ERR_INVALID_CALLBACK", "Callback must be a function. Received " + typeof callback);
      }
      queueMicrotask(() => callback(...args));
    }

    memoryUsage() {
      return host.memoryUsage();
    }

    cpuUsage(previous) {
      const [sec, ns] = hrtimeNow();
      const user = sec * 1e6 + Math.floor(ns / 1e3);
      const usage = { user, system: 0 };
      if (previous) {
        return { user: usage.user - previous.user, system: usage.system - previous.system };
      }
      return usage;
    }

    uptime() {
      const [sec, ns] = hrtimeNow();
      return sec + ns / 1e9;
    }

    cwd() {
      return "/";
    }

    chdir(directory) {
      throw notPermitted("chdir", "process.chdir('" + directory + "')");
    }

    umask(mask) {
      if (mask !== undefined) {
        throw notPermitted("umask", "setting the process umask");
      }
      return 0o22;
    }

    exit(code) {
      this._exiting = true;
      if (code !== undefined) {
        this.exitCode = code;
      }
      this.emit("exit", this.exitCode === undefined ? 0 : this.exitCode);
      const err = new codes.ERR_PROCESS_EXIT(
        "process.exit(" + (code === undefined ? "" : code) + ") requested request termination"
      );
      err.exitCode = this.exitCode === undefined ? 0 : this.exitCode;
      throw err;
    }

    abort() {
      const err = new codes.ERR_PROCESS_EXIT("process.abort() requested request termination");
      err.exitCode = 134;
      throw err;
    }

    kill(pid, signal) {
      throw notPermitted("kill", "process.kill(" + pid + ")");
    }

    reallyExit(code) {
      this.exit(code);
    }

    emitWarning(warning, typeOrOptions, code, ctor) {
      let type = "Warning";
      let detail;
      if (typeof typeOrOptions === "object" && typeOrOptions !== null) {
        type = typeOrOptions.type || type;
        code = typeOrOptions.code;
        detail = typeOrOptions.detail;
      } else if (typeof typeOrOptions === "string") {
        type = typeOrOptions;
      }
      let err;
      if (warning instanceof Error ||
          (warning !== null && typeof warning === "object" && typeof warning.message === "string")) {
        err = warning;
      } else {
        err = new Error(String(warning));
        err.name = type;
        if (Error.captureStackTrace) {
          Error.captureStackTrace(err, this.emitWarning);
        }
      }
      if (code !== undefined) err.code = code;
      if (detail !== undefined) err.detail = detail;
      if (this.listenerCount("warning") > 0) {
        this.emit("warning", err);
      } else {
        console.warn("(" + this.title + ":" + this.pid + ") " +
          (err.code ? "[" + err.code + "] " : "") + (err.name || "Warning") + ": " + err.message);
      }
    }

    getuid() { return 0; }
    geteuid() { return 0; }
    getgid() { return 0; }
    getegid() { return 0; }
    getgroups() { return []; }
    setuid() { throw notPermitted("setuid"); }
    seteuid() { throw notPermitted("seteuid"); }
    setgid() { throw notPermitted("setgid"); }
    setegid() { throw notPermitted("setegid"); }
    setgroups() { throw notPermitted("setgroups"); }
    initgroups() { throw notPermitted("initgroups"); }

    resourceUsage() {
      return {
        userCPUTime: 0, systemCPUTime: 0, maxRSS: 0, sharedMemorySize: 0,
        unsharedDataSize: 0, unsharedStackSize: 0, minorPageFault: 0,
        majorPageFault: 0, swappedOut: 0, fsRead: 0, fsWrite: 0,
        ipcSent: 0, ipcReceived: 0, signalsCount: 0, voluntaryContextSwitches: 0,
        involuntaryContextSwitches: 0,
      };
    }

    availableMemory() {
      const usage = host.memoryUsage();
      return Math.max(0, 512 * 1024 * 1024 - usage.heapUsed);
    }

    constrainedMemory() {
      return 512 * 1024 * 1024;
    }

    binding(name) {
      throw unsupported("process.binding('" + name + "')");
    }

    _linkedBinding(name) {
      throw unsupported("process._linkedBinding('" + name + "')");
    }

    dlopen() {
      throw notPermitted("dlopen", "loading native addons");
    }

    loadEnvFile() {
      throw unsupported("process.loadEnvFile");
    }

    getBuiltinModule(id) {
      if (globalThis.__nanoNodeIsRegistered && globalThis.__nanoNodeIsRegistered(id)) {
        return globalThis.__nanoNodeRequire(id);
      }
      return undefined;
    }

    setUncaughtExceptionCaptureCallback(fn) {
      this._uncaughtExceptionCaptureCallback = fn;
    }

    hasUncaughtExceptionCaptureCallback() {
      return this._uncaughtExceptionCaptureCallback != null;
    }

    setSourceMapsEnabled(enabled) {
      this.sourceMapsEnabled = !!enabled;
    }

    disconnect() {}
    ref() {}
    unref() {}
    openStdin() {
      return this.stdin;
    }
  }

  const process = new Process();

  module.exports = process;
  module.exports.__installGlobals = function __installGlobals(g) {
    g.process = process;
    if (g.global === undefined) {
      g.global = g;
    }
  };
});
