"use strict";
// node:vm, node:inspector(/promises), node:wasi, node:trace_events,
// node:repl, node:tty, node:readline(/promises), node:domain.
//
// vm/repl require dynamic code evaluation, which the NANO isolate model
// forbids outright (CONTRACT.md §6) — every code-executing entry point
// throws ERR_OPERATION_NOT_PERMITTED even on construction, so feature
// detection fails loudly instead of silently succeeding.
__nanoNodeRegister("vm", function (module, exports, require) {
  const { notPermitted } = require("internal/errors");

  const CONTEXT_MARKER = Symbol("nano.vm.context");

  class Script {
    constructor() {
      throw notPermitted("vm.Script", "dynamic code evaluation is disabled in the NANO runtime");
    }
  }
  class SourceTextModule {
    constructor() {
      throw notPermitted("vm.SourceTextModule", "dynamic code evaluation is disabled in the NANO runtime");
    }
  }

  function denyEval(name) {
    return () => {
      throw notPermitted("vm." + name, "dynamic code evaluation is disabled in the NANO runtime");
    };
  }

  function createContext(sandbox) {
    const ctx = sandbox || {};
    Object.defineProperty(ctx, CONTEXT_MARKER, { value: true, enumerable: false, configurable: true });
    return ctx;
  }
  function isContext(obj) {
    return !!(obj && obj[CONTEXT_MARKER] === true);
  }

  module.exports = {
    Script,
    SourceTextModule,
    SyntheticModule: SourceTextModule,
    createContext,
    isContext,
    runInContext: denyEval("runInContext"),
    runInNewContext: denyEval("runInNewContext"),
    runInThisContext: denyEval("runInThisContext"),
    compileFunction: denyEval("compileFunction"),
    measureMemory: denyEval("measureMemory"),
    constants: Object.freeze({
      USE_MAIN_CONTEXT_DEFAULT_LOADER: 1,
      DONT_CONTEXTIFY: 2,
    }),
  };
});

__nanoNodeRegister("inspector", function (module, exports, require) {
  const { makeError, codes } = require("internal/errors");

  function notAvailable(what) {
    return new codes.ERR_INSPECTOR_NOT_AVAILABLE("Inspector is not available: " + what);
  }

  class Session extends require("events") {
    constructor() {
      super();
      this._connected = false;
    }
    connect() {
      throw notAvailable("Session.connect");
    }
    connectToMainThread() {
      throw notAvailable("Session.connectToMainThread");
    }
    post() {
      throw makeError(Error, "ERR_INSPECTOR_NOT_CONNECTED", "Session is not connected");
    }
    disconnect() {
      this._connected = false;
    }
  }

  module.exports = {
    Session,
    open() {
      throw notAvailable("inspector.open");
    },
    close() {},
    url() {
      return undefined;
    },
    waitForDebugger() {
      throw notAvailable("inspector.waitForDebugger");
    },
    console: globalThis.console,
  };
});

__nanoNodeRegister("inspector/promises", function (module, exports, require) {
  const base = require("inspector");
  class Session extends base.Session {
    post() {
      return Promise.reject(base.Session.prototype.post.call(this));
    }
  }
  module.exports = { ...base, Session };
});

__nanoNodeRegister("wasi", function (module, exports, require) {
  const { makeError } = require("internal/errors");
  class WASI {
    constructor() {
      throw makeError(Error, "ERR_WASI_NOT_AVAILABLE", "WASI is not available in the NANO runtime");
    }
  }
  module.exports = { WASI };
});

__nanoNodeRegister("trace_events", function (module, exports, require) {
  const { makeError } = require("internal/errors");
  class Tracing {
    constructor() {
      this.enabled = false;
      this.categories = "";
    }
    enable() {
      throw makeError(Error, "ERR_TRACE_EVENTS_UNAVAILABLE", "Trace events are unavailable");
    }
    disable() {}
  }
  module.exports = {
    createTracing() {
      throw makeError(Error, "ERR_TRACE_EVENTS_UNAVAILABLE", "Trace events are unavailable");
    },
    getEnabledCategories() {
      return "";
    },
  };
});

__nanoNodeRegister("repl", function (module, exports, require) {
  const EventEmitter = require("events");
  const { notPermitted } = require("internal/errors");

  class Recoverable extends SyntaxError {
    constructor(err) {
      super(err ? err.message : "Recoverable error");
      this.err = err;
    }
  }

  class REPLServer extends EventEmitter {
    constructor() {
      super();
      throw notPermitted("repl.REPLServer", "the REPL requires dynamic code evaluation");
    }
  }

  module.exports = {
    REPLServer,
    Recoverable,
    start() {
      throw notPermitted("repl.start", "the REPL requires dynamic code evaluation");
    },
    REPL_MODE_SLOPPY: Symbol("repl-sloppy"),
    REPL_MODE_STRICT: Symbol("repl-strict"),
    writer(obj) {
      const util = require("util");
      return util.inspect(obj);
    },
  };
});

__nanoNodeRegister("tty", function (module, exports, require) {
  const net = require("net");

  class ReadStream extends net.Socket {
    constructor(fd) {
      super();
      this.fd = fd;
      this.isRaw = false;
      this.isTTY = false;
    }
    setRawMode(mode) {
      this.isRaw = !!mode;
      return this;
    }
  }

  class WriteStream extends net.Socket {
    constructor(fd) {
      super();
      this.fd = fd;
      this.isTTY = false;
      this.columns = undefined;
      this.rows = undefined;
    }
    getColorDepth() {
      return 1;
    }
    hasColors(count) {
      return false;
    }
    getWindowSize() {
      return [this.columns, this.rows];
    }
    cursorTo(x, y, cb) {
      if (typeof y === "function") cb = y;
      if (cb) queueMicrotask(cb);
      return true;
    }
    moveCursor(dx, dy, cb) {
      if (cb) queueMicrotask(cb);
      return true;
    }
    clearLine(dir, cb) {
      if (typeof dir === "function") cb = dir;
      if (cb) queueMicrotask(cb);
      return true;
    }
    clearScreenDown(cb) {
      if (cb) queueMicrotask(cb);
      return true;
    }
  }

  module.exports = {
    ReadStream,
    WriteStream,
    isatty(fd) {
      return false;
    },
  };
});

__nanoNodeRegister("readline", function (module, exports, require) {
  const EventEmitter = require("events");
  const { makeError } = require("internal/errors");

  function stripFinalNewline(lines, hadTrailingNewline) {
    if (!hadTrailingNewline && lines.length > 0 && lines[lines.length - 1] === "") {
      lines.pop();
    }
    return lines;
  }

  class Interface extends EventEmitter {
    constructor(options) {
      super();
      if (options && (typeof options.input === "undefined" && typeof options.write !== "function")) {
        throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", 'The "input" argument must be specified');
      }
      const opts = options && options.input !== undefined ? options : { input: options };
      this.input = opts.input;
      this.output = opts.output;
      this.terminal = !!opts.terminal;
      this.history = [];
      this.historySize = opts.historySize || 30;
      this._closed = false;
      this._paused = false;
      this._buffer = "";
      this._questionQueue = [];
      this._lineListenerAttached = false;
      this._promptStr = opts.prompt !== undefined ? opts.prompt : "> ";
      this._removeNewlineOnly = opts.removeHistoryDuplicates;

      if (this.input && typeof this.input.on === "function") {
        this.input.on("data", (chunk) => this._onData(String(chunk)));
        this.input.on("end", () => {
          if (this._buffer.length > 0) {
            this._emitLine(this._buffer);
            this._buffer = "";
          }
          this.close();
        });
      }
    }
    _onData(chunk) {
      if (this._paused || this._closed) return;
      this._buffer += chunk;
      let idx;
      while ((idx = this._buffer.search(/\r\n|\n|\r/)) !== -1) {
        const line = this._buffer.slice(0, idx);
        const matched = /\r\n|\n|\r/.exec(this._buffer);
        this._buffer = this._buffer.slice(idx + matched[0].length);
        this._emitLine(line);
      }
    }
    _emitLine(line) {
      if (line.length > 0) {
        this.history.unshift(line);
        if (this.history.length > this.historySize) this.history.pop();
      }
      const pending = this._questionQueue.shift();
      if (pending) {
        pending(line);
      } else {
        this.emit("line", line);
      }
    }
    setPrompt(prompt) {
      this._promptStr = prompt;
    }
    getPrompt() {
      return this._promptStr;
    }
    prompt(preserveCursor) {
      if (this.output && typeof this.output.write === "function") {
        this.output.write(this._promptStr);
      }
    }
    question(query, optionsOrCb, cb) {
      const callback = typeof optionsOrCb === "function" ? optionsOrCb : cb;
      const options = typeof optionsOrCb === "function" ? {} : (optionsOrCb || {});
      if (this.output && typeof this.output.write === "function") {
        this.output.write(query);
      }
      const handler = (answer) => callback(answer);
      if (options.signal) {
        if (options.signal.aborted) return;
        options.signal.addEventListener("abort", () => {
          const idx = this._questionQueue.indexOf(handler);
          if (idx !== -1) this._questionQueue.splice(idx, 1);
        }, { once: true });
      }
      this._questionQueue.push(handler);
    }
    write(data, key) {
      if (this.output && typeof this.output.write === "function") {
        this.output.write(data);
      }
    }
    pause() {
      this._paused = true;
      this.emit("pause");
      return this;
    }
    resume() {
      this._paused = false;
      this.emit("resume");
      return this;
    }
    close() {
      if (this._closed) return;
      this._closed = true;
      this.emit("close");
    }
    [Symbol.asyncIterator]() {
      const self = this;
      const queue = [];
      const waiters = [];
      let ended = false;
      const onLine = (line) => {
        const w = waiters.shift();
        if (w) w.resolve({ value: line, done: false });
        else queue.push(line);
      };
      const onClose = () => {
        ended = true;
        for (const w of waiters.splice(0)) w.resolve({ value: undefined, done: true });
      };
      self.on("line", onLine);
      self.on("close", onClose);
      return {
        next() {
          if (queue.length > 0) return Promise.resolve({ value: queue.shift(), done: false });
          if (ended) return Promise.resolve({ value: undefined, done: true });
          return new Promise((resolve) => waiters.push({ resolve }));
        },
        return() {
          self.removeListener("line", onLine);
          self.removeListener("close", onClose);
          return Promise.resolve({ value: undefined, done: true });
        },
        [Symbol.asyncIterator]() {
          return this;
        },
      };
    }
  }

  function createInterface(options, output, completer) {
    if (options && typeof options.on === "function" && output === undefined) {
      options = { input: options };
    } else if (output !== undefined && (options === undefined || typeof options.input === "undefined")) {
      options = { input: options, output, completer };
    }
    return new Interface(options);
  }

  function cursorTo(stream, x, y, cb) {
    if (typeof y === "function") cb = y;
    if (stream && typeof stream.write === "function") {
      stream.write("\x1b[" + (x + 1) + "G");
    }
    if (cb) queueMicrotask(cb);
    return true;
  }
  function moveCursor(stream, dx, dy, cb) {
    if (cb) queueMicrotask(cb);
    return true;
  }
  function clearLine(stream, dir, cb) {
    if (typeof dir === "function") cb = dir;
    if (stream && typeof stream.write === "function") {
      stream.write("\x1b[2K");
    }
    if (cb) queueMicrotask(cb);
    return true;
  }
  function clearScreenDown(stream, cb) {
    if (cb) queueMicrotask(cb);
    return true;
  }
  function emitKeypressEvents(stream, iface) {
    // Minimal: parse single-character keys from 'data' events.
    if (!stream || typeof stream.on !== "function" || stream._nanoKeypressWired) return;
    stream._nanoKeypressWired = true;
    stream.on("data", (chunk) => {
      const s = String(chunk);
      for (const ch of s) {
        const key = { sequence: ch, name: ch === "\r" ? "return" : ch === "\x7f" ? "backspace" : ch, ctrl: false, meta: false, shift: false };
        stream.emit("keypress", ch, key);
      }
    });
  }

  module.exports = {
    Interface, createInterface,
    cursorTo, moveCursor, clearLine, clearScreenDown, emitKeypressEvents,
  };
});

__nanoNodeRegister("readline/promises", function (module, exports, require) {
  const { Interface: CallbackInterface } = require("readline");

  class Interface extends CallbackInterface {
    question(query, options = {}) {
      return new Promise((resolve) => {
        super.question(query, options, resolve);
      });
    }
  }

  function createInterface(options, output, completer) {
    if (options && typeof options.on === "function" && output === undefined) {
      options = { input: options };
    } else if (output !== undefined && (options === undefined || typeof options.input === "undefined")) {
      options = { input: options, output, completer };
    }
    return new Interface(options);
  }

  module.exports = { Interface, createInterface };
});

__nanoNodeRegister("domain", function (module, exports, require) {
  const EventEmitter = require("events");

  class Domain extends EventEmitter {
    constructor() {
      super();
      this.members = [];
    }
    run(fn, ...args) {
      try {
        return fn.apply(this, args);
      } catch (err) {
        if (this.listenerCount("error") > 0) {
          this.emit("error", err);
          return undefined;
        }
        throw err;
      }
    }
    bind(fn) {
      const self = this;
      return function bound(...args) {
        return self.run(() => fn.apply(this, args));
      };
    }
    intercept(fn) {
      const self = this;
      return function intercepted(err, ...args) {
        if (err) {
          self.emit("error", err);
          return undefined;
        }
        return fn.apply(this, args);
      };
    }
    add(emitter) {
      if (!this.members.includes(emitter)) this.members.push(emitter);
    }
    remove(emitter) {
      const idx = this.members.indexOf(emitter);
      if (idx !== -1) this.members.splice(idx, 1);
    }
    enter() {}
    exit() {}
    dispose() {
      this.removeAllListeners();
      this.members = [];
    }
  }

  function create() {
    return new Domain();
  }

  module.exports = { Domain, create, active: null };
});
