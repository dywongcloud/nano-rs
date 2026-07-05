"use strict";
// node:timers, node:timers/promises — Node-shaped Timeout/Immediate handles.
__nanoNodeRegister("timers", function (module, exports, require) {
  const { makeError } = require("internal/errors");

  const kRef = Symbol("ref");
  const kId = Symbol("id");

  class Timeout {
    constructor(rawId) {
      this[kId] = rawId;
      this[kRef] = true;
      this._destroyed = false;
      this._idleTimeout = -1;
    }
    ref() {
      this[kRef] = true;
      return this;
    }
    unref() {
      this[kRef] = false;
      return this;
    }
    hasRef() {
      return this[kRef];
    }
    refresh() {
      return this;
    }
    close() {
      clearTimeout(this[kId]);
      this._destroyed = true;
      return this;
    }
    [Symbol.toPrimitive]() {
      return this[kId];
    }
    [Symbol.dispose]() {
      this.close();
    }
  }

  class Immediate {
    constructor(rawId) {
      this[kId] = rawId;
      this[kRef] = true;
      this._destroyed = false;
    }
    ref() {
      this[kRef] = true;
      return this;
    }
    unref() {
      this[kRef] = false;
      return this;
    }
    hasRef() {
      return this[kRef];
    }
    [Symbol.toPrimitive]() {
      return this[kId];
    }
    [Symbol.dispose]() {
      clearImmediate(this);
    }
  }

  function rawIdOf(handle) {
    if (handle instanceof Timeout || handle instanceof Immediate) {
      return handle[kId];
    }
    return handle;
  }

  function setTimeoutWrapped(callback, delay, ...args) {
    if (typeof callback !== "function") {
      throw makeError(TypeError, "ERR_INVALID_CALLBACK", "Callback must be a function. Received " + typeof callback);
    }
    const rawId = globalThis.setTimeout(() => callback(...args), delay);
    return new Timeout(rawId);
  }

  function clearTimeoutWrapped(handle) {
    if (handle === undefined || handle === null) return;
    globalThis.clearTimeout(rawIdOf(handle));
  }

  function setIntervalWrapped(callback, delay, ...args) {
    if (typeof callback !== "function") {
      throw makeError(TypeError, "ERR_INVALID_CALLBACK", "Callback must be a function. Received " + typeof callback);
    }
    const rawId = globalThis.setInterval(() => callback(...args), delay);
    return new Timeout(rawId);
  }

  function clearIntervalWrapped(handle) {
    if (handle === undefined || handle === null) return;
    globalThis.clearInterval(rawIdOf(handle));
  }

  // setImmediate: FIFO queue drained via setTimeout(0), matching Node's
  // ordering guarantee (immediates run in registration order).
  let immediateQueue = [];
  let immediateScheduled = false;
  let immediateNextId = 1;

  function drainImmediates() {
    immediateScheduled = false;
    const batch = immediateQueue;
    immediateQueue = [];
    for (const entry of batch) {
      if (!entry.cleared) {
        entry.callback(...entry.args);
      }
    }
  }

  function setImmediateWrapped(callback, ...args) {
    if (typeof callback !== "function") {
      throw makeError(TypeError, "ERR_INVALID_CALLBACK", "Callback must be a function. Received " + typeof callback);
    }
    const entry = { id: immediateNextId++, callback, args, cleared: false };
    immediateQueue.push(entry);
    if (!immediateScheduled) {
      immediateScheduled = true;
      globalThis.setTimeout(drainImmediates, 0);
    }
    return new Immediate(entry);
  }

  function clearImmediateWrapped(handle) {
    if (handle === undefined || handle === null) return;
    const entry = handle instanceof Immediate ? handle[kId] : handle;
    if (entry && typeof entry === "object") {
      entry.cleared = true;
    }
  }

  module.exports = {
    setTimeout: setTimeoutWrapped,
    clearTimeout: clearTimeoutWrapped,
    setInterval: setIntervalWrapped,
    clearInterval: clearIntervalWrapped,
    setImmediate: setImmediateWrapped,
    clearImmediate: clearImmediateWrapped,
    active(handle) {
      return handle;
    },
    unenroll() {},
    enroll(handle) {
      return handle;
    },
    Timeout,
    Immediate,
    __installGlobals(g) {
      if (typeof g.setImmediate !== "function") {
        g.setImmediate = setImmediateWrapped;
      }
      if (typeof g.clearImmediate !== "function") {
        g.clearImmediate = clearImmediateWrapped;
      }
      if (typeof g.queueMicrotask !== "function") {
        g.queueMicrotask = (cb) => Promise.resolve().then(cb);
      }
    },
  };
});

__nanoNodeRegister("timers/promises", function (module, exports, require) {
  const { makeError } = require("internal/errors");
  const timers = require("timers");

  function abortError(reason) {
    const err = new Error("The operation was aborted");
    err.name = "AbortError";
    err.code = "ABORT_ERR";
    if (reason !== undefined) err.cause = reason;
    return err;
  }

  function checkSignal(signal) {
    if (signal !== undefined && (typeof signal !== "object" || typeof signal.aborted !== "boolean")) {
      throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", 'The "signal" argument must be an instance of AbortSignal');
    }
  }

  function timeout(delay, value, options) {
    const signal = options && options.signal;
    checkSignal(signal);
    return new Promise((resolve, reject) => {
      if (signal && signal.aborted) {
        reject(abortError(signal.reason));
        return;
      }
      const handle = timers.setTimeout(() => {
        cleanup();
        resolve(value);
      }, delay);
      let onAbort;
      function cleanup() {
        if (signal && onAbort) signal.removeEventListener("abort", onAbort);
      }
      if (signal) {
        onAbort = () => {
          timers.clearTimeout(handle);
          cleanup();
          reject(abortError(signal.reason));
        };
        signal.addEventListener("abort", onAbort, { once: true });
      }
    });
  }

  function immediate(value, options) {
    const signal = options && options.signal;
    checkSignal(signal);
    return new Promise((resolve, reject) => {
      if (signal && signal.aborted) {
        reject(abortError(signal.reason));
        return;
      }
      const handle = timers.setImmediate(() => {
        cleanup();
        resolve(value);
      });
      let onAbort;
      function cleanup() {
        if (signal && onAbort) signal.removeEventListener("abort", onAbort);
      }
      if (signal) {
        onAbort = () => {
          timers.clearImmediate(handle);
          cleanup();
          reject(abortError(signal.reason));
        };
        signal.addEventListener("abort", onAbort, { once: true });
      }
    });
  }

  async function* interval(delay, value, options) {
    const signal = options && options.signal;
    checkSignal(signal);
    while (true) {
      await timeout(delay, undefined, { signal });
      yield value;
    }
  }

  const scheduler = {
    wait(delay, options) {
      return timeout(delay, undefined, options);
    },
    yield() {
      return immediate(undefined);
    },
  };

  module.exports = { setTimeout: timeout, setImmediate: immediate, setInterval: interval, scheduler };
});
