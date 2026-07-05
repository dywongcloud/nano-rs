"use strict";
// node:stream — Readable/Writable/Duplex/Transform with Node v22 event
// ordering, plus stream/promises, stream/consumers, stream/web.
__nanoNodeRegister("stream", function (module, exports, require) {
  const EventEmitter = require("events");
  const { codes, makeError } = require("internal/errors");

  let defaultHWMBytes = 65536;
  let defaultHWMObjects = 16;

  function getDefaultHighWaterMark(objectMode) {
    return objectMode ? defaultHWMObjects : defaultHWMBytes;
  }
  function setDefaultHighWaterMark(objectMode, value) {
    if (objectMode) {
      defaultHWMObjects = value;
    } else {
      defaultHWMBytes = value;
    }
  }

  function nop() {}

  function toBuffer(chunk, encoding) {
    const { Buffer } = require("buffer");
    if (typeof chunk === "string") {
      return Buffer.from(chunk, encoding || "utf8");
    }
    return chunk;
  }

  function abortError(reason) {
    const err = new Error("The operation was aborted");
    err.name = "AbortError";
    err.code = "ABORT_ERR";
    if (reason !== undefined) err.cause = reason;
    return err;
  }

  // =====================================================================
  // Stream base
  // =====================================================================
  class Stream extends EventEmitter {
    constructor(opts) {
      super(opts);
    }
    pipe(dest, options) {
      return pipeImpl(this, dest, options);
    }
  }

  // =====================================================================
  // Readable
  // =====================================================================
  class ReadableState {
    constructor(options, isDuplex) {
      options = options || {};
      this.objectMode = !!(options.objectMode || (isDuplex && options.readableObjectMode));
      this.highWaterMark = options.highWaterMark !== undefined
        ? options.highWaterMark
        : isDuplex && options.readableHighWaterMark !== undefined
          ? options.readableHighWaterMark
          : getDefaultHighWaterMark(this.objectMode);
      this.buffer = [];
      this.length = 0;
      this.pipes = [];
      this.flowing = null;
      this.ended = false;
      this.endEmitted = false;
      this.reading = false;
      this.constructed = true;
      this.sync = true;
      this.needReadable = false;
      this.emittedReadable = false;
      this.readableListening = false;
      this.resumeScheduled = false;
      this.errorEmitted = false;
      this.emitClose = options.emitClose !== false;
      this.autoDestroy = options.autoDestroy !== false;
      this.destroyed = false;
      this.errored = null;
      this.closed = false;
      this.closeEmitted = false;
      this.defaultEncoding = options.defaultEncoding || "utf8";
      this.awaitDrainWriters = null;
      this.decoder = null;
      this.encoding = null;
      this.readingMore = false;
      this.dataEmitted = false;
      if (options.encoding) {
        const { StringDecoder } = require("string_decoder");
        this.decoder = new StringDecoder(options.encoding);
        this.encoding = options.encoding;
      }
    }
  }

  class Readable extends Stream {
    constructor(options) {
      super(options);
      const isDuplex = this instanceof Duplex;
      this._readableState = new ReadableState(options, isDuplex);
      if (options) {
        if (typeof options.read === "function") this._read = options.read;
        if (typeof options.destroy === "function") this._destroy = options.destroy;
        if (typeof options.construct === "function") this._construct = options.construct;
        if (options.signal && !isDuplex) {
          addAbortSignal(options.signal, this);
        }
      }
      if (typeof this._construct === "function") {
        const state = this._readableState;
        state.constructed = false;
        queueMicrotask(() => {
          this._construct((err) => {
            state.constructed = true;
            if (err) {
              errorOrDestroy(this, err);
            } else if (state.needReadable || state.flowing) {
              maybeReadMore(this, state);
            }
            this.emit("nano:constructed");
          });
        });
      }
    }

    get readable() {
      const s = this._readableState;
      return !!s && s.readable !== false && !s.destroyed && !s.errorEmitted && !s.endEmitted;
    }
    set readable(v) {
      if (this._readableState) {
        this._readableState.readable = !!v;
      }
    }
    get readableEnded() {
      return this._readableState ? this._readableState.endEmitted : false;
    }
    get readableFlowing() {
      return this._readableState.flowing;
    }
    set readableFlowing(v) {
      this._readableState.flowing = v;
    }
    get readableHighWaterMark() {
      return this._readableState.highWaterMark;
    }
    get readableLength() {
      return this._readableState.length;
    }
    get readableObjectMode() {
      return this._readableState.objectMode;
    }
    get readableEncoding() {
      return this._readableState.encoding;
    }
    get destroyed() {
      return this._readableState ? this._readableState.destroyed : false;
    }
    set destroyed(v) {
      if (this._readableState) {
        this._readableState.destroyed = v;
      }
    }
    get errored() {
      return this._readableState ? this._readableState.errored : null;
    }
    get closed() {
      return this._readableState ? this._readableState.closed : false;
    }
    get readableAborted() {
      const s = this._readableState;
      return !!(s.destroyed || s.errored) && !s.endEmitted;
    }
    get readableDidRead() {
      return this._readableState.dataEmitted;
    }

    push(chunk, encoding) {
      return readableAddChunk(this, chunk, encoding, false);
    }
    unshift(chunk, encoding) {
      return readableAddChunk(this, chunk, encoding, true);
    }

    isPaused() {
      return this._readableState.flowing === false;
    }

    setEncoding(enc) {
      const { StringDecoder } = require("string_decoder");
      const decoder = new StringDecoder(enc);
      this._readableState.decoder = decoder;
      this._readableState.encoding = enc;
      // Convert existing buffer
      const s = this._readableState;
      let content = "";
      for (const data of s.buffer) {
        content += decoder.write(data);
      }
      s.buffer.length = 0;
      if (content !== "") {
        s.buffer.push(content);
      }
      s.length = content.length;
      return this;
    }

    read(n) {
      const state = this._readableState;
      if (n === undefined || Number.isNaN(n)) {
        n = NaN;
      }
      if (!Number.isNaN(n) && (typeof n !== "number" || n < 0)) {
        throw makeError(RangeError, "ERR_OUT_OF_RANGE", 'The value of "size" is out of range.');
      }

      // howMuchToRead (Node semantics): flowing mode consumes one buffered
      // chunk at a time; objectMode always one; explicit n caps at length.
      const explicit = !Number.isNaN(n);
      if (!explicit) {
        if (state.objectMode) {
          n = state.length > 0 ? 1 : 0;
        } else if (state.flowing && state.length > 0) {
          n = state.buffer[0].length;
        } else {
          n = state.length;
        }
      } else if (n > state.highWaterMark) {
        state.highWaterMark = nextPow2(n);
      }
      if (state.objectMode && explicit && n > 0) {
        n = 1;
      }

      if (state.ended || state.reading || !state.constructed) {
        // fallthrough to buffered extraction only
      } else if (state.length === 0 || state.length - n < state.highWaterMark) {
        // ask for more
        state.reading = true;
        state.sync = true;
        try {
          this._read(state.highWaterMark);
        } catch (err) {
          errorOrDestroy(this, err);
        }
        state.sync = false;
        if (!state.reading) {
          return this.read(n);
        }
      }

      let ret;
      if (state.objectMode) {
        if (n > 0 && state.length > 0) {
          ret = state.buffer.shift();
          state.length -= 1;
        } else {
          ret = null;
        }
      } else if (n <= 0 || state.length === 0) {
        ret = null;
      } else if (n >= state.length) {
        ret = state.encoding ? state.buffer.join("") : concatBuffers(state.buffer, state.length);
        state.buffer.length = 0;
        state.length = 0;
      } else {
        ret = extractN(state, n);
      }

      if (ret !== null) {
        state.dataEmitted = true;
      }

      if (state.length === 0) {
        if (!state.ended) {
          state.needReadable = true;
        }
        if (state.ended) {
          endReadable(this);
        }
      }
      return ret;
    }

    _read() {
      throw new codes.ERR_METHOD_NOT_IMPLEMENTED("The _read() method is not implemented");
    }

    pause() {
      const state = this._readableState;
      if (state.flowing !== false) {
        state.flowing = false;
        this.emit("pause");
      }
      return this;
    }

    resume() {
      const state = this._readableState;
      if (!state.flowing) {
        state.flowing = !state.readableListening;
        if (!state.resumeScheduled) {
          state.resumeScheduled = true;
          queueMicrotask(() => {
            state.resumeScheduled = false;
            if (!state.reading) {
              this.read(0);
            }
            state.flowing = true;
            flow(this);
            this.emit("resume");
            if (state.flowing && !state.reading) {
              this.read(0);
            }
          });
        }
      }
      return this;
    }

    destroy(err, cb) {
      destroyImpl(this, err, cb);
      return this;
    }

    _destroy(err, cb) {
      cb(err);
    }

    wrap(oldStream) {
      let paused = false;
      oldStream.on("data", (chunk) => {
        if (!this.push(chunk) && typeof oldStream.pause === "function") {
          paused = true;
          oldStream.pause();
        }
      });
      oldStream.on("end", () => this.push(null));
      oldStream.on("error", (err) => errorOrDestroy(this, err));
      this._read = () => {
        if (paused && typeof oldStream.resume === "function") {
          paused = false;
          oldStream.resume();
        }
      };
      return this;
    }

    unpipe(dest) {
      const state = this._readableState;
      if (state.pipes.length === 0) return this;
      if (dest === undefined) {
        const pipes = state.pipes.slice();
        state.pipes = [];
        this.pause();
        for (const d of pipes) {
          d.emit("unpipe", this, { hasUnpiped: false });
        }
        return this;
      }
      const idx = state.pipes.indexOf(dest);
      if (idx === -1) return this;
      state.pipes.splice(idx, 1);
      if (state.pipes.length === 0) {
        this.pause();
      }
      dest.emit("unpipe", this, { hasUnpiped: false });
      return this;
    }

    on(name, fn) {
      const res = super.on(name, fn);
      const state = this._readableState;
      if (name === "data") {
        state.readableListening = this.listenerCount("readable") > 0;
        if (state.flowing !== false) {
          this.resume();
        }
      } else if (name === "readable") {
        if (!state.endEmitted && !state.readableListening) {
          state.readableListening = state.needReadable = true;
          state.flowing = false;
          state.emittedReadable = false;
          if (state.length > 0) {
            emitReadable(this);
          } else if (!state.reading) {
            queueMicrotask(() => this.read(0));
          }
        }
      }
      return res;
    }

    removeListener(name, fn) {
      const res = super.removeListener(name, fn);
      if (name === "readable") {
        queueMicrotask(() => updateReadableListening(this));
      }
      return res;
    }

    removeAllListeners(name) {
      const res = super.removeAllListeners(name);
      if (name === "readable" || name === undefined) {
        queueMicrotask(() => updateReadableListening(this));
      }
      return res;
    }

    [Symbol.asyncIterator]() {
      return createAsyncIterator(this);
    }

    iterator(options) {
      return createAsyncIterator(this, options);
    }

    async *[Symbol.for("nano.values")]() {
      yield* createAsyncIterator(this);
    }

    map(fn, options) {
      const src = this;
      return Readable.from((async function* () {
        for await (const chunk of src) {
          yield await fn(chunk);
        }
      })());
    }

    filter(fn, options) {
      const src = this;
      return Readable.from((async function* () {
        for await (const chunk of src) {
          if (await fn(chunk)) yield chunk;
        }
      })());
    }

    async forEach(fn) {
      for await (const chunk of this) {
        await fn(chunk);
      }
    }

    async toArray(options) {
      const out = [];
      for await (const chunk of this) {
        out.push(chunk);
      }
      return out;
    }

    async some(fn) {
      for await (const chunk of this) {
        if (await fn(chunk)) {
          this.destroy();
          return true;
        }
      }
      return false;
    }

    async every(fn) {
      for await (const chunk of this) {
        if (!(await fn(chunk))) {
          this.destroy();
          return false;
        }
      }
      return true;
    }

    async find(fn) {
      for await (const chunk of this) {
        if (await fn(chunk)) {
          this.destroy();
          return chunk;
        }
      }
      return undefined;
    }

    async reduce(fn, initial) {
      let acc = initial;
      let first = arguments.length < 2;
      for await (const chunk of this) {
        if (first) {
          acc = chunk;
          first = false;
        } else {
          acc = await fn(acc, chunk);
        }
      }
      if (first) {
        throw makeError(TypeError, "ERR_INVALID_ARG_VALUE", "Reduce of an empty stream requires an initial value");
      }
      return acc;
    }

    drop(n) {
      const src = this;
      return Readable.from((async function* () {
        let i = 0;
        for await (const chunk of src) {
          if (i++ >= n) yield chunk;
        }
      })());
    }

    take(n) {
      const src = this;
      return Readable.from((async function* () {
        let i = 0;
        if (n <= 0) return;
        for await (const chunk of src) {
          yield chunk;
          if (++i >= n) break;
        }
      })());
    }

    flatMap(fn) {
      const src = this;
      return Readable.from((async function* () {
        for await (const chunk of src) {
          yield* await fn(chunk);
        }
      })());
    }

    compose(stream, options) {
      return composeImpl(this, stream);
    }

    static from(iterable, opts) {
      return readableFrom(iterable, opts);
    }

    static fromWeb(webStream, options) {
      const reader = webStream.getReader();
      const readable = new Readable({
        objectMode: options && options.objectMode,
        highWaterMark: options && options.highWaterMark,
        read() {
          reader.read().then(
            ({ value, done }) => {
              if (done) {
                this.push(null);
              } else {
                this.push(value);
              }
            },
            (err) => errorOrDestroy(this, err)
          );
        },
        destroy(err, cb) {
          reader.cancel(err).then(() => cb(err), () => cb(err));
        },
      });
      return readable;
    }

    static toWeb(streamReadable) {
      return new ReadableStream({
        start(controller) {
          streamReadable.on("data", (chunk) => {
            controller.enqueue(chunk);
            if (controller.desiredSize !== null && controller.desiredSize <= 0) {
              streamReadable.pause();
            }
          });
          streamReadable.on("end", () => {
            try { controller.close(); } catch (_e) { /* already closed */ }
          });
          streamReadable.on("error", (err) => {
            try { controller.error(err); } catch (_e) { /* already errored */ }
          });
        },
        pull() {
          streamReadable.resume();
        },
        cancel(reason) {
          streamReadable.destroy(reason instanceof Error ? reason : undefined);
        },
      });
    }

    static isDisturbed(stream) {
      const s = stream._readableState;
      return !!(s && (s.dataEmitted || stream.readableAborted));
    }
  }

  function nextPow2(n) {
    let p = 1;
    while (p < n) p <<= 1;
    return p;
  }

  function concatBuffers(list, length) {
    const { Buffer } = require("buffer");
    if (list.length === 1) return list[0];
    return Buffer.concat(list, length);
  }

  function extractN(state, n) {
    if (state.encoding) {
      let out = "";
      while (out.length < n && state.buffer.length > 0) {
        const head = state.buffer[0];
        const need = n - out.length;
        if (head.length <= need) {
          out += head;
          state.buffer.shift();
        } else {
          out += head.slice(0, need);
          state.buffer[0] = head.slice(need);
        }
      }
      state.length -= out.length;
      return out;
    }
    const { Buffer } = require("buffer");
    const out = Buffer.allocUnsafe(n);
    let filled = 0;
    while (filled < n && state.buffer.length > 0) {
      const head = state.buffer[0];
      const need = n - filled;
      if (head.length <= need) {
        out.set(head, filled);
        filled += head.length;
        state.buffer.shift();
      } else {
        out.set(head.subarray(0, need), filled);
        state.buffer[0] = head.subarray(need);
        filled += need;
      }
    }
    state.length -= filled;
    return out;
  }

  function readableAddChunk(stream, chunk, encoding, addToFront) {
    const state = stream._readableState;
    if (chunk === null) {
      state.reading = false;
      if (!state.ended) {
        state.ended = true;
        if (state.decoder) {
          const tail = state.decoder.end();
          if (tail && tail.length) {
            state.buffer.push(tail);
            state.length += state.objectMode ? 1 : tail.length;
          }
        }
        if (state.length === 0 && !state.endEmitted) {
          emitReadableIfListening(stream, state);
          endReadable(stream);
        } else {
          emitReadableIfListening(stream, state);
        }
      }
      return false;
    }

    if (state.ended && !addToFront) {
      errorOrDestroy(stream, new codes.ERR_STREAM_PUSH_AFTER_EOF());
      return false;
    }
    if (state.endEmitted && addToFront) {
      errorOrDestroy(stream, new codes.ERR_STREAM_UNSHIFT_AFTER_END_EVENT());
      return false;
    }
    if (state.destroyed) {
      return false;
    }

    if (!state.objectMode) {
      if (typeof chunk === "string") {
        if (state.decoder) {
          chunk = state.decoder.write(toBuffer(chunk, encoding || state.defaultEncoding));
          if (chunk.length === 0) {
            if (!addToFront) state.reading = false;
            maybeReadMore(stream, state);
            return state.length < state.highWaterMark;
          }
        } else {
          chunk = toBuffer(chunk, encoding || state.defaultEncoding);
        }
      } else if (ArrayBuffer.isView(chunk)) {
        if (state.decoder) {
          chunk = state.decoder.write(chunk);
          if (chunk.length === 0) {
            if (!addToFront) state.reading = false;
            maybeReadMore(stream, state);
            return state.length < state.highWaterMark;
          }
        } else {
          const { Buffer } = require("buffer");
          if (!Buffer.isBuffer(chunk)) {
            chunk = Buffer.from(chunk.buffer, chunk.byteOffset, chunk.byteLength);
          }
        }
      } else if (chunk !== undefined) {
        errorOrDestroy(stream, makeError(TypeError, "ERR_INVALID_ARG_TYPE",
          'The "chunk" argument must be of type string or an instance of Buffer or Uint8Array. Received ' + typeof chunk));
        return false;
      }
    }

    const size = state.objectMode ? 1 : chunk.length;
    if (addToFront) {
      state.buffer.unshift(chunk);
      state.length += size;
    } else {
      state.reading = false;
      state.buffer.push(chunk);
      state.length += size;
    }

    if (state.flowing && state.length > 0 && !state.sync) {
      // fast-path flow
      flow(stream);
    } else {
      emitReadableIfListening(stream, state);
    }
    maybeReadMore(stream, state);
    return state.length < state.highWaterMark;
  }

  function emitReadableIfListening(stream, state) {
    state.needReadable = false;
    if (state.readableListening && !state.emittedReadable) {
      emitReadable(stream);
    }
  }

  function emitReadable(stream) {
    const state = stream._readableState;
    state.emittedReadable = true;
    queueMicrotask(() => {
      state.emittedReadable = false;
      if (state.length > 0 || state.ended) {
        stream.emit("readable");
      }
      state.needReadable = !state.flowing && !state.ended && state.length <= state.highWaterMark;
      flow(stream);
    });
  }

  function maybeReadMore(stream, state) {
    if (!state.readingMore && state.constructed) {
      state.readingMore = true;
      queueMicrotask(() => {
        while (!state.reading && !state.ended &&
               (state.length < state.highWaterMark || (state.flowing && state.length === 0))) {
          const len = state.length;
          stream.read(0);
          if (len === state.length) break;
        }
        state.readingMore = false;
      });
    }
  }

  function flow(stream) {
    const state = stream._readableState;
    while (state.flowing && state.length > 0) {
      const chunk = stream.read();
      if (chunk === null) break;
      stream.emit("data", chunk);
    }
  }

  function updateReadableListening(stream) {
    const state = stream._readableState;
    state.readableListening = stream.listenerCount("readable") > 0;
    if (stream.listenerCount("data") > 0) {
      stream.resume();
    } else if (!state.readableListening) {
      state.flowing = null;
    }
  }

  function endReadable(stream) {
    const state = stream._readableState;
    if (!state.endEmitted && state.ended && state.length === 0) {
      state.endEmitted = true;
      queueMicrotask(() => {
        if (state.destroyed && state.closeEmitted) return;
        stream.readable = false;
        state.endEventEmitted = true;
        stream.emit("end");
        if (state.autoDestroy) {
          const wState = stream._writableState;
          const autoDestroy = !wState || (wState.autoDestroy && (wState.finished || wState.writable === false));
          if (autoDestroy) {
            stream.destroy();
          }
        }
      });
    }
  }

  function pipeImpl(src, dest, options) {
    const state = src._readableState;
    state.pipes.push(dest);
    const doEnd = (!options || options.end !== false) &&
      dest !== globalThis.process?.stdout && dest !== globalThis.process?.stderr;

    const onData = (chunk) => {
      const ret = dest.write(chunk);
      if (ret === false) {
        src.pause();
      }
    };
    const onDrain = () => {
      src.resume();
    };
    const onEnd = () => {
      if (doEnd) dest.end();
    };
    const cleanup = () => {
      src.removeListener("data", onData);
      dest.removeListener("drain", onDrain);
      src.removeListener("end", onEnd);
      src.removeListener("close", onSrcClose);
      dest.removeListener("close", onDestClose);
      dest.removeListener("error", onDestError);
      src.removeListener("error", onSrcError);
    };
    const onDestError = (_err) => {
      cleanup();
      if (dest.listenerCount("error") === 0) {
        // re-throwing handled by dest itself
      }
      src.unpipe(dest);
    };
    const onSrcError = (_err) => {
      cleanup();
    };
    const onSrcClose = () => {};
    const onDestClose = () => {
      cleanup();
      src.unpipe(dest);
    };

    src.on("data", onData);
    dest.on("drain", onDrain);
    src.on("end", onEnd);
    src.on("close", onSrcClose);
    dest.on("close", onDestClose);
    dest.on("error", onDestError);
    src.on("error", onSrcError);

    dest.emit("pipe", src);
    return dest;
  }

  function createAsyncIterator(stream, options) {
    const destroyOnReturn = !options || options.destroyOnReturn !== false;
    let error = null;
    let ended = false;
    const queue = [];
    const waiting = [];

    const onData = (chunk) => {
      const w = waiting.shift();
      if (w) {
        w.resolve({ value: chunk, done: false });
      } else {
        queue.push(chunk);
      }
      // One chunk per next(): never prefetch past what the consumer took.
      stream.pause();
    };
    const onEnd = () => {
      ended = true;
      drainWaiting();
    };
    const onError = (err) => {
      error = err;
      drainWaiting();
    };
    const onClose = () => {
      ended = true;
      drainWaiting();
    };
    function drainWaiting() {
      for (const w of waiting.splice(0)) {
        if (error) {
          w.reject(error);
        } else {
          w.resolve({ value: undefined, done: true });
        }
      }
    }
    function adjustPause() {}

    stream.on("data", onData);
    stream.on("end", onEnd);
    stream.on("error", onError);
    stream.on("close", onClose);
    stream.pause();

    return {
      next() {
        if (queue.length > 0) {
          const value = queue.shift();
          return Promise.resolve({ value, done: false });
        }
        if (error) {
          return Promise.reject(error);
        }
        if (ended || stream.destroyed) {
          return Promise.resolve({ value: undefined, done: true });
        }
        stream.resume();
        return new Promise((resolve, reject) => {
          waiting.push({ resolve, reject });
        });
      },
      return() {
        // Real Node emits 'close' several microtask hops after the break
        // continuation resumes; defer destroy two hops to match.
        if (destroyOnReturn) {
          queueMicrotask(() => queueMicrotask(() => stream.destroy()));
        }
        return Promise.resolve({ value: undefined, done: true });
      },
      throw(err) {
        stream.destroy(err);
        return Promise.reject(err);
      },
      [Symbol.asyncIterator]() {
        return this;
      },
    };
  }

  function readableFrom(iterable, opts) {
    if (typeof iterable === "string" || ArrayBuffer.isView(iterable)) {
      iterable = [iterable];
    }
    let iterator;
    if (iterable && typeof iterable[Symbol.asyncIterator] === "function") {
      iterator = iterable[Symbol.asyncIterator]();
    } else if (iterable && typeof iterable[Symbol.iterator] === "function") {
      iterator = iterable[Symbol.iterator]();
    } else if (iterable && typeof iterable.then === "function") {
      const promise = iterable;
      iterator = (async function* () {
        yield await promise;
      })();
    } else {
      throw makeError(TypeError, "ERR_INVALID_ARG_TYPE",
        'The "iterable" argument must be an instance of Iterable. Received ' + typeof iterable);
    }
    let reading = false;
    const readable = new Readable({
      objectMode: true,
      highWaterMark: 1,
      ...opts,
      read() {
        if (reading) return;
        reading = true;
        Promise.resolve(iterator.next()).then(
          ({ value, done }) => {
            reading = false;
            if (done) {
              this.push(null);
            } else if (this.push(value)) {
              this._read();
            }
          },
          (err) => {
            reading = false;
            errorOrDestroy(this, err);
          }
        );
      },
      destroy(err, cb) {
        if (typeof iterator.return === "function") {
          Promise.resolve(iterator.return()).then(() => cb(err), () => cb(err));
        } else {
          cb(err);
        }
      },
    });
    return readable;
  }

  // =====================================================================
  // Writable
  // =====================================================================
  class WritableState {
    constructor(options, isDuplex) {
      options = options || {};
      this.objectMode = !!(options.objectMode || (isDuplex && options.writableObjectMode));
      this.highWaterMark = options.highWaterMark !== undefined
        ? options.highWaterMark
        : isDuplex && options.writableHighWaterMark !== undefined
          ? options.writableHighWaterMark
          : getDefaultHighWaterMark(this.objectMode);
      this.decodeStrings = options.decodeStrings !== false;
      this.defaultEncoding = options.defaultEncoding || "utf8";
      this.length = 0;
      this.writing = false;
      this.corked = 0;
      this.sync = true;
      this.buffered = [];
      this.ended = false;
      this.ending = false;
      this.finished = false;
      this.prefinished = false;
      this.destroyed = false;
      this.errored = null;
      this.closed = false;
      this.closeEmitted = false;
      this.errorEmitted = false;
      this.emitClose = options.emitClose !== false;
      this.autoDestroy = options.autoDestroy !== false;
      this.constructed = true;
      this.needDrain = false;
      this.writable = true;
      this.pendingcb = 0;
      this.onFinished = [];
    }
  }

  class Writable extends Stream {
    constructor(options) {
      super(options);
      const isDuplex = this instanceof Duplex;
      this._writableState = new WritableState(options, isDuplex);
      if (options) {
        if (typeof options.write === "function") this._write = options.write;
        if (typeof options.writev === "function") this._writev = options.writev;
        if (typeof options.final === "function") this._final = options.final;
        if (typeof options.destroy === "function") this._destroy = options.destroy;
        if (typeof options.construct === "function") this._construct = options.construct;
        if (options.signal && !isDuplex) {
          addAbortSignal(options.signal, this);
        }
      }
      if (typeof this._construct === "function" && !this._readableState) {
        const state = this._writableState;
        state.constructed = false;
        queueMicrotask(() => {
          this._construct((err) => {
            state.constructed = true;
            if (err) {
              errorOrDestroy(this, err);
            } else {
              clearBufferSync(this, state);
            }
            this.emit("nano:constructed");
          });
        });
      }
    }

    get writable() {
      const s = this._writableState;
      return !!s && s.writable !== false && !s.destroyed && !s.errored && !s.ending && !s.ended;
    }
    set writable(v) {
      if (this._writableState) {
        this._writableState.writable = !!v;
      }
    }
    get writableEnded() {
      return this._writableState ? this._writableState.ending : false;
    }
    get writableFinished() {
      return this._writableState ? this._writableState.finished : false;
    }
    get writableHighWaterMark() {
      return this._writableState.highWaterMark;
    }
    get writableLength() {
      return this._writableState.length;
    }
    get writableObjectMode() {
      return this._writableState.objectMode;
    }
    get writableCorked() {
      return this._writableState.corked;
    }
    get writableNeedDrain() {
      return this._writableState ? this._writableState.needDrain : false;
    }
    get destroyed() {
      return this._writableState ? this._writableState.destroyed : false;
    }
    set destroyed(v) {
      if (this._writableState) {
        this._writableState.destroyed = v;
      }
    }
    get errored() {
      return this._writableState ? this._writableState.errored : null;
    }
    get closed() {
      return this._writableState ? this._writableState.closed : false;
    }
    get writableAborted() {
      const s = this._writableState;
      return !!(s.destroyed || s.errored) && !s.finished;
    }

    write(chunk, encoding, cb) {
      if (typeof encoding === "function") {
        cb = encoding;
        encoding = undefined;
      }
      return writeImpl(this, chunk, encoding, cb);
    }

    cork() {
      this._writableState.corked += 1;
    }

    uncork() {
      const state = this._writableState;
      if (state.corked > 0) {
        state.corked -= 1;
        if (!state.writing) {
          clearBufferSync(this, state);
        }
      }
    }

    setDefaultEncoding(encoding) {
      this._writableState.defaultEncoding = encoding;
      return this;
    }

    end(chunk, encoding, cb) {
      const state = this._writableState;
      if (typeof chunk === "function") {
        cb = chunk;
        chunk = undefined;
        encoding = undefined;
      } else if (typeof encoding === "function") {
        cb = encoding;
        encoding = undefined;
      }

      let err;
      if (chunk !== undefined && chunk !== null) {
        writeImpl(this, chunk, encoding, undefined);
      }

      if (state.corked > 0) {
        state.corked = 1;
        this.uncork();
      }

      if (state.ending || state.destroyed) {
        err = state.destroyed
          ? new codes.ERR_STREAM_DESTROYED("end")
          : new codes.ERR_STREAM_ALREADY_FINISHED("end");
      } else {
        state.ending = true;
        finishMaybe(this, state);
        state.ended = true;
      }

      if (typeof cb === "function") {
        if (err) {
          queueMicrotask(() => cb(err));
        } else if (state.finished) {
          queueMicrotask(() => cb());
        } else {
          state.onFinished.push(cb);
        }
      }
      if (err) {
        queueMicrotask(() => errorOrDestroy(this, err, true));
      }
      return this;
    }

    _write(chunk, encoding, callback) {
      if (this._writev) {
        this._writev([{ chunk, encoding }], callback);
      } else {
        throw new codes.ERR_METHOD_NOT_IMPLEMENTED("The _write() method is not implemented");
      }
    }

    destroy(err, cb) {
      destroyImpl(this, err, cb);
      return this;
    }

    _destroy(err, cb) {
      cb(err);
    }

    static fromWeb(webWritable, options) {
      const writer = webWritable.getWriter();
      return new Writable({
        objectMode: options && options.objectMode,
        highWaterMark: options && options.highWaterMark,
        decodeStrings: false,
        write(chunk, encoding, callback) {
          writer.write(chunk).then(() => callback(), (err) => callback(err));
        },
        final(callback) {
          writer.close().then(() => callback(), (err) => callback(err));
        },
        destroy(err, callback) {
          writer.abort(err).then(() => callback(err), () => callback(err));
        },
      });
    }

    static toWeb(streamWritable) {
      return new WritableStream({
        write(chunk) {
          return new Promise((resolve, reject) => {
            streamWritable.write(chunk, (err) => (err ? reject(err) : resolve()));
          });
        },
        close() {
          return new Promise((resolve, reject) => {
            streamWritable.end((err) => (err ? reject(err) : resolve()));
          });
        },
        abort(reason) {
          streamWritable.destroy(reason instanceof Error ? reason : undefined);
        },
      });
    }
  }

  function writeImpl(stream, chunk, encoding, cb) {
    const state = stream._writableState;
    if (typeof cb !== "function") {
      cb = nop;
    }

    if (chunk === null) {
      const err = new codes.ERR_STREAM_NULL_VALUES();
      queueMicrotask(() => cb(err));
      errorOrDestroy(stream, err, true);
      return false;
    }

    if (!state.objectMode) {
      if (typeof chunk === "string") {
        if (state.decodeStrings) {
          chunk = toBuffer(chunk, encoding || state.defaultEncoding);
        }
      } else if (ArrayBuffer.isView(chunk)) {
        const { Buffer } = require("buffer");
        if (!Buffer.isBuffer(chunk)) {
          chunk = Buffer.from(chunk.buffer, chunk.byteOffset, chunk.byteLength);
        }
      } else {
        const err = makeError(TypeError, "ERR_INVALID_ARG_TYPE",
          'The "chunk" argument must be of type string or an instance of Buffer or Uint8Array. Received ' +
          (chunk === null ? "null" : typeof chunk));
        queueMicrotask(() => cb(err));
        errorOrDestroy(stream, err, true);
        return false;
      }
    }

    let err;
    if (state.ending) {
      err = new codes.ERR_STREAM_WRITE_AFTER_END("write after end");
    } else if (state.destroyed) {
      err = new codes.ERR_STREAM_DESTROYED("Cannot call write after a stream was destroyed");
    }
    if (err) {
      queueMicrotask(() => cb(err));
      errorOrDestroy(stream, err, true);
      return false;
    }

    const len = state.objectMode ? 1 : chunk.length;
    state.length += len;
    state.pendingcb += 1;

    if (state.writing || state.corked > 0 || !state.constructed || state.buffered.length > 0) {
      state.buffered.push({ chunk, encoding, cb });
    } else {
      doWrite(stream, state, chunk, encoding, cb);
    }

    const ret = state.length < state.highWaterMark;
    if (!ret) {
      state.needDrain = true;
    }
    return ret;
  }

  function doWrite(stream, state, chunk, encoding, cb) {
    state.writing = true;
    state.sync = true;
    let called = false;
    const onwrite = (err) => {
      if (called) {
        errorOrDestroy(stream, new codes.ERR_MULTIPLE_CALLBACK());
        return;
      }
      called = true;
      state.writing = false;
      const len = state.objectMode ? 1 : chunk.length;
      state.length -= len;
      if (err) {
        state.pendingcb -= 1;
        queueMicrotask(() => cb(err));
        errorOrDestroy(stream, err, true);
        return;
      }
      if (state.sync) {
        queueMicrotask(() => {
          state.pendingcb -= 1;
          cb();
          afterWrite(stream, state);
        });
      } else {
        state.pendingcb -= 1;
        cb();
        afterWrite(stream, state);
      }
    };
    try {
      stream._write(chunk, encoding || state.defaultEncoding, onwrite);
    } catch (err) {
      onwrite(err);
    }
    state.sync = false;
  }

  function afterWrite(stream, state) {
    if (state.destroyed) {
      // drop buffered writes with destroy error
      for (const { cb } of state.buffered.splice(0)) {
        state.pendingcb -= 1;
        queueMicrotask(() => cb(new codes.ERR_STREAM_DESTROYED("write")));
      }
      return;
    }
    clearBufferSync(stream, state);
    if (state.needDrain && state.length === 0 && !state.ending && !state.destroyed) {
      state.needDrain = false;
      stream.emit("drain");
    }
    finishMaybe(stream, state);
  }

  function clearBufferSync(stream, state) {
    if (state.writing || state.corked > 0 || !state.constructed || state.destroyed) {
      return;
    }
    if (state.buffered.length === 0) {
      return;
    }
    if (typeof stream._writev === "function" && state.buffered.length > 1) {
      const entries = state.buffered.splice(0);
      const totalLen = entries.reduce((acc, e) => acc + (state.objectMode ? 1 : e.chunk.length), 0);
      state.writing = true;
      state.sync = true;
      let called = false;
      const onwrite = (err) => {
        if (called) return;
        called = true;
        state.writing = false;
        state.length -= totalLen;
        state.pendingcb -= entries.length;
        if (err) {
          for (const e of entries) {
            queueMicrotask(() => e.cb(err));
          }
          errorOrDestroy(stream, err, true);
          return;
        }
        const finishCbs = () => {
          for (const e of entries) {
            e.cb();
          }
          afterWrite(stream, state);
        };
        if (state.sync) {
          queueMicrotask(finishCbs);
        } else {
          finishCbs();
        }
      };
      try {
        stream._writev(entries.map((e) => ({ chunk: e.chunk, encoding: e.encoding || state.defaultEncoding })), onwrite);
      } catch (err) {
        onwrite(err);
      }
      state.sync = false;
      return;
    }
    const entry = state.buffered.shift();
    doWrite(stream, state, entry.chunk, entry.encoding, entry.cb);
  }

  function needFinish(state) {
    return state.ending && state.constructed && state.length === 0 &&
      state.pendingcb === 0 && !state.errored && state.buffered.length === 0 &&
      !state.finished && !state.writing && !state.errorEmitted && !state.closeEmitted;
  }

  function finishMaybe(stream, state) {
    if (!needFinish(state)) {
      return;
    }
    if (!state.prefinished && typeof stream._final === "function" && !state.finalCalled) {
      state.finalCalled = true;
      state.sync = true;
      state.pendingcb += 1;
      try {
        stream._final((err) => {
          state.pendingcb -= 1;
          if (err) {
            errorOrDestroy(stream, err, state.sync);
            return;
          }
          state.prefinished = true;
          stream.emit("prefinish");
          queueMicrotask(() => emitFinish(stream, state));
        });
      } catch (err) {
        state.pendingcb -= 1;
        errorOrDestroy(stream, err, state.sync);
      }
      state.sync = false;
    } else {
      if (!state.prefinished) {
        state.prefinished = true;
        stream.emit("prefinish");
      }
      queueMicrotask(() => emitFinish(stream, state));
    }
  }

  function emitFinish(stream, state) {
    if (state.finished || state.destroyed || state.errorEmitted) {
      if (state.destroyed && !state.finished && !state.errored) {
        // destroyed after end() flush: still emit finish? Node: no
      }
      return;
    }
    if (!needFinish(state)) {
      return;
    }
    state.finished = true;
    for (const cb of state.onFinished.splice(0)) {
      cb();
    }
    stream.emit("finish");
    if (state.autoDestroy) {
      const rState = stream._readableState;
      const autoDestroy = !rState || (rState.autoDestroy && (rState.endEmitted || rState.readable === false));
      if (autoDestroy) {
        stream.destroy();
      }
    }
  }

  // =====================================================================
  // destroy / error plumbing (shared)
  // =====================================================================
  function destroyImpl(stream, err, cb) {
    const r = stream._readableState;
    const w = stream._writableState;

    if ((w && w.destroyed) || (r && r.destroyed)) {
      if (typeof cb === "function") {
        cb();
      }
      return;
    }

    if (err) {
      err.stack; // materialize
      if (w && !w.errored) w.errored = err;
      if (r && !r.errored) r.errored = err;
    }

    if (w) {
      w.destroyed = true;
      w.writable = false;
    }
    if (r) {
      r.destroyed = true;
      r.readable = false;
    }

    if (w) {
      const destroyErr = err || new codes.ERR_STREAM_DESTROYED("end");
      for (const cb of w.onFinished.splice(0)) {
        queueMicrotask(() => cb(destroyErr));
      }
    }
    stream._destroy(err || null, (err2) => {
      if (err2) {
        err2.stack;
        if (w && !w.errored) w.errored = err2;
        if (r && !r.errored) r.errored = err2;
      }
      const emitErr = (w && w.errored) || (r && r.errored);
      queueMicrotask(() => {
        if (emitErr && !(w && w.errorEmitted) && !(r && r.errorEmitted)) {
          if (w) w.errorEmitted = true;
          if (r) r.errorEmitted = true;
          stream.emit("error", emitErr);
        }
        queueMicrotask(() => {
          emitCloseNT(stream);
          if (typeof cb === "function") {
            cb(emitErr || undefined);
          }
        });
      });
    });
  }

  function emitCloseNT(stream) {
    const r = stream._readableState;
    const w = stream._writableState;
    if ((w && w.closeEmitted) || (r && r.closeEmitted)) {
      return;
    }
    if (w) {
      w.closed = true;
      w.closeEmitted = true;
    }
    if (r) {
      r.closed = true;
      r.closeEmitted = true;
    }
    const emitClose = (w ? w.emitClose : true) && (r ? r.emitClose : true);
    if (emitClose) {
      stream.emit("close");
    }
  }

  function errorOrDestroy(stream, err, _sync) {
    const r = stream._readableState;
    const w = stream._writableState;
    if ((w && w.destroyed) || (r && r.destroyed)) {
      return;
    }
    const autoDestroy = (r && r.autoDestroy) || (w && w.autoDestroy);
    if (autoDestroy) {
      stream.destroy(err);
    } else if (err) {
      err.stack;
      if (w && !w.errored) w.errored = err;
      if (r && !r.errored) r.errored = err;
      queueMicrotask(() => {
        if ((r && r.errorEmitted) || (w && w.errorEmitted)) return;
        if (r) r.errorEmitted = true;
        if (w) w.errorEmitted = true;
        stream.emit("error", err);
      });
    }
  }

  // =====================================================================
  // Duplex / Transform / PassThrough
  // =====================================================================
  class Duplex extends Readable {
    constructor(options) {
      super(options);
      // Mixin writable
      this._writableState = new WritableState(options, true);
      if (options) {
        if (typeof options.write === "function") this._write = options.write;
        if (typeof options.writev === "function") this._writev = options.writev;
        if (typeof options.final === "function") this._final = options.final;
        if (options.readable === false) {
          this._readableState.readable = false;
          this._readableState.ended = true;
          this._readableState.endEmitted = true;
        }
        if (options.writable === false) {
          this._writableState.writable = false;
          this._writableState.ending = true;
          this._writableState.ended = true;
          this._writableState.finished = true;
        }
        this.allowHalfOpen = options.allowHalfOpen !== false;
        if (options.signal) {
          addAbortSignal(options.signal, this);
        }
      } else {
        this.allowHalfOpen = true;
      }
      if (!this.allowHalfOpen) {
        this.once("end", () => {
          queueMicrotask(() => {
            if (!this._writableState.ending && !this._writableState.destroyed) {
              this.end();
            }
          });
        });
      }
    }

    get destroyed() {
      return this._readableState.destroyed || this._writableState.destroyed;
    }
    set destroyed(v) {
      this._readableState.destroyed = v;
      this._writableState.destroyed = v;
    }

    static from(src) {
      return duplexFrom(src);
    }
    static fromWeb(pair, options) {
      const readable = Readable.fromWeb(pair.readable, options);
      const writable = Writable.fromWeb(pair.writable, options);
      return duplexify(readable, writable, options);
    }
    static toWeb(duplex) {
      return {
        readable: Readable.toWeb(duplex),
        writable: Writable.toWeb(duplex),
      };
    }
  }
  // Copy Writable prototype methods onto Duplex
  for (const name of ["write", "cork", "uncork", "setDefaultEncoding", "end", "_write"]) {
    Duplex.prototype[name] = Writable.prototype[name];
  }
  for (const name of ["writable", "writableEnded", "writableFinished", "writableHighWaterMark",
    "writableLength", "writableObjectMode", "writableCorked", "writableNeedDrain", "writableAborted"]) {
    Object.defineProperty(Duplex.prototype, name, Object.getOwnPropertyDescriptor(Writable.prototype, name));
  }

  function duplexify(readable, writable, options) {
    const d = new Duplex({
      objectMode: true,
      highWaterMark: 1,
      ...options,
      read() {
        // driven by readable's data below
      },
      write(chunk, encoding, cb) {
        writable.write(chunk, encoding, cb);
      },
      final(cb) {
        writable.end(() => cb());
      },
      destroy(err, cb) {
        readable.destroy(err || undefined);
        writable.destroy(err || undefined);
        cb(err);
      },
    });
    readable.on("data", (chunk) => {
      if (!d.push(chunk)) {
        readable.pause();
      }
    });
    d._read = () => readable.resume();
    readable.on("end", () => d.push(null));
    readable.on("error", (err) => d.destroy(err));
    writable.on("error", (err) => d.destroy(err));
    return d;
  }

  function duplexFrom(src) {
    if (src && typeof src === "object" && src.readable !== undefined && typeof src.readable === "object" &&
        src.writable !== undefined && typeof src.writable === "object") {
      // { readable, writable } pair (possibly web streams)
      const readable = typeof src.readable.getReader === "function" ? Readable.fromWeb(src.readable) : src.readable;
      const writable = typeof src.writable.getWriter === "function" ? Writable.fromWeb(src.writable) : src.writable;
      return duplexify(readable, writable);
    }
    if (typeof src === "function") {
      // async function (source) => sink : transform-style
      const pass = new PassThrough({ objectMode: true });
      const out = new PassThrough({ objectMode: true });
      const result = src(createAsyncIterator(pass));
      if (result && typeof result[Symbol.asyncIterator] === "function") {
        (async () => {
          try {
            for await (const chunk of result) {
              if (!out.write(chunk)) {
                await new Promise((res) => out.once("drain", res));
              }
            }
            out.end();
          } catch (err) {
            out.destroy(err);
          }
        })();
      }
      const d = duplexify(out, pass);
      return d;
    }
    if (src && typeof src[Symbol.asyncIterator] === "function") {
      const r = readableFrom(src);
      const d = duplexify(r, new Writable({ objectMode: true, write: (c, e, cb) => cb() }));
      return d;
    }
    if (src && typeof src.then === "function") {
      return duplexFrom((async function* () {
        yield await src;
      })());
    }
    throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", "Duplex.from: unsupported source");
  }

  class Transform extends Duplex {
    constructor(options) {
      super(options);
      if (options) {
        if (typeof options.transform === "function") this._transform = options.transform;
        if (typeof options.flush === "function") this._flush = options.flush;
      }
      this._transformCallback = null;
    }

    _transform(chunk, encoding, callback) {
      throw new codes.ERR_METHOD_NOT_IMPLEMENTED("The _transform() method is not implemented");
    }

    _write(chunk, encoding, callback) {
      const rState = this._readableState;
      let called = false;
      this._transform(chunk, encoding, (err, val) => {
        if (called) {
          return;
        }
        called = true;
        if (err) {
          callback(err);
          return;
        }
        if (val !== undefined && val !== null) {
          this.push(val);
        }
        if (rState.length < rState.highWaterMark || rState.length === 0) {
          callback();
        } else {
          // backpressure: wait until readable side is drained
          this._transformCallback = callback;
        }
      });
    }

    _read(_n) {
      if (this._transformCallback) {
        const cb = this._transformCallback;
        this._transformCallback = null;
        cb();
      }
    }

    _final(callback) {
      if (typeof this._flush === "function") {
        this._flush((err, val) => {
          if (err) {
            callback(err);
            return;
          }
          if (val !== undefined && val !== null) {
            this.push(val);
          }
          this.push(null);
          callback();
        });
      } else {
        this.push(null);
        callback();
      }
    }
  }

  class PassThrough extends Transform {
    _transform(chunk, encoding, callback) {
      callback(null, chunk);
    }
  }

  // =====================================================================
  // finished / pipeline
  // =====================================================================
  function isReadableNodeStream(obj) {
    return obj instanceof Stream && typeof obj.read === "function" && obj._readableState !== undefined;
  }
  function isWritableNodeStream(obj) {
    return obj instanceof Stream && typeof obj.write === "function" && obj._writableState !== undefined;
  }

  function finished(stream, options, callback) {
    if (typeof options === "function") {
      callback = options;
      options = {};
    }
    options = options || {};
    const readable = options.readable !== false && isReadableLike(stream);
    const writable = options.writable !== false && isWritableLike(stream);

    let readableFinished = !readable || (stream._readableState && stream._readableState.endEmitted);
    let writableFinished = !writable || (stream._writableState && stream._writableState.finished);
    let done = false;

    const rState = stream._readableState;
    const wState = stream._writableState;

    const complete = (err) => {
      if (done) return;
      done = true;
      cleanup();
      callback.call(stream, err);
    };

    const onEnd = () => {
      readableFinished = true;
      if (writableFinished) complete();
    };
    const onFinish = () => {
      writableFinished = true;
      if (readableFinished) complete();
    };
    const onError = (err) => complete(err);
    const onClose = () => {
      if (readable && !readableFinished && rState && !rState.errored && !rState.endEmitted) {
        complete(new codes.ERR_STREAM_PREMATURE_CLOSE());
        return;
      }
      if (writable && !writableFinished && wState && !wState.errored && !wState.finished) {
        complete(new codes.ERR_STREAM_PREMATURE_CLOSE());
        return;
      }
      if (readableFinished && writableFinished) {
        complete();
      }
    };

    const cleanup = () => {
      stream.removeListener("end", onEnd);
      stream.removeListener("finish", onFinish);
      stream.removeListener("error", onError);
      stream.removeListener("close", onClose);
      if (abortListener) abortListener[Symbol.dispose]();
    };

    let abortListener = null;
    if (options.signal) {
      if (options.signal.aborted) {
        queueMicrotask(() => complete(abortError(options.signal.reason)));
      } else {
        const listener = () => complete(abortError(options.signal.reason));
        options.signal.addEventListener("abort", listener, { once: true });
        abortListener = { [Symbol.dispose]: () => options.signal.removeEventListener("abort", listener) };
      }
    }

    if ((rState && rState.errored) || (wState && wState.errored)) {
      queueMicrotask(() => complete((rState && rState.errored) || (wState && wState.errored)));
    } else if (readableFinished && writableFinished) {
      queueMicrotask(() => complete());
    } else if (stream.destroyed && !(rState && rState.closeEmitted) && !(wState && wState.closeEmitted)) {
      stream.once("close", onClose);
    } else if (stream.destroyed) {
      queueMicrotask(() => onClose());
    } else {
      if (readable) stream.on("end", onEnd);
      if (writable) stream.on("finish", onFinish);
      stream.on("error", onError);
      stream.on("close", onClose);
    }

    return cleanup;
  }

  function isReadableLike(s) {
    return s && (s._readableState !== undefined || typeof s.read === "function");
  }
  function isWritableLike(s) {
    return s && (s._writableState !== undefined || typeof s.write === "function");
  }

  function pipeline(...args) {
    let callback = nop;
    if (typeof args[args.length - 1] === "function" &&
        !(args.length >= 2 && isStreamCandidate(args[args.length - 1]) === false && args.length === 2)) {
      // last arg is the completion callback only if it's not a stage function...
      // A stage function takes a source; the completion callback signature is (err).
      // Node resolves: last arg is callback when it's a function AND the arg before
      // it is a stream/iterable/function. Heuristic: if the last function has been
      // popped and at least 2 stages remain, treat as callback.
    }
    let streams = args;
    let options;
    if (typeof streams[streams.length - 1] === "function" && streams.length >= 3) {
      callback = streams.pop();
    } else if (typeof streams[streams.length - 1] === "function" && streams.length === 2 &&
               isStreamCandidate(streams[0]) && !isStreamCandidate(streams[1])) {
      // could be (source, dest-fn) with no callback — Node requires callback for
      // the callback API; if last fn expects a source arg treat as stage
      if (streams[1].length === 0) {
        callback = streams.pop();
      }
    }
    if (Array.isArray(streams[0]) && streams.length >= 1 && typeof streams[1] !== "object") {
      // pipeline([a, b, c], cb)
      const arr = streams[0];
      streams = arr;
    }
    if (streams.length > 0 && streams[streams.length - 1] !== null &&
        typeof streams[streams.length - 1] === "object" &&
        !isStreamCandidate(streams[streams.length - 1]) &&
        streams[streams.length - 1].signal !== undefined) {
      options = streams.pop();
    }
    if (streams.length < 2) {
      throw makeError(TypeError, "ERR_MISSING_ARGS", "The streams argument is required");
    }

    let error;
    let finishedCount = 0;
    const allStreams = [];
    let done = false;

    const finishOne = (err) => {
      finishedCount -= 1;
      if (err && !error) {
        error = err;
      }
      if (err) {
        for (let i = allStreams.length - 1; i >= 0; i -= 1) {
          const s = allStreams[i];
          if (typeof s.destroy === "function" && !s.destroyed) {
            s.on("error", nop); // secondary errors are reported via the callback
            s.destroy(err);
          }
        }
      }
      if (finishedCount === 0 && !done) {
        done = true;
        callback(error, lastValue);
      }
    };

    let lastValue;
    let prev = null;

    // Normalize stages into node streams
    const stages = streams.map((stage, i) => {
      const isFirst = i === 0;
      const isLast = i === streams.length - 1;
      if (isStreamCandidate(stage)) {
        if (typeof stage.getReader === "function") {
          return Readable.fromWeb(stage);
        }
        if (typeof stage.getWriter === "function") {
          return Writable.fromWeb(stage);
        }
        return stage;
      }
      if (isFirst) {
        if (typeof stage === "function") {
          const res = stage();
          return readableFrom(res);
        }
        return readableFrom(stage);
      }
      if (typeof stage === "function") {
        if (isLast) {
          return { __sinkFn: stage };
        }
        return { __transformFn: stage };
      }
      throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", "pipeline stage " + i + " is not a stream, iterable, or function");
    });

    // Build the chain
    let source = stages[0];
    allStreams.push(source);
    for (let i = 1; i < stages.length; i += 1) {
      const stage = stages[i];
      const isLast = i === stages.length - 1;
      if (stage.__transformFn) {
        const fn = stage.__transformFn;
        const iterOut = fn(createAsyncIterator(source), options);
        source = readableFrom(iterOut);
        allStreams.push(source);
        continue;
      }
      if (stage.__sinkFn) {
        const fn = stage.__sinkFn;
        finishedCount += 1;
        Promise.resolve(fn(createAsyncIterator(source), options))
          .then((val) => {
            lastValue = val;
            finishOne();
          }, (err) => finishOne(err));
        source = null;
        continue;
      }
      // real stream
      allStreams.push(stage);
      finishedCount += 1;
      finished(stage, { readable: !isLast || isReadableLike(stage) && stage._readableState !== undefined && !isLast, writable: true }, (err) => {
        afterClosed(stage, () => finishOne(err));
      });
      // For intermediate duplex: track its readable side end too via next stage
      source.pipe(stage, { end: true });
      source = stage;
    }

    if (source && source !== stages[0]) {
      // final stage is a stream — completion driven by 'finished' above; but if
      // the final stream is a readable-only sink (rare), consume it.
    }
    // Track the source's errors too
    finishedCount += 1;
    finished(stages[0], { readable: true, writable: false }, (err) => {
      afterClosed(stages[0], () => finishOne(err));
    });

    function afterClosed(stage, cb) {
      const r = stage._readableState;
      const w = stage._writableState;
      const closed = (r && r.closeEmitted) || (w && w.closeEmitted) ||
        (!r && !w) || !(stage.destroyed || (r && r.errored) || (w && w.errored));
      if (closed) {
        cb();
        return;
      }
      stage.once("close", cb);
    }

    return stages[stages.length - 1].__sinkFn ? undefined : stages[stages.length - 1];
  }

  function isStreamCandidate(obj) {
    return obj !== null && typeof obj === "object" &&
      (obj instanceof Stream ||
       typeof obj.pipe === "function" ||
       typeof obj.write === "function" && typeof obj.end === "function" ||
       typeof obj.getReader === "function" ||
       typeof obj.getWriter === "function");
  }

  function addAbortSignal(signal, stream) {
    if (!signal || typeof signal.aborted !== "boolean") {
      throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", 'The "signal" argument must be an instance of AbortSignal');
    }
    const onAbort = () => {
      stream.destroy(abortError(signal.reason));
    };
    if (signal.aborted) {
      queueMicrotask(onAbort);
    } else {
      signal.addEventListener("abort", onAbort, { once: true });
      const cleanup = () => signal.removeEventListener("abort", onAbort);
      stream.once("close", cleanup);
    }
    return stream;
  }

  function composeImpl(...streams) {
    if (streams.length === 0) {
      throw makeError(TypeError, "ERR_MISSING_ARGS", "The streams argument is required");
    }
    if (streams.length === 1) {
      return duplexFrom(streams[0]);
    }
    const first = streams[0];
    const last = streams[streams.length - 1];
    const pass = new PassThrough({ objectMode: true });
    let current = isStreamCandidate(first) && typeof first.pipe === "function" ? first : readableFrom(first);
    for (let i = 1; i < streams.length; i += 1) {
      const s = streams[i];
      if (typeof s === "function") {
        current = readableFrom(s(createAsyncIterator(current)));
      } else {
        current.pipe(s);
        current = s;
      }
    }
    const writableSide = isWritableLike(first) && typeof first.write === "function"
      ? first
      : new Writable({ objectMode: true, write: (c, e, cb) => cb() });
    return duplexify(current, writableSide);
  }

  // =====================================================================
  // exports
  // =====================================================================
  function isDestroyed(stream) {
    return !!(stream && stream.destroyed);
  }

  Stream.Readable = Readable;
  Stream.Writable = Writable;
  Stream.Duplex = Duplex;
  Stream.Transform = Transform;
  Stream.PassThrough = PassThrough;
  Stream.Stream = Stream;
  Stream.pipeline = pipeline;
  Stream.finished = finished;
  Stream.addAbortSignal = addAbortSignal;
  Stream.compose = composeImpl;
  Stream.destroy = (stream, err) => {
    if (stream && typeof stream.destroy === "function") stream.destroy(err);
    return stream;
  };
  Stream.isReadable = (s) => {
    if (!s) return false;
    if (s._readableState === undefined && typeof s.read !== "function") return false;
    const st = s._readableState;
    if (!st) return null;
    return !st.destroyed && !st.endEmitted && st.readable !== false;
  };
  Stream.isWritable = (s) => {
    if (!s) return false;
    if (s._writableState === undefined && typeof s.write !== "function") return false;
    const st = s._writableState;
    if (!st) return null;
    return !st.destroyed && !st.ending && st.writable !== false;
  };
  Stream.isErrored = (s) => !!(s && (
    (s._readableState && s._readableState.errored) ||
    (s._writableState && s._writableState.errored)
  ));
  Stream.isDisturbed = Readable.isDisturbed;
  Stream.getDefaultHighWaterMark = getDefaultHighWaterMark;
  Stream.setDefaultHighWaterMark = setDefaultHighWaterMark;
  Stream._isUint8Array = (v) => Object.prototype.toString.call(v) === "[object Uint8Array]";
  Stream._uint8ArrayToBuffer = (v) => require("buffer").Buffer.from(v.buffer, v.byteOffset, v.byteLength);
  Stream.promises = null; // set lazily below via getter

  Object.defineProperty(Stream, "promises", {
    configurable: true,
    enumerable: true,
    get() {
      return require("stream/promises");
    },
  });

  module.exports = Stream;
  module.exports.default = Stream;
});

__nanoNodeRegister("stream/promises", function (module, exports, require) {
  const { pipeline, finished } = require("stream");

  function pipelinePromise(...streams) {
    let options;
    if (streams.length > 0 && streams[streams.length - 1] !== null &&
        typeof streams[streams.length - 1] === "object" &&
        typeof streams[streams.length - 1].pipe !== "function" &&
        typeof streams[streams.length - 1].getReader !== "function" &&
        typeof streams[streams.length - 1][Symbol.asyncIterator] !== "function" &&
        typeof streams[streams.length - 1] !== "function") {
      options = streams.pop();
    }
    return new Promise((resolve, reject) => {
      const cb = (err, value) => (err ? reject(err) : resolve(value));
      if (options) {
        pipeline(...streams, options, cb);
      } else {
        pipeline(...streams, cb);
      }
    });
  }

  function finishedPromise(stream, options) {
    return new Promise((resolve, reject) => {
      finished(stream, options || {}, (err) => (err ? reject(err) : resolve()));
    });
  }

  module.exports = { pipeline: pipelinePromise, finished: finishedPromise };
});

__nanoNodeRegister("stream/consumers", function (module, exports, require) {
  async function* iterate(stream) {
    if (stream && typeof stream.getReader === "function") {
      const reader = stream.getReader();
      try {
        while (true) {
          const { value, done } = await reader.read();
          if (done) break;
          yield value;
        }
      } finally {
        reader.releaseLock();
      }
    } else {
      yield* stream;
    }
  }

  async function collectChunks(stream) {
    const chunks = [];
    for await (const chunk of iterate(stream)) {
      chunks.push(chunk);
    }
    return chunks;
  }

  async function buffer(stream) {
    const { Buffer } = require("buffer");
    const chunks = await collectChunks(stream);
    return Buffer.concat(chunks.map((c) =>
      typeof c === "string" ? Buffer.from(c) : Buffer.from(c.buffer ? new Uint8Array(c.buffer, c.byteOffset, c.byteLength) : c)
    ));
  }

  async function arrayBuffer(stream) {
    const buf = await buffer(stream);
    return buf.buffer.slice(buf.byteOffset, buf.byteOffset + buf.byteLength);
  }

  async function text(stream) {
    const buf = await buffer(stream);
    return buf.toString("utf8");
  }

  async function json(stream) {
    return JSON.parse(await text(stream));
  }

  async function blob(stream) {
    const chunks = await collectChunks(stream);
    return new Blob(chunks);
  }

  module.exports = { arrayBuffer, blob, buffer, json, text };
});

__nanoNodeRegister("stream/web", function (module, exports, require) {
  const g = globalThis;

  class ByteLengthQueuingStrategy {
    constructor(init) {
      this.highWaterMark = init.highWaterMark;
    }
    size(chunk) {
      return chunk.byteLength;
    }
  }
  class CountQueuingStrategy {
    constructor(init) {
      this.highWaterMark = init.highWaterMark;
    }
    size() {
      return 1;
    }
  }

  const web = require("internal/web");

  module.exports = {
    ReadableStream: g.ReadableStream,
    ReadableStreamDefaultReader: g.ReadableStreamDefaultReader,
    ReadableStreamBYOBReader: g.ReadableStreamBYOBReader,
    ReadableStreamBYOBRequest: g.ReadableStreamBYOBRequest,
    ReadableByteStreamController: g.ReadableByteStreamController,
    ReadableStreamDefaultController: g.ReadableStreamDefaultController,
    TransformStream: g.TransformStream || web.TransformStream,
    TransformStreamDefaultController: g.TransformStreamDefaultController,
    WritableStream: g.WritableStream,
    WritableStreamDefaultWriter: g.WritableStreamDefaultWriter,
    WritableStreamDefaultController: g.WritableStreamDefaultController,
    ByteLengthQueuingStrategy: g.ByteLengthQueuingStrategy || ByteLengthQueuingStrategy,
    CountQueuingStrategy: g.CountQueuingStrategy || CountQueuingStrategy,
    TextEncoderStream: web.TextEncoderStream,
    TextDecoderStream: web.TextDecoderStream,
    CompressionStream: web.CompressionStream,
    DecompressionStream: web.DecompressionStream,
  };
});
