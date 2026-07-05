"use strict";
// internal/web — WHATWG web-platform gap-fill (installed only when the
// bound global is missing a feature; never overwrites a working native).
__nanoNodeRegister("internal/web", function (module, exports, require) {
  const hasGlobal = (name) => typeof globalThis[name] !== "undefined";

  // ---------------------------------------------------------------------
  // EventTarget / Event / CustomEvent
  // ---------------------------------------------------------------------
  class NanoEvent {
    constructor(type, init) {
      if (type === undefined) {
        throw new TypeError("Failed to construct 'Event': 1 argument required, but only 0 present.");
      }
      this.type = String(type);
      this.bubbles = !!(init && init.bubbles);
      this.cancelable = !!(init && init.cancelable);
      this.composed = !!(init && init.composed);
      this.defaultPrevented = false;
      this.eventPhase = 0;
      this.target = null;
      this.currentTarget = null;
      this.timeStamp = typeof performance !== "undefined" ? performance.now() : Date.now();
      this._stopped = false;
      this._stoppedImmediate = false;
      this._path = [];
    }
    stopPropagation() {
      this._stopped = true;
    }
    stopImmediatePropagation() {
      this._stopped = true;
      this._stoppedImmediate = true;
    }
    preventDefault() {
      if (this.cancelable) {
        this.defaultPrevented = true;
      }
    }
    composedPath() {
      return this._path.slice();
    }
  }

  class NanoCustomEvent extends NanoEvent {
    constructor(type, init) {
      super(type, init);
      this.detail = init && "detail" in init ? init.detail : null;
    }
  }

  class NanoEventTarget {
    constructor() {
      this._listeners = new Map();
    }
    addEventListener(type, listener, options) {
      if (listener === null || listener === undefined) return;
      type = String(type);
      const opts = options === true || options === false ? { capture: options } : (options || {});
      let list = this._listeners.get(type);
      if (!list) {
        list = [];
        this._listeners.set(type, list);
      }
      const key = typeof listener === "function" ? listener : listener.handleEvent;
      if (list.some((l) => l.rawListener === listener && !!l.capture === !!opts.capture)) {
        return;
      }
      const entry = { listener: key, rawListener: listener, once: !!opts.once, capture: !!opts.capture, signal: opts.signal };
      list.push(entry);
      if (opts.signal) {
        if (opts.signal.aborted) {
          this.removeEventListener(type, listener, options);
        } else {
          opts.signal.addEventListener("abort", () => this.removeEventListener(type, listener, options), { once: true });
        }
      }
    }
    removeEventListener(type, listener, options) {
      type = String(type);
      const opts = options === true || options === false ? { capture: options } : (options || {});
      const list = this._listeners.get(type);
      if (!list) return;
      const idx = list.findIndex((l) => l.rawListener === listener && !!l.capture === !!opts.capture);
      if (idx !== -1) list.splice(idx, 1);
    }
    dispatchEvent(event) {
      event.target = this;
      event.currentTarget = this;
      const list = this._listeners.get(event.type);
      if (list) {
        for (const entry of list.slice()) {
          if (event._stoppedImmediate) break;
          try {
            entry.listener.call(this, event);
          } finally {
            if (entry.once) {
              this.removeEventListener(event.type, entry.rawListener, { capture: entry.capture });
            }
          }
        }
      }
      const handlerProp = this["on" + event.type];
      if (typeof handlerProp === "function" && !event._stoppedImmediate) {
        handlerProp.call(this, event);
      }
      return !event.defaultPrevented;
    }
  }

  const EventTarget = hasGlobal("EventTarget") ? globalThis.EventTarget : NanoEventTarget;
  const Event = hasGlobal("Event") ? globalThis.Event : NanoEvent;
  const CustomEvent = hasGlobal("CustomEvent") ? globalThis.CustomEvent : NanoCustomEvent;

  // ---------------------------------------------------------------------
  // AbortController / AbortSignal (patch missing statics onto a working native)
  // ---------------------------------------------------------------------
  let AbortController = globalThis.AbortController;
  let AbortSignal = globalThis.AbortSignal;

  function abortErrorFor(reason) {
    const err = new Error("The operation was aborted");
    err.name = "AbortError";
    err.code = "ABORT_ERR";
    if (reason !== undefined) err.cause = reason;
    return err;
  }

  if (!hasGlobal("AbortController") || !hasGlobal("AbortSignal")) {
    class NanoAbortSignal extends EventTarget {
      constructor() {
        super();
        this._aborted = false;
        this._reason = undefined;
        this.onabort = null;
      }
      get aborted() {
        return this._aborted;
      }
      get reason() {
        return this._reason;
      }
      throwIfAborted() {
        if (this._aborted) throw this._reason;
      }
      _doAbort(reason) {
        if (this._aborted) return;
        this._aborted = true;
        this._reason = reason !== undefined ? reason : abortErrorFor();
        const ev = new Event("abort");
        this.dispatchEvent(ev);
      }
      static abort(reason) {
        const s = new NanoAbortSignal();
        s._doAbort(reason !== undefined ? reason : abortErrorFor());
        return s;
      }
      static timeout(ms) {
        const s = new NanoAbortSignal();
        setTimeout(() => s._doAbort(abortErrorFor("TimeoutError")), ms);
        return s;
      }
      static any(signals) {
        const s = new NanoAbortSignal();
        for (const sig of signals) {
          if (sig.aborted) {
            s._doAbort(sig.reason);
            break;
          }
          sig.addEventListener("abort", () => s._doAbort(sig.reason), { once: true });
        }
        return s;
      }
    }
    class NanoAbortController {
      constructor() {
        this.signal = new NanoAbortSignal();
      }
      abort(reason) {
        this.signal._doAbort(reason !== undefined ? reason : abortErrorFor());
      }
    }
    AbortController = NanoAbortController;
    AbortSignal = NanoAbortSignal;
  } else {
    // Patch missing statics onto the working native implementation.
    if (typeof AbortSignal.timeout !== "function") {
      AbortSignal.timeout = function timeout(ms) {
        const c = new AbortController();
        setTimeout(() => c.abort(abortErrorFor("TimeoutError")), ms);
        return c.signal;
      };
    }
    if (typeof AbortSignal.any !== "function") {
      AbortSignal.any = function any(signals) {
        const c = new AbortController();
        for (const sig of signals) {
          if (sig.aborted) {
            c.abort(sig.reason);
            break;
          }
          sig.addEventListener("abort", () => c.abort(sig.reason), { once: true });
        }
        return c.signal;
      };
    }
  }

  // ---------------------------------------------------------------------
  // atob / btoa (WHATWG forgiving-base64)
  // ---------------------------------------------------------------------
  function invalidCharError(msg) {
    const err = new Error(msg);
    err.name = "InvalidCharacterError";
    err.code = 5;
    return err;
  }
  const B64_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  function atobImpl(input) {
    const str = String(input).replace(/[\t\n\f\r ]/g, "");
    if (str.length % 4 === 1 || /[^A-Za-z0-9+/]/.test(str.replace(/=/g, "")) || /=(?!=?$)/.test(str)) {
      throw invalidCharError("The string to be decoded is not correctly encoded.");
    }
    const clean = str.replace(/=+$/, "");
    let out = "";
    for (let i = 0; i < clean.length; i += 4) {
      const chunk = clean.slice(i, i + 4);
      let n = 0;
      for (let j = 0; j < chunk.length; j += 1) {
        n = (n << 6) | B64_ALPHABET.indexOf(chunk[j]);
      }
      n <<= (4 - chunk.length) * 6;
      const bytes = chunk.length - 1;
      for (let j = 0; j < bytes; j += 1) {
        out += String.fromCharCode((n >>> (16 - j * 8)) & 0xff);
      }
    }
    return out;
  }
  function btoaImpl(input) {
    const str = String(input);
    for (let i = 0; i < str.length; i += 1) {
      if (str.charCodeAt(i) > 0xff) {
        throw invalidCharError("The string to be encoded contains characters outside of the Latin1 range.");
      }
    }
    let out = "";
    for (let i = 0; i < str.length; i += 3) {
      const b0 = str.charCodeAt(i);
      const b1 = i + 1 < str.length ? str.charCodeAt(i + 1) : undefined;
      const b2 = i + 2 < str.length ? str.charCodeAt(i + 2) : undefined;
      const n = (b0 << 16) | ((b1 || 0) << 8) | (b2 || 0);
      out += B64_ALPHABET[(n >>> 18) & 63];
      out += B64_ALPHABET[(n >>> 12) & 63];
      out += b1 !== undefined ? B64_ALPHABET[(n >>> 6) & 63] : "=";
      out += b2 !== undefined ? B64_ALPHABET[n & 63] : "=";
    }
    return out;
  }

  // ---------------------------------------------------------------------
  // MessageChannel / MessagePort (in-isolate, structuredClone-based)
  // ---------------------------------------------------------------------
  const untransferable = new WeakSet();

  class NanoMessagePort extends EventTarget {
    constructor() {
      super();
      this._peer = null;
      this._started = false;
      this._queue = [];
      this._closed = false;
      this.onmessage = null;
      this.onmessageerror = null;
    }
    postMessage(data, transferOrOptions) {
      if (this._closed || !this._peer) return;
      let cloned;
      try {
        cloned = structuredClone(data);
      } catch (err) {
        queueMicrotask(() => {
          const ev = new Event("messageerror");
          ev.data = data;
          this.dispatchEvent(ev);
        });
        return;
      }
      const peer = this._peer;
      queueMicrotask(() => {
        if (peer._closed) return;
        if (peer._started) {
          const ev = new Event("message");
          ev.data = cloned;
          peer.dispatchEvent(ev);
        } else {
          peer._queue.push(cloned);
        }
      });
    }
    start() {
      if (this._started) return;
      this._started = true;
      const queued = this._queue.splice(0);
      for (const data of queued) {
        const ev = new Event("message");
        ev.data = data;
        this.dispatchEvent(ev);
      }
    }
    close() {
      this._closed = true;
    }
    set onmessage(fn) {
      // Event-handler-IDL-attribute pattern: real global EventTarget (used
      // as our base when available) does not auto-invoke onXxx properties,
      // so bridge it through addEventListener explicitly.
      if (this._onmessageListener) {
        this.removeEventListener("message", this._onmessageListener);
        this._onmessageListener = null;
      }
      this._onmessage = fn;
      if (typeof fn === "function") {
        this._onmessageListener = fn;
        this.addEventListener("message", fn);
        this.start();
      }
    }
    get onmessage() {
      return this._onmessage || null;
    }
  }

  class MessageChannel {
    constructor() {
      const p1 = new NanoMessagePort();
      const p2 = new NanoMessagePort();
      p1._peer = p2;
      p2._peer = p1;
      this.port1 = p1;
      this.port2 = p2;
    }
  }

  function markAsUntransferable(obj) {
    untransferable.add(obj);
  }
  function isMarkedAsUntransferable(obj) {
    return untransferable.has(obj);
  }

  // ---------------------------------------------------------------------
  // BroadcastChannel (in-isolate registry keyed by name)
  // ---------------------------------------------------------------------
  const broadcastRegistry = new Map(); // name -> Set<channel>

  class NanoBroadcastChannel extends EventTarget {
    constructor(name) {
      super();
      this.name = String(name);
      this._closed = false;
      this._onmessageListener = null;
      let set = broadcastRegistry.get(this.name);
      if (!set) {
        set = new Set();
        broadcastRegistry.set(this.name, set);
      }
      set.add(this);
    }
    postMessage(data) {
      if (this._closed) {
        throw invalidCharError("BroadcastChannel is closed");
      }
      let cloned;
      try {
        cloned = structuredClone(data);
      } catch (_e) {
        cloned = data;
      }
      const set = broadcastRegistry.get(this.name);
      if (!set) return;
      for (const chan of set) {
        if (chan === this || chan._closed) continue;
        queueMicrotask(() => {
          const ev = new Event("message");
          ev.data = cloned;
          ev.origin = "nano://isolate";
          chan.dispatchEvent(ev);
        });
      }
    }
    close() {
      this._closed = true;
      const set = broadcastRegistry.get(this.name);
      if (set) set.delete(this);
    }
    set onmessage(fn) {
      if (this._onmessageListener) {
        this.removeEventListener("message", this._onmessageListener);
        this._onmessageListener = null;
      }
      if (typeof fn === "function") {
        this._onmessageListener = fn;
        this.addEventListener("message", fn);
      }
    }
    get onmessage() {
      return this._onmessageListener || null;
    }
  }
  const BroadcastChannel = hasGlobal("BroadcastChannel") ? globalThis.BroadcastChannel : NanoBroadcastChannel;

  // ---------------------------------------------------------------------
  // Minimal TransformStream (only if the runtime hasn't bound one)
  // ---------------------------------------------------------------------
  let TransformStreamImpl = globalThis.TransformStream;
  if (!TransformStreamImpl) {
    TransformStreamImpl = class NanoTransformStream {
      constructor(transformer = {}, writableStrategy, readableStrategy) {
        let readableController;
        const readable = new ReadableStream({
          start(c) {
            readableController = c;
          },
        }, readableStrategy);
        const controller = {
          enqueue: (chunk) => readableController.enqueue(chunk),
          error: (e) => readableController.error(e),
          terminate: () => {
            try { readableController.close(); } catch (_e) { /* already closed */ }
          },
        };
        let startPromise = Promise.resolve();
        if (typeof transformer.start === "function") {
          startPromise = Promise.resolve(transformer.start(controller));
        }
        const writable = new WritableStream({
          write(chunk) {
            return startPromise.then(() => {
              if (typeof transformer.transform === "function") {
                return transformer.transform(chunk, controller);
              }
              controller.enqueue(chunk);
            });
          },
          close() {
            return startPromise.then(() => {
              if (typeof transformer.flush === "function") {
                return Promise.resolve(transformer.flush(controller)).then(() => controller.terminate());
              }
              controller.terminate();
            });
          },
          abort(reason) {
            readableController.error(reason);
          },
        }, writableStrategy);
        this.readable = readable;
        this.writable = writable;
      }
    };
  }

  // ---------------------------------------------------------------------
  // CompressionStream / DecompressionStream over __nano_node_host zlib
  // ---------------------------------------------------------------------
  const host = globalThis.__nano_node_host;
  const ZLIB_KIND = {
    gzip: { c: "gzip", d: "gunzip" },
    deflate: { c: "deflate", d: "inflate" },
    "deflate-raw": { c: "deflateRaw", d: "inflateRaw" },
  };

  function makeCodecStream(format, mode) {
    const kinds = ZLIB_KIND[format];
    if (!kinds) {
      throw new TypeError("Unsupported compression format: '" + format + "'");
    }
    const kind = mode === "compress" ? kinds.c : kinds.d;
    let handle = null;
    return new TransformStreamImpl({
      start() {
        handle = host.zlibCreate(kind, -1);
      },
      transform(chunk, controller) {
        const bytes = chunk instanceof Uint8Array ? chunk : new Uint8Array(chunk);
        const out = host.zlibPush(handle, bytes, false);
        if (out.length > 0) controller.enqueue(out);
      },
      flush(controller) {
        const out = host.zlibPush(handle, new Uint8Array(0), true);
        if (out.length > 0) controller.enqueue(out);
        handle = null;
      },
    });
  }

  class CompressionStream {
    constructor(format) {
      return makeCodecStream(format, "compress");
    }
  }
  class DecompressionStream {
    constructor(format) {
      return makeCodecStream(format, "decompress");
    }
  }

  // ---------------------------------------------------------------------
  // TextEncoderStream / TextDecoderStream
  // ---------------------------------------------------------------------
  class TextEncoderStream {
    constructor() {
      const encoder = new TextEncoder();
      return new TransformStreamImpl({
        transform(chunk, controller) {
          controller.enqueue(encoder.encode(chunk));
        },
      });
    }
  }
  class TextDecoderStream {
    constructor(label, options) {
      const decoder = new TextDecoder(label || "utf-8", options);
      return new TransformStreamImpl({
        transform(chunk, controller) {
          const text = decoder.decode(chunk, { stream: true });
          if (text.length > 0) controller.enqueue(text);
        },
        flush(controller) {
          const text = decoder.decode();
          if (text.length > 0) controller.enqueue(text);
        },
      });
    }
  }

  // ---------------------------------------------------------------------
  // reportError / navigator / self
  // ---------------------------------------------------------------------
  function reportError(err) {
    console.error(err);
  }

  function __installGlobals(g) {
    if (!hasGlobal("EventTarget")) g.EventTarget = EventTarget;
    if (!hasGlobal("Event")) g.Event = Event;
    if (!hasGlobal("CustomEvent")) g.CustomEvent = CustomEvent;
    if (!hasGlobal("AbortController")) g.AbortController = AbortController;
    if (!hasGlobal("AbortSignal")) g.AbortSignal = AbortSignal;
    if (typeof g.atob !== "function") g.atob = atobImpl;
    if (typeof g.btoa !== "function") g.btoa = btoaImpl;
    if (typeof g.queueMicrotask !== "function") g.queueMicrotask = (cb) => Promise.resolve().then(cb);
    if (!hasGlobal("MessageChannel")) g.MessageChannel = MessageChannel;
    if (!hasGlobal("MessagePort")) g.MessagePort = NanoMessagePort;
    if (!hasGlobal("BroadcastChannel")) g.BroadcastChannel = BroadcastChannel;
    if (!hasGlobal("TransformStream")) g.TransformStream = TransformStreamImpl;
    if (!hasGlobal("CompressionStream")) g.CompressionStream = CompressionStream;
    if (!hasGlobal("DecompressionStream")) g.DecompressionStream = DecompressionStream;
    if (!hasGlobal("TextEncoderStream")) g.TextEncoderStream = TextEncoderStream;
    if (!hasGlobal("TextDecoderStream")) g.TextDecoderStream = TextDecoderStream;
    if (!hasGlobal("reportError")) g.reportError = reportError;
    if (!hasGlobal("self")) g.self = g;
    if (!hasGlobal("navigator")) {
      g.navigator = Object.freeze({
        userAgent: "NANO/2.1 (Node.js compat)",
        platform: "Linux x86_64",
        language: "en-US",
        languages: Object.freeze(["en-US"]),
        hardwareConcurrency: host.availableParallelism(),
        onLine: true,
      });
    }
  }

  module.exports = {
    __installGlobals,
    EventTarget,
    Event,
    CustomEvent,
    AbortController,
    AbortSignal,
    MessageChannel,
    MessagePort: NanoMessagePort,
    BroadcastChannel,
    TransformStream: TransformStreamImpl,
    CompressionStream,
    DecompressionStream,
    TextEncoderStream,
    TextDecoderStream,
    markAsUntransferable,
    isMarkedAsUntransferable,
    atob: atobImpl,
    btoa: btoaImpl,
    reportError,
  };
});
