"use strict";
// events — Node.js v22 EventEmitter (node:events) for the NANO runtime.
//
// Pure in-isolate computation; nothing here is sandbox-restricted. Two
// documented divergences from Node v22:
//   - EventEmitterAsyncResource uses a minimal in-isolate AsyncResource
//     stand-in (monotonic ids, pass-through runInAsyncScope) because there
//     is no async_hooks host support.
//   - getEventListeners(target) for a web EventTarget throws
//     ERR_UNSUPPORTED_OPERATION: host EventTarget listener lists are not
//     introspectable from this layer.
__nanoNodeRegister("events", function (module, exports, require) {
  const { codes, unsupported } = require("internal/errors");
  const {
    ERR_INVALID_ARG_TYPE,
    ERR_OUT_OF_RANGE,
    ERR_UNHANDLED_ERROR,
    ERR_INVALID_THIS,
  } = codes;

  // ---------------------------------------------------------------------
  // Symbols
  // ---------------------------------------------------------------------
  const kRejection = Symbol.for("nodejs.rejection");
  const kCapture = Symbol("kCapture");
  const kErrorMonitor = Symbol("events.errorMonitor");
  const kMaxEventTargetListeners = Symbol("events.maxEventTargetListeners");
  const kMaxEventTargetListenersWarned =
    Symbol("events.maxEventTargetListenersWarned");
  const kAsyncResource = Symbol("kAsyncResource");
  const kEventEmitter = Symbol("kEventEmitter");
  const kDispose =
    typeof Symbol.dispose === "symbol" ? Symbol.dispose : Symbol.for("nodejs.dispose");

  // %AsyncIteratorPrototype% — reached through an async generator instance
  // (no dynamic code generation involved).
  const AsyncIteratorPrototype = Object.getPrototypeOf(
    Object.getPrototypeOf(Object.getPrototypeOf((async function* () {})()))
  );

  // ---------------------------------------------------------------------
  // Validation / message helpers (match Node's error text closely)
  // ---------------------------------------------------------------------
  function simpleInspect(value) {
    if (value === null) return "null";
    if (value === undefined) return "undefined";
    const t = typeof value;
    if (t === "string") return "'" + value + "'";
    if (t === "bigint") return String(value) + "n";
    if (t === "symbol") return value.toString();
    if (t === "function") {
      return value.name ? "[Function: " + value.name + "]" : "[Function (anonymous)]";
    }
    return String(value);
  }

  function inspectValue(value) {
    try {
      const util = require("util");
      if (util !== null && typeof util === "object" && typeof util.inspect === "function") {
        return util.inspect(value);
      }
    } catch (_e) {
      // util not registered in this build — fall through to the simple form
    }
    return simpleInspect(value);
  }

  function specificType(value) {
    if (value === null || value === undefined) return String(value);
    if (typeof value === "function" && value.name) return "function " + value.name;
    if (typeof value === "object") {
      if (value.constructor && value.constructor.name) {
        return "an instance of " + value.constructor.name;
      }
      return simpleInspect(value);
    }
    let inspected = simpleInspect(value);
    if (inspected.length > 28) inspected = inspected.slice(0, 25) + "...";
    return "type " + typeof value + " (" + inspected + ")";
  }

  function oxfordOr(items) {
    if (items.length === 1) return items[0];
    if (items.length === 2) return items[0] + " or " + items[1];
    return items.slice(0, -1).join(", ") + ", or " + items[items.length - 1];
  }

  function invalidArgType(name, expected, value) {
    const list = Array.isArray(expected) ? expected : [expected];
    const types = [];
    const instances = [];
    for (let i = 0; i < list.length; i++) {
      if (/^[a-z]/.test(list[i])) types.push(list[i]);
      else instances.push(list[i]);
    }
    const parts = [];
    if (types.length > 0) parts.push("of type " + oxfordOr(types));
    if (instances.length > 0) parts.push("an instance of " + oxfordOr(instances));
    const kind = name.indexOf(".") === -1 ? "argument" : "property";
    return new ERR_INVALID_ARG_TYPE(
      'The "' + name + '" ' + kind + " must be " + parts.join(" or ") +
      ". Received " + specificType(value)
    );
  }

  function outOfRange(name, range, value) {
    return new ERR_OUT_OF_RANGE(
      'The value of "' + name + '" is out of range. It must be ' + range +
      ". Received " + String(value)
    );
  }

  function checkListener(listener) {
    if (typeof listener !== "function") {
      throw invalidArgType("listener", "function", listener);
    }
  }

  function validateBoolean(value, name) {
    if (typeof value !== "boolean") throw invalidArgType(name, "boolean", value);
  }

  function validateNonNegativeNumber(value, name) {
    if (typeof value !== "number") throw invalidArgType(name, "number", value);
    if (value < 0 || Number.isNaN(value)) throw outOfRange(name, ">= 0", value);
  }

  function validateAbortSignal(signal, name) {
    if (signal !== undefined &&
        (signal === null || typeof signal !== "object" || !("aborted" in signal))) {
      throw invalidArgType(name, "AbortSignal", signal);
    }
  }

  function scheduleMicrotask(fn) {
    if (typeof queueMicrotask === "function") {
      queueMicrotask(fn);
      return;
    }
    Promise.resolve().then(fn);
  }

  class AbortError extends Error {
    constructor(message, options) {
      super(message === undefined ? "The operation was aborted" : message, options);
      this.code = "ABORT_ERR";
      this.name = "AbortError";
    }
  }

  function isEventTarget(value) {
    if (value === null || typeof value !== "object") return false;
    const ET = globalThis.EventTarget;
    if (typeof ET === "function" && value instanceof ET) return true;
    return typeof value.addEventListener === "function" &&
      typeof value.removeEventListener === "function" &&
      typeof value.dispatchEvent === "function";
  }

  // ---------------------------------------------------------------------
  // EventEmitter core
  // ---------------------------------------------------------------------
  let defaultMaxListeners = 10;

  function EventEmitter(opts) {
    EventEmitter.init.call(this, opts);
  }

  EventEmitter.prototype._events = undefined;
  EventEmitter.prototype._eventsCount = 0;
  EventEmitter.prototype._maxListeners = undefined;

  Object.defineProperty(EventEmitter.prototype, kCapture, {
    value: false,
    writable: true,
    enumerable: false,
  });

  EventEmitter.init = function init(opts) {
    if (this._events === undefined ||
        this._events === Object.getPrototypeOf(this)._events) {
      this._events = { __proto__: null };
      this._eventsCount = 0;
    }
    this._maxListeners = this._maxListeners || undefined;
    if (opts && opts.captureRejections) {
      validateBoolean(opts.captureRejections, "options.captureRejections");
      this[kCapture] = Boolean(opts.captureRejections);
    } else {
      // Own property so per-instance state is stable even if the
      // prototype default is flipped later (matches Node).
      this[kCapture] = EventEmitter.prototype[kCapture];
    }
  };

  function _getMaxListeners(that) {
    return that._maxListeners === undefined ? defaultMaxListeners : that._maxListeners;
  }

  function arrayClone(arr) {
    return arr.slice();
  }

  function unwrapListeners(arr) {
    const ret = new Array(arr.length);
    for (let i = 0; i < ret.length; ++i) {
      ret[i] = arr[i].listener || arr[i];
    }
    return ret;
  }

  function targetDisplayName(target) {
    if (target !== null && typeof target === "object" &&
        target.constructor && target.constructor.name) {
      return target.constructor.name;
    }
    return "EventEmitter";
  }

  function emitMaxListenersWarning(target, type, count, max) {
    const warning = new Error(
      "Possible EventEmitter memory leak detected. " + count + " " +
      String(type) + " listeners added to [" + targetDisplayName(target) +
      "]. MaxListeners is " + max +
      ". Use emitter.setMaxListeners() to increase limit"
    );
    warning.name = "MaxListenersExceededWarning";
    warning.emitter = target;
    warning.type = type;
    warning.count = count;
    const proc = globalThis.process;
    if (proc !== undefined && proc !== null && typeof proc.emitWarning === "function") {
      proc.emitWarning(warning);
    } else {
      console.warn(warning.name + ": " + warning.message);
    }
  }

  function addCatch(that, promise, type, args) {
    if (!that[kCapture]) return;
    try {
      const then = promise.then;
      if (typeof then === "function") {
        then.call(promise, undefined, function (err) {
          // Node uses process.nextTick; a microtask is the closest
          // in-isolate equivalent.
          scheduleMicrotask(function () {
            emitUnhandledRejectionOrErr(that, err, type, args);
          });
        });
      }
    } catch (err) {
      that.emit("error", err);
    }
  }

  function emitUnhandledRejectionOrErr(ee, err, type, args) {
    if (typeof ee[kRejection] === "function") {
      ee[kRejection](err, type, ...args);
    } else {
      // Disable capture while re-emitting to avoid infinite recursion.
      const prev = ee[kCapture];
      try {
        ee[kCapture] = false;
        ee.emit("error", err);
      } finally {
        ee[kCapture] = prev;
      }
    }
  }

  EventEmitter.prototype.setMaxListeners = function setMaxListeners(n) {
    validateNonNegativeNumber(n, "setMaxListeners");
    this._maxListeners = n;
    return this;
  };

  EventEmitter.prototype.getMaxListeners = function getMaxListeners() {
    return _getMaxListeners(this);
  };

  EventEmitter.prototype.emit = function emit(type, ...args) {
    let doError = type === "error";

    const events = this._events;
    if (events !== undefined) {
      if (doError && events[kErrorMonitor] !== undefined) {
        this.emit(kErrorMonitor, ...args);
      }
      doError = doError && events.error === undefined;
    } else if (!doError) {
      return false;
    }

    if (doError) {
      let er;
      if (args.length > 0) er = args[0];
      // Realm-robust Error detection: instanceof plus structural check so
      // errors created in another realm (e.g. host-created) still rethrow.
      if (er instanceof Error ||
          (er !== null && typeof er === "object" &&
           typeof er.message === "string" && typeof er.stack === "string")) {
        throw er; // Unhandled 'error' event
      }
      const err = new ERR_UNHANDLED_ERROR(
        "Unhandled error. (" + inspectValue(er) + ")"
      );
      err.context = er;
      throw err; // Unhandled 'error' event
    }

    const handler = events[type];
    if (handler === undefined) return false;

    if (typeof handler === "function") {
      const result = handler.apply(this, args);
      if (result !== undefined && result !== null) {
        addCatch(this, result, type, args);
      }
    } else {
      const len = handler.length;
      const listeners = arrayClone(handler);
      for (let i = 0; i < len; ++i) {
        const result = listeners[i].apply(this, args);
        if (result !== undefined && result !== null) {
          addCatch(this, result, type, args);
        }
      }
    }
    return true;
  };

  function _addListener(target, type, listener, prepend) {
    checkListener(listener);

    let events = target._events;
    let existing;
    if (events === undefined) {
      events = target._events = { __proto__: null };
      target._eventsCount = 0;
    } else {
      // Emit 'newListener' first so it does not observe the new listener.
      if (events.newListener !== undefined) {
        target.emit("newListener", type,
          listener.listener !== undefined ? listener.listener : listener);
        // 'newListener' handlers may mutate _events.
        events = target._events;
      }
      existing = events[type];
    }

    if (existing === undefined) {
      events[type] = listener;
      ++target._eventsCount;
    } else {
      if (typeof existing === "function") {
        existing = events[type] =
          prepend ? [listener, existing] : [existing, listener];
      } else if (prepend) {
        existing.unshift(listener);
      } else {
        existing.push(listener);
      }

      const m = _getMaxListeners(target);
      if (m > 0 && existing.length > m && !existing.warned) {
        existing.warned = true;
        emitMaxListenersWarning(target, type, existing.length, m);
      }
    }
    return target;
  }

  EventEmitter.prototype.addListener = function addListener(type, listener) {
    return _addListener(this, type, listener, false);
  };
  EventEmitter.prototype.on = EventEmitter.prototype.addListener;

  EventEmitter.prototype.prependListener =
    function prependListener(type, listener) {
      return _addListener(this, type, listener, true);
    };

  function onceWrapper() {
    if (!this.fired) {
      this.target.removeListener(this.type, this.wrapFn);
      this.fired = true;
      if (arguments.length === 0) return this.listener.call(this.target);
      return this.listener.apply(this.target, arguments);
    }
    return undefined;
  }

  function _onceWrap(target, type, listener) {
    const state = { fired: false, wrapFn: undefined, target, type, listener };
    const wrapped = onceWrapper.bind(state);
    wrapped.listener = listener;
    state.wrapFn = wrapped;
    return wrapped;
  }

  EventEmitter.prototype.once = function once(type, listener) {
    checkListener(listener);
    this.on(type, _onceWrap(this, type, listener));
    return this;
  };

  EventEmitter.prototype.prependOnceListener =
    function prependOnceListener(type, listener) {
      checkListener(listener);
      this.prependListener(type, _onceWrap(this, type, listener));
      return this;
    };

  EventEmitter.prototype.removeListener =
    function removeListener(type, listener) {
      checkListener(listener);

      const events = this._events;
      if (events === undefined) return this;

      const list = events[type];
      if (list === undefined) return this;

      if (list === listener || list.listener === listener) {
        this._eventsCount -= 1;
        if (this._eventsCount === 0) {
          this._events = { __proto__: null };
        } else {
          delete events[type];
        }
        if (events.removeListener !== undefined) {
          this.emit("removeListener", type, list.listener || listener);
        }
      } else if (typeof list !== "function") {
        let position = -1;
        for (let i = list.length - 1; i >= 0; i--) {
          if (list[i] === listener || list[i].listener === listener) {
            position = i;
            break;
          }
        }
        if (position < 0) return this;

        if (position === 0) list.shift();
        else list.splice(position, 1);

        if (list.length === 1) events[type] = list[0];

        if (events.removeListener !== undefined) {
          this.emit("removeListener", type, listener);
        }
      }
      return this;
    };
  EventEmitter.prototype.off = EventEmitter.prototype.removeListener;

  EventEmitter.prototype.removeAllListeners =
    function removeAllListeners(type) {
      const events = this._events;
      if (events === undefined) return this;

      // No 'removeListener' listeners: fast path, no per-listener events.
      if (events.removeListener === undefined) {
        if (arguments.length === 0) {
          this._events = { __proto__: null };
          this._eventsCount = 0;
        } else if (events[type] !== undefined) {
          if (--this._eventsCount === 0) this._events = { __proto__: null };
          else delete events[type];
        }
        return this;
      }

      // Emit 'removeListener' for every listener; 'removeListener' last.
      if (arguments.length === 0) {
        for (const key of Reflect.ownKeys(events)) {
          if (key === "removeListener") continue;
          this.removeAllListeners(key);
        }
        this.removeAllListeners("removeListener");
        this._events = { __proto__: null };
        this._eventsCount = 0;
        return this;
      }

      const listeners = events[type];
      if (typeof listeners === "function") {
        this.removeListener(type, listeners);
      } else if (listeners !== undefined) {
        for (let i = listeners.length - 1; i >= 0; i--) {
          this.removeListener(type, listeners[i]);
        }
      }
      return this;
    };

  function _listeners(target, type, unwrap) {
    const events = target._events;
    if (events === undefined) return [];
    const evlistener = events[type];
    if (evlistener === undefined) return [];
    if (typeof evlistener === "function") {
      return unwrap ? [evlistener.listener || evlistener] : [evlistener];
    }
    return unwrap ? unwrapListeners(evlistener) : arrayClone(evlistener);
  }

  EventEmitter.prototype.listeners = function listeners(type) {
    return _listeners(this, type, true);
  };

  EventEmitter.prototype.rawListeners = function rawListeners(type) {
    return _listeners(this, type, false);
  };

  EventEmitter.prototype.listenerCount =
    function listenerCount(type, listener) {
      const events = this._events;
      if (events !== undefined) {
        const evlistener = events[type];
        if (typeof evlistener === "function") {
          if (listener !== undefined && listener !== null) {
            return listener === evlistener || listener === evlistener.listener ? 1 : 0;
          }
          return 1;
        } else if (evlistener !== undefined) {
          if (listener !== undefined && listener !== null) {
            let matching = 0;
            for (let i = 0; i < evlistener.length; i++) {
              if (evlistener[i] === listener || evlistener[i].listener === listener) {
                matching++;
              }
            }
            return matching;
          }
          return evlistener.length;
        }
      }
      return 0;
    };

  EventEmitter.prototype.eventNames = function eventNames() {
    return this._eventsCount > 0 ? Reflect.ownKeys(this._events) : [];
  };

  // ---------------------------------------------------------------------
  // Static helpers
  // ---------------------------------------------------------------------
  function staticListenerCount(emitter, type) {
    if (emitter !== null && emitter !== undefined &&
        typeof emitter.listenerCount === "function") {
      return emitter.listenerCount(type);
    }
    return EventEmitter.prototype.listenerCount.call(emitter, type);
  }

  function getEventListeners(emitterOrTarget, type) {
    // Matches Node v22: unguarded property access (a plain TypeError on
    // null/undefined, exactly as Node throws) and unwrapped listeners.
    if (typeof emitterOrTarget.listeners === "function") {
      return emitterOrTarget.listeners(type);
    }
    if (isEventTarget(emitterOrTarget)) {
      // Host/web EventTarget listener lists are not introspectable from
      // this layer (Node reads private internals) — fail loudly.
      throw unsupported("events.getEventListeners(EventTarget)");
    }
    throw invalidArgType("emitter", ["EventEmitter", "EventTarget"], emitterOrTarget);
  }

  function getMaxListenersStatic(emitterOrTarget) {
    if (emitterOrTarget !== null && emitterOrTarget !== undefined &&
        typeof emitterOrTarget.getMaxListeners === "function") {
      return _getMaxListeners(emitterOrTarget);
    }
    if (isEventTarget(emitterOrTarget)) {
      const n = emitterOrTarget[kMaxEventTargetListeners];
      return n === undefined || n === null ? defaultMaxListeners : n;
    }
    throw invalidArgType("emitter", ["EventEmitter", "EventTarget"], emitterOrTarget);
  }

  function setMaxListenersStatic(n = defaultMaxListeners, ...eventTargets) {
    validateNonNegativeNumber(n, "setMaxListeners");
    if (eventTargets.length === 0) {
      defaultMaxListeners = n;
      return;
    }
    for (let i = 0; i < eventTargets.length; i++) {
      const target = eventTargets[i];
      if (isEventTarget(target)) {
        target[kMaxEventTargetListeners] = n;
        target[kMaxEventTargetListenersWarned] = false;
      } else if (target !== null && target !== undefined &&
                 typeof target.setMaxListeners === "function") {
        target.setMaxListeners(n);
      } else {
        throw invalidArgType("eventTargets", ["EventEmitter", "EventTarget"], target);
      }
    }
  }

  function eventTargetAgnosticAddListener(emitter, name, listener, flags) {
    if (emitter !== null && emitter !== undefined &&
        typeof emitter.on === "function") {
      if (flags !== undefined && flags !== null && flags.once) {
        emitter.once(name, listener);
      } else {
        emitter.on(name, listener);
      }
    } else if (emitter !== null && emitter !== undefined &&
               typeof emitter.addEventListener === "function") {
      emitter.addEventListener(name, listener, flags);
    } else {
      throw invalidArgType("emitter", "EventEmitter", emitter);
    }
  }

  function eventTargetAgnosticRemoveListener(emitter, name, listener, flags) {
    if (emitter !== null && emitter !== undefined &&
        typeof emitter.removeListener === "function") {
      emitter.removeListener(name, listener);
    } else if (emitter !== null && emitter !== undefined &&
               typeof emitter.removeEventListener === "function") {
      emitter.removeEventListener(name, listener, flags);
    } else {
      throw invalidArgType("emitter", "EventEmitter", emitter);
    }
  }

  async function once(emitter, name, options) {
    const opts = options === undefined || options === null ? {} : options;
    const signal = opts.signal;
    validateAbortSignal(signal, "options.signal");
    if (signal !== undefined && signal.aborted) {
      throw new AbortError(undefined, { cause: signal.reason });
    }
    return new Promise(function (resolve, reject) {
      const errorListener = function (err) {
        emitter.removeListener(name, resolver);
        if (signal !== undefined) {
          eventTargetAgnosticRemoveListener(signal, "abort", abortListener);
        }
        reject(err);
      };
      const resolver = function (...args) {
        if (typeof emitter.removeListener === "function") {
          emitter.removeListener("error", errorListener);
        }
        if (signal !== undefined) {
          eventTargetAgnosticRemoveListener(signal, "abort", abortListener);
        }
        resolve(args);
      };
      const abortListener = function () {
        eventTargetAgnosticRemoveListener(emitter, name, resolver);
        if (name !== "error" && typeof emitter.once === "function") {
          eventTargetAgnosticRemoveListener(emitter, "error", errorListener);
        }
        reject(new AbortError(undefined, { cause: signal.reason }));
      };

      eventTargetAgnosticAddListener(emitter, name, resolver, { once: true });
      if (name !== "error" && typeof emitter.once === "function") {
        emitter.once("error", errorListener);
      }
      if (signal !== undefined) {
        eventTargetAgnosticAddListener(signal, "abort", abortListener, { once: true });
      }
    });
  }

  function on(emitter, event, options) {
    const opts = options === undefined || options === null ? {} : options;
    const signal = opts.signal;
    validateAbortSignal(signal, "options.signal");
    if (signal !== undefined && signal.aborted) {
      throw new AbortError(undefined, { cause: signal.reason });
    }
    const closeEvents = opts.close;
    if (closeEvents !== undefined && closeEvents !== null && !Array.isArray(closeEvents)) {
      throw invalidArgType("options.close", "Array", closeEvents);
    }

    const unconsumedEvents = [];
    const unconsumedPromises = [];
    let error = null;
    let finished = false;

    const subscriptions = [];
    function subscribe(target, name, handler, flags) {
      eventTargetAgnosticAddListener(target, name, handler, flags);
      subscriptions.push([target, name, handler, flags]);
    }
    function unsubscribeAll() {
      while (subscriptions.length > 0) {
        const sub = subscriptions.pop();
        eventTargetAgnosticRemoveListener(sub[0], sub[1], sub[2], sub[3]);
      }
    }

    function eventHandler(...eventArgs) {
      const promise = unconsumedPromises.shift();
      if (promise !== undefined) {
        promise.resolve({ value: eventArgs, done: false });
      } else {
        unconsumedEvents.push(eventArgs);
      }
    }

    function errorHandler(err) {
      finished = true;
      const pending = unconsumedPromises.shift();
      if (pending !== undefined) pending.reject(err);
      else error = err;
      closeHandler();
    }

    function closeHandler() {
      unsubscribeAll();
      finished = true;
      const doneResult = { value: undefined, done: true };
      while (unconsumedPromises.length > 0) {
        unconsumedPromises.shift().resolve(doneResult);
      }
      return Promise.resolve(doneResult);
    }

    const iterator = {
      next() {
        // 1. consume buffered events (even after finish/return)
        if (unconsumedEvents.length > 0) {
          return Promise.resolve({ value: unconsumedEvents.shift(), done: false });
        }
        // 2. deliver a stored error exactly once
        if (error !== null) {
          const p = Promise.reject(error);
          error = null;
          return p;
        }
        // 3. done
        if (finished) return closeHandler();
        // 4. wait for the next event
        return new Promise(function (resolve, reject) {
          unconsumedPromises.push({ resolve, reject });
        });
      },
      return() {
        return closeHandler();
      },
      throw(err) {
        if (!err || !(err instanceof Error)) {
          throw invalidArgType("EventEmitter.AsyncIterator", "Error", err);
        }
        errorHandler(err);
        return undefined;
      },
    };
    Object.setPrototypeOf(iterator, AsyncIteratorPrototype);
    Object.defineProperty(iterator, Symbol.asyncIterator, {
      value: function () { return this; },
      writable: true,
      enumerable: false,
      configurable: true,
    });

    subscribe(emitter, event, eventHandler);
    if (event !== "error" && typeof emitter.on === "function") {
      subscribe(emitter, "error", errorHandler);
    }
    if (closeEvents !== undefined && closeEvents !== null) {
      for (let i = 0; i < closeEvents.length; i++) {
        subscribe(emitter, closeEvents[i], closeHandler);
      }
    }
    if (signal !== undefined) {
      const abortHandler = function () {
        errorHandler(new AbortError(undefined, { cause: signal.reason }));
      };
      signal.addEventListener("abort", abortHandler, { once: true });
      subscriptions.push([signal, "abort", abortHandler, undefined]);
    }

    return iterator;
  }

  function addAbortListener(signal, listener) {
    if (signal === undefined) {
      throw invalidArgType("signal", "AbortSignal", signal);
    }
    validateAbortSignal(signal, "signal");
    if (typeof listener !== "function") {
      throw invalidArgType("listener", "function", listener);
    }
    let removeEventListener;
    if (signal.aborted) {
      scheduleMicrotask(function () { listener(); });
    } else {
      signal.addEventListener("abort", listener, { once: true });
      removeEventListener = function () {
        signal.removeEventListener("abort", listener);
      };
    }
    const disposable = { __proto__: null };
    disposable[kDispose] = function () {
      if (removeEventListener !== undefined) removeEventListener();
    };
    return disposable;
  }

  // ---------------------------------------------------------------------
  // EventEmitterAsyncResource (minimal in-isolate AsyncResource stand-in)
  // ---------------------------------------------------------------------
  let asyncIdCounter = 0;

  class MinimalAsyncResource {
    constructor(type) {
      if (typeof type !== "string") {
        throw invalidArgType("type", "string", type);
      }
      this._type = type;
      this._asyncId = ++asyncIdCounter;
      this._triggerAsyncId = 0;
      this._destroyed = false;
    }
    asyncId() {
      return this._asyncId;
    }
    triggerAsyncId() {
      return this._triggerAsyncId;
    }
    runInAsyncScope(fn, thisArg, ...args) {
      return Reflect.apply(fn, thisArg, args);
    }
    emitDestroy() {
      this._destroyed = true;
      return this;
    }
  }

  class EventEmitterReferencingAsyncResource extends MinimalAsyncResource {
    constructor(ee, type) {
      super(type);
      this[kEventEmitter] = ee;
    }
    get eventEmitter() {
      if (this[kEventEmitter] === undefined) {
        throw new ERR_INVALID_THIS(
          'Value of "this" must be of type EventEmitterReferencingAsyncResource'
        );
      }
      return this[kEventEmitter];
    }
  }

  class EventEmitterAsyncResource extends EventEmitter {
    constructor(options) {
      let name;
      let opts = options;
      if (typeof opts === "string") {
        name = opts;
        opts = undefined;
      } else {
        if (new.target === EventEmitterAsyncResource) {
          const givenName = opts === null || opts === undefined ? undefined : opts.name;
          if (typeof givenName !== "string") {
            throw invalidArgType("options.name", "string", givenName);
          }
        }
        name = (opts !== null && opts !== undefined && opts.name) || new.target.name;
      }
      super(opts);
      this[kAsyncResource] = new EventEmitterReferencingAsyncResource(this, name);
    }
    emit(event, ...args) {
      const resource = this.asyncResource;
      return resource.runInAsyncScope(
        EventEmitter.prototype.emit, this, event, ...args
      );
    }
    emitDestroy() {
      this.asyncResource.emitDestroy();
    }
    get asyncId() {
      return this.asyncResource.asyncId();
    }
    get triggerAsyncId() {
      return this.asyncResource.triggerAsyncId();
    }
    get asyncResource() {
      const resource = this[kAsyncResource];
      if (resource === undefined) {
        throw new ERR_INVALID_THIS(
          'Value of "this" must be of type EventEmitterAsyncResource'
        );
      }
      return resource;
    }
  }

  // ---------------------------------------------------------------------
  // Export wiring (Node classic shape: module.exports === EventEmitter)
  // ---------------------------------------------------------------------
  EventEmitter.EventEmitter = EventEmitter;
  EventEmitter.usingDomains = false;
  EventEmitter.captureRejectionSymbol = kRejection;
  EventEmitter.errorMonitor = kErrorMonitor;
  EventEmitter.kMaxEventTargetListeners = kMaxEventTargetListeners;
  EventEmitter.kMaxEventTargetListenersWarned = kMaxEventTargetListenersWarned;
  EventEmitter.EventEmitterAsyncResource = EventEmitterAsyncResource;
  EventEmitter.once = once;
  EventEmitter.on = on;
  EventEmitter.getEventListeners = getEventListeners;
  EventEmitter.getMaxListeners = getMaxListenersStatic;
  EventEmitter.setMaxListeners = setMaxListenersStatic;
  EventEmitter.listenerCount = staticListenerCount;
  EventEmitter.addAbortListener = addAbortListener;

  Object.defineProperty(EventEmitter, "captureRejections", {
    get() {
      return EventEmitter.prototype[kCapture];
    },
    set(value) {
      validateBoolean(value, "EventEmitter.captureRejections");
      EventEmitter.prototype[kCapture] = value;
    },
    enumerable: true,
    configurable: true,
  });

  Object.defineProperty(EventEmitter, "defaultMaxListeners", {
    get() {
      return defaultMaxListeners;
    },
    set(arg) {
      validateNonNegativeNumber(arg, "defaultMaxListeners");
      defaultMaxListeners = arg;
    },
    enumerable: true,
    configurable: true,
  });

  module.exports = EventEmitter;
});
