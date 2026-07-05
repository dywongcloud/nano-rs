"use strict";
// node:perf_hooks, node:async_hooks, node:diagnostics_channel, node:v8,
// node:module, node:constants, node:test.
__nanoNodeRegister("perf_hooks", function (module, exports, require) {
  const EventEmitter = require("events");
  const host = globalThis.__nano_node_host;

  class PerformanceEntry {
    constructor(name, entryType, startTime, duration) {
      this.name = name;
      this.entryType = entryType;
      this.startTime = startTime;
      this.duration = duration;
      this.detail = null;
    }
    toJSON() {
      return { name: this.name, entryType: this.entryType, startTime: this.startTime, duration: this.duration, detail: this.detail };
    }
  }
  class PerformanceMark extends PerformanceEntry {
    constructor(name, options) {
      super(name, "mark", performance.now(), 0);
      this.detail = (options && options.detail) || null;
    }
  }
  class PerformanceMeasure extends PerformanceEntry {
    constructor(name, startTime, duration, detail) {
      super(name, "measure", startTime, duration);
      this.detail = detail || null;
    }
  }

  const marks = new Map(); // name -> PerformanceMark
  const entries = []; // ordered list of all entries
  const observers = new Set();

  function notifyObservers(list) {
    for (const obs of observers) {
      if (!obs._types || list.some((e) => obs._types.includes(e.entryType))) {
        queueMicrotask(() => obs._callback({
          getEntries: () => list.slice(),
          getEntriesByName: (name, type) => list.filter((e) => e.name === name && (!type || e.entryType === type)),
          getEntriesByType: (type) => list.filter((e) => e.entryType === type),
        }, obs));
      }
    }
  }

  const perf = {
    now: () => performance.now(),
    timeOrigin: typeof performance.timeOrigin === "number" ? performance.timeOrigin : Date.now(),
    nodeTiming: Object.freeze({
      name: "node", entryType: "node", startTime: 0, duration: 0,
      nodeStart: 0, v8Start: 0, bootstrapComplete: 0, environment: 0,
      loopStart: -1, loopExit: -1, idleTime: 0,
    }),
    eventLoopUtilization(prev, util) {
      return { idle: 0, active: 0, utilization: 0 };
    },
    mark(name, options) {
      const m = new PerformanceMark(name, options);
      marks.set(name, m);
      entries.push(m);
      notifyObservers([m]);
      return m;
    },
    measure(name, startOrOptions, end) {
      let startTime = 0;
      let duration;
      let detail;
      if (typeof startOrOptions === "object" && startOrOptions !== null) {
        const opts = startOrOptions;
        startTime = opts.start !== undefined
          ? (typeof opts.start === "string" ? marks.get(opts.start).startTime : opts.start)
          : 0;
        const endTime = opts.end !== undefined
          ? (typeof opts.end === "string" ? marks.get(opts.end).startTime : opts.end)
          : performance.now();
        duration = opts.duration !== undefined ? opts.duration : endTime - startTime;
        detail = opts.detail;
      } else {
        startTime = startOrOptions !== undefined
          ? (marks.has(startOrOptions) ? marks.get(startOrOptions).startTime : 0)
          : 0;
        const endTime = end !== undefined ? (marks.has(end) ? marks.get(end).startTime : end) : performance.now();
        duration = endTime - startTime;
      }
      const meas = new PerformanceMeasure(name, startTime, duration, detail);
      entries.push(meas);
      notifyObservers([meas]);
      return meas;
    },
    clearMarks(name) {
      if (name === undefined) {
        marks.clear();
      } else {
        marks.delete(name);
      }
      for (let i = entries.length - 1; i >= 0; i -= 1) {
        if (entries[i].entryType === "mark" && (name === undefined || entries[i].name === name)) {
          entries.splice(i, 1);
        }
      }
    },
    clearMeasures(name) {
      for (let i = entries.length - 1; i >= 0; i -= 1) {
        if (entries[i].entryType === "measure" && (name === undefined || entries[i].name === name)) {
          entries.splice(i, 1);
        }
      }
    },
    getEntries: () => entries.slice(),
    getEntriesByName: (name, type) => entries.filter((e) => e.name === name && (!type || e.entryType === type)),
    getEntriesByType: (type) => entries.filter((e) => e.entryType === type),
    toJSON() {
      return { timeOrigin: this.timeOrigin, entries: entries.map((e) => e.toJSON()) };
    },
  };

  class PerformanceObserver {
    constructor(callback) {
      this._callback = callback;
      this._types = null;
    }
    observe(options) {
      this._types = options && options.entryTypes ? options.entryTypes : (options && options.type ? [options.type] : null);
      observers.add(this);
    }
    disconnect() {
      observers.delete(this);
    }
    takeRecords() {
      return [];
    }
    static get supportedEntryTypes() {
      return Object.freeze(["mark", "measure"]);
    }
  }

  class Histogram {
    constructor() {
      this._samples = [];
      this.min = 0;
      this.max = 0;
      this.mean = 0;
      this.stddev = 0;
      this.exceeds = 0;
      this.count = 0;
    }
    _record(value) {
      this._samples.push(value);
      this.count = this._samples.length;
      this.min = Math.min(...this._samples);
      this.max = Math.max(...this._samples);
      this.mean = this._samples.reduce((a, b) => a + b, 0) / this._samples.length;
      const variance = this._samples.reduce((acc, v) => acc + (v - this.mean) ** 2, 0) / this._samples.length;
      this.stddev = Math.sqrt(variance);
    }
    percentile(p) {
      if (this._samples.length === 0) return 0;
      const sorted = [...this._samples].sort((a, b) => a - b);
      const idx = Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1);
      return sorted[Math.max(0, idx)];
    }
    percentiles() {
      return new Map([[50, this.percentile(50)], [90, this.percentile(90)], [99, this.percentile(99)]]);
    }
    reset() {
      this._samples = [];
      this.min = this.max = this.mean = this.stddev = this.count = 0;
    }
  }

  function monitorEventLoopDelay(options) {
    const h = new Histogram();
    let timer = null;
    let last = null;
    const resolutionMs = (options && options.resolution) || 10;
    return {
      enable() {
        last = performance.now();
        timer = setInterval(() => {
          const now = performance.now();
          h._record(Math.max(0, now - last - resolutionMs));
          last = now;
        }, resolutionMs);
        timer.unref?.();
        return true;
      },
      disable() {
        clearInterval(timer);
        return true;
      },
      reset: () => h.reset(),
      get min() { return h.min; },
      get max() { return h.max; },
      get mean() { return h.mean; },
      get stddev() { return h.stddev; },
      percentile: (p) => h.percentile(p),
      percentiles: () => h.percentiles(),
    };
  }

  function createHistogram() {
    return new Histogram();
  }

  module.exports = {
    performance: perf,
    PerformanceObserver,
    PerformanceEntry,
    PerformanceMark,
    PerformanceMeasure,
    monitorEventLoopDelay,
    createHistogram,
    constants: Object.freeze({
      NODE_PERFORMANCE_GC_MAJOR: 4,
      NODE_PERFORMANCE_GC_MINOR: 1,
      NODE_PERFORMANCE_GC_INCREMENTAL: 8,
      NODE_PERFORMANCE_GC_WEAKCB: 16,
    }),
  };
});

__nanoNodeRegister("async_hooks", function (module, exports, require) {
  let nextAsyncId = 1;
  let executionAsyncId = 0;
  let triggerAsyncId = 0;

  // Functional for synchronous continuations within run()/runInAsyncScope();
  // does NOT propagate across native macrotask boundaries (setTimeout,
  // Promise continuations scheduled outside the synchronous run body)
  // since that requires V8 async-context-propagation hooks this sandbox
  // does not have access to (documented divergence).
  class AsyncLocalStorage {
    constructor() {
      this._stack = [];
    }
    getStore() {
      return this._stack.length > 0 ? this._stack[this._stack.length - 1] : undefined;
    }
    run(store, callback, ...args) {
      this._stack.push(store);
      try {
        return callback(...args);
      } finally {
        this._stack.pop();
      }
    }
    exit(callback, ...args) {
      const saved = this._stack;
      this._stack = [];
      try {
        return callback(...args);
      } finally {
        this._stack = saved;
      }
    }
    enterWith(store) {
      this._stack.push(store);
    }
    disable() {
      this._stack = [];
    }
    static bind(fn, thisArg) {
      return fn.bind(thisArg);
    }
    static snapshot() {
      return (fn, ...args) => fn(...args);
    }
  }

  class AsyncResource {
    constructor(type, options) {
      this.type = type;
      this._asyncId = nextAsyncId++;
      this._triggerAsyncId = (options && options.triggerAsyncId) || triggerAsyncId;
    }
    runInAsyncScope(fn, thisArg, ...args) {
      const prevExec = executionAsyncId;
      executionAsyncId = this._asyncId;
      try {
        return fn.apply(thisArg, args);
      } finally {
        executionAsyncId = prevExec;
      }
    }
    asyncId() {
      return this._asyncId;
    }
    triggerAsyncId() {
      return this._triggerAsyncId;
    }
    bind(fn, thisArg) {
      const self = this;
      const bound = function (...args) {
        return self.runInAsyncScope(fn, thisArg || this, ...args);
      };
      return bound;
    }
    static bind(fn, type, thisArg) {
      const resource = new AsyncResource(type || fn.name || "bound-anonymous-fn");
      return resource.bind(fn, thisArg);
    }
    emitDestroy() {
      return this;
    }
  }

  function createHook(callbacks) {
    return {
      enable() { return this; },
      disable() { return this; },
    };
  }

  module.exports = {
    AsyncLocalStorage,
    AsyncResource,
    createHook,
    executionAsyncId: () => executionAsyncId,
    triggerAsyncId: () => triggerAsyncId,
    executionAsyncResource: () => ({}),
  };
});

__nanoNodeRegister("diagnostics_channel", function (module, exports, require) {
  const channels = new Map();

  class Channel {
    constructor(name) {
      this.name = name;
      this._subscribers = [];
    }
    get hasSubscribers() {
      return this._subscribers.length > 0;
    }
    publish(message) {
      for (const sub of this._subscribers.slice()) {
        sub(message, this.name);
      }
    }
    subscribe(onMessage) {
      this._subscribers.push(onMessage);
    }
    unsubscribe(onMessage) {
      const idx = this._subscribers.indexOf(onMessage);
      if (idx !== -1) {
        this._subscribers.splice(idx, 1);
        return true;
      }
      return false;
    }
    bindStore(store, transform) {
      this._bindStore = store;
      this._bindTransform = transform || ((m) => m);
    }
    runStores(message, fn, thisArg, ...args) {
      if (!this._bindStore) return fn.apply(thisArg, args);
      return this._bindStore.run(this._bindTransform(message), () => fn.apply(thisArg, args));
    }
  }

  function channel(name) {
    let ch = channels.get(name);
    if (!ch) {
      ch = new Channel(name);
      channels.set(name, ch);
    }
    return ch;
  }
  function hasSubscribers(name) {
    return channels.has(name) && channels.get(name).hasSubscribers;
  }
  function subscribe(name, onMessage) {
    channel(name).subscribe(onMessage);
  }
  function unsubscribe(name, onMessage) {
    return channels.has(name) && channels.get(name).unsubscribe(onMessage);
  }

  function tracingChannel(nameOrChannels) {
    const base = typeof nameOrChannels === "string" ? nameOrChannels : null;
    const chans = base
      ? {
          start: channel(base + ":start"), end: channel(base + ":end"),
          asyncStart: channel(base + ":asyncStart"), asyncEnd: channel(base + ":asyncEnd"),
          error: channel(base + ":error"),
        }
      : nameOrChannels;

    return {
      start: chans.start, end: chans.end, asyncStart: chans.asyncStart,
      asyncEnd: chans.asyncEnd, error: chans.error,
      subscribe(handlers) {
        for (const key of Object.keys(handlers)) {
          if (chans[key]) chans[key].subscribe(handlers[key]);
        }
      },
      unsubscribe(handlers) {
        for (const key of Object.keys(handlers)) {
          if (chans[key]) chans[key].unsubscribe(handlers[key]);
        }
      },
      traceSync(fn, context = {}, thisArg, ...args) {
        chans.start.publish(context);
        try {
          const result = fn.apply(thisArg, args);
          context.result = result;
          chans.end.publish(context);
          return result;
        } catch (err) {
          context.error = err;
          chans.error.publish(context);
          chans.end.publish(context);
          throw err;
        }
      },
      tracePromise(fn, context = {}, thisArg, ...args) {
        chans.start.publish(context);
        chans.asyncStart.publish(context);
        return Promise.resolve(fn.apply(thisArg, args)).then(
          (result) => {
            context.result = result;
            chans.asyncEnd.publish(context);
            return result;
          },
          (err) => {
            context.error = err;
            chans.error.publish(context);
            chans.asyncEnd.publish(context);
            throw err;
          }
        ).finally(() => chans.end.publish(context));
      },
      traceCallback(fn, position = -1, context = {}, thisArg, ...args) {
        chans.start.publish(context);
        chans.asyncStart.publish(context);
        const idx = position < 0 ? args.length + position : position;
        const userCb = args[idx];
        args[idx] = (err, ...cbArgs) => {
          if (err) context.error = err;
          else context.result = cbArgs[0];
          if (err) chans.error.publish(context);
          chans.asyncEnd.publish(context);
          chans.end.publish(context);
          if (typeof userCb === "function") userCb(err, ...cbArgs);
        };
        return fn.apply(thisArg, args);
      },
    };
  }

  module.exports = { channel, hasSubscribers, subscribe, unsubscribe, tracingChannel, Channel };
});

__nanoNodeRegister("v8", function (module, exports, require) {
  const { unsupported, notPermitted } = require("internal/errors");
  const host = globalThis.__nano_node_host;

  module.exports = {
    getHeapStatistics() {
      const m = host.memoryUsage();
      return {
        total_heap_size: m.heapTotal, total_heap_size_executable: 0,
        total_physical_size: m.heapTotal, total_available_size: Math.max(0, 512 * 1024 * 1024 - m.heapUsed),
        used_heap_size: m.heapUsed, heap_size_limit: 512 * 1024 * 1024,
        malloced_memory: 0, peak_malloced_memory: 0, does_zap_garbage: 0,
        number_of_native_contexts: 1, number_of_detached_contexts: 0, total_global_handles_size: 0,
        used_global_handles_size: 0, external_memory: m.external,
      };
    },
    getHeapSpaceStatistics() {
      return [];
    },
    getHeapCodeStatistics() {
      return { code_and_metadata_size: 0, bytecode_and_metadata_size: 0, external_script_source_size: 0, cpu_profiler_metadata_size: 0 };
    },
    cachedDataVersionTag() {
      return 0;
    },
    setFlagsFromString() {},
    serialize() {
      throw unsupported("v8.serialize");
    },
    deserialize() {
      throw unsupported("v8.deserialize");
    },
    Serializer: class Serializer {
      constructor() { throw unsupported("v8.Serializer"); }
    },
    Deserializer: class Deserializer {
      constructor() { throw unsupported("v8.Deserializer"); }
    },
    writeHeapSnapshot() {
      throw notPermitted("writeHeapSnapshot", "heap snapshots are disabled in the NANO runtime");
    },
    GCProfiler: class GCProfiler {
      constructor() { throw unsupported("v8.GCProfiler"); }
    },
    promiseHooks: {
      onInit: () => () => {},
      onSettled: () => () => {},
      onBefore: () => () => {},
      onAfter: () => () => {},
      createHook: () => ({ enable() {}, disable() {} }),
    },
    startupSnapshot: {
      isBuildingSnapshot: () => false,
      addSerializeCallback() { throw unsupported("v8.startupSnapshot.addSerializeCallback"); },
      addDeserializeCallback() { throw unsupported("v8.startupSnapshot.addDeserializeCallback"); },
      setDeserializeMainFunction() { throw unsupported("v8.startupSnapshot.setDeserializeMainFunction"); },
    },
  };
});

__nanoNodeRegister("module", function (module, exports, require) {
  class SourceMap {
    constructor(payload) {
      this.payload = payload;
      this._decoded = payload && payload.mappings ? payload.mappings : "";
    }
    findEntry() {
      return {};
    }
  }

  class Module {
    constructor(id, parent) {
      this.id = id || ".";
      this.exports = {};
      this.parent = parent || null;
      this.filename = null;
      this.loaded = false;
      this.children = [];
      this.paths = [];
    }
    static _cache = {};
    static builtinModules = globalThis.__nanoNodeBuiltinIds ? globalThis.__nanoNodeBuiltinIds() : [];
    static isBuiltin(id) {
      const name = id.startsWith("node:") ? id.slice(5) : id;
      return globalThis.__nanoNodeIsRegistered ? globalThis.__nanoNodeIsRegistered(name) : false;
    }
    static createRequire(filename) {
      return globalThis.__nanoNodeRequire;
    }
    static syncBuiltinESMExports() {}
    static register() {
      const { unsupported } = require("internal/errors");
      throw unsupported("module.register (loader customization)");
    }
    static findSourceMap() {
      return undefined;
    }
    static enableCompileCache() {
      return { status: 2, message: "Compile cache is not applicable in the NANO runtime" };
    }
    static getCompileCacheDir() {
      return undefined;
    }
  }

  module.exports = Module;
  Module.Module = Module;
  Module.SourceMap = SourceMap;
  Module.constants = Object.freeze({
    compileCacheStatus: Object.freeze({ ENABLED: 0, ALREADY_ENABLED: 1, DISABLED: 2, FAILED: 3 }),
  });
});

__nanoNodeRegister("constants", function (module, exports, require) {
  const os = require("os");
  const fs = require("fs");
  const crypto = require("crypto");
  module.exports = {
    ...os.constants.errno,
    ...os.constants.signals,
    ...fs.constants,
    ...crypto.constants,
  };
});

__nanoNodeRegister("test", function (module, exports, require) {
  const { makeError } = require("internal/errors");

  const rootSuites = [];
  let currentSuite = { name: null, tests: [], hooks: { before: [], after: [], beforeEach: [], afterEach: [] }, parent: null };
  const suiteStack = [currentSuite];
  const failures = [];
  let totalTests = 0;
  let passedTests = 0;

  function currentContext() {
    return suiteStack[suiteStack.length - 1];
  }

  function makeTestContext(name) {
    return {
      name,
      diagnostic(msg) {
        console.log("# " + msg);
      },
      skip(msg) {
        throw Object.assign(new Error("__nano_test_skip__"), { __skip: true, reason: msg });
      },
      todo(msg) {
        throw Object.assign(new Error("__nano_test_todo__"), { __todo: true, reason: msg });
      },
      test(subName, subOptions, subFn) {
        return test(subName, subOptions, subFn);
      },
    };
  }

  async function runOne(entry) {
    totalTests += 1;
    const ctx = makeTestContext(entry.name);
    const suite = entry.suite;
    try {
      for (const hook of suite.hooks.beforeEach) await hook(ctx);
      if (entry.options.skip) {
        console.log("ok " + totalTests + " - " + entry.name + " # SKIP");
        passedTests += 1;
        return;
      }
      await entry.fn(ctx);
      for (const hook of suite.hooks.afterEach) await hook(ctx);
      console.log("ok " + totalTests + " - " + entry.name);
      passedTests += 1;
    } catch (err) {
      if (err && err.__skip) {
        console.log("ok " + totalTests + " - " + entry.name + " # SKIP " + (err.reason || ""));
        passedTests += 1;
        return;
      }
      if (err && err.__todo) {
        console.log("ok " + totalTests + " - " + entry.name + " # TODO " + (err.reason || ""));
        passedTests += 1;
        return;
      }
      console.log("not ok " + totalTests + " - " + entry.name);
      console.log("  ---");
      console.log("  message: " + err.message);
      console.log("  ...");
      failures.push({ name: entry.name, error: err });
    }
  }

  function test(name, options, fn) {
    if (typeof name === "function") { fn = name; name = name.name || "<anonymous>"; options = {}; }
    if (typeof options === "function") { fn = options; options = {}; }
    options = options || {};
    const suite = currentContext();
    const entry = { name, options, fn: fn || (() => {}), suite };
    suite.tests.push(entry);
    const promise = Promise.resolve().then(() => runOne(entry));
    rootSuites.push(promise);
    return promise;
  }
  test.skip = (name, options, fn) => test(name, { ...(typeof options === "object" ? options : {}), skip: true }, fn);
  test.todo = (name, options, fn) => test(name, { ...(typeof options === "object" ? options : {}), todo: true }, fn);
  test.only = test;

  function describe(name, fn) {
    const suite = { name, tests: [], hooks: { before: [], after: [], beforeEach: [], afterEach: [] }, parent: currentContext() };
    suiteStack.push(suite);
    try {
      if (fn) fn();
    } finally {
      suiteStack.pop();
    }
  }
  describe.skip = () => {};
  const suite = describe;
  const it = test;

  function before(fn) { currentContext().hooks.before.push(fn); }
  function after(fn) { currentContext().hooks.after.push(fn); }
  function beforeEach(fn) { currentContext().hooks.beforeEach.push(fn); }
  function afterEach(fn) { currentContext().hooks.afterEach.push(fn); }

  // ---------------------------------------------------------------------
  // mock
  // ---------------------------------------------------------------------
  function createMockFn(original) {
    const calls = [];
    let impl = original || (() => undefined);
    const implOnceQueue = [];
    let callCount = 0;

    function mockFn(...args) {
      callCount += 1;
      const record = { arguments: args, result: undefined, error: undefined, target: this };
      calls.push(record);
      try {
        const fn = implOnceQueue.length > 0 ? implOnceQueue.shift() : impl;
        record.result = fn.apply(this, args);
        return record.result;
      } catch (err) {
        record.error = err;
        throw err;
      }
    }
    mockFn.mock = {
      calls,
      callCount: () => callCount,
      mockImplementation(newImpl) {
        impl = newImpl;
      },
      mockImplementationOnce(newImpl) {
        implOnceQueue.push(newImpl);
      },
      resetCalls() {
        calls.length = 0;
        callCount = 0;
      },
      restore() {
        impl = original || (() => undefined);
      },
    };
    return mockFn;
  }

  const mock = {
    fn(original) {
      return createMockFn(original);
    },
    method(obj, methodName, implementation) {
      const original = obj[methodName];
      const mocked = createMockFn(implementation || original);
      obj[methodName] = mocked;
      mocked.mock.restore = () => {
        obj[methodName] = original;
      };
      return mocked;
    },
    getter(obj, propertyName, implementation) {
      const desc = Object.getOwnPropertyDescriptor(obj, propertyName);
      const mocked = createMockFn(implementation || (desc && desc.get));
      Object.defineProperty(obj, propertyName, { ...desc, get: mocked });
      return mocked;
    },
    setter(obj, propertyName, implementation) {
      const desc = Object.getOwnPropertyDescriptor(obj, propertyName);
      const mocked = createMockFn(implementation || (desc && desc.set));
      Object.defineProperty(obj, propertyName, { ...desc, set: mocked });
      return mocked;
    },
    reset() {},
    restoreAll() {},
    timers: {
      enable() {
        const { unsupported } = require("internal/errors");
        throw unsupported("mock.timers (fake timers)");
      },
      reset() {},
    },
  };

  async function run() {
    await Promise.all(rootSuites);
    if (failures.length > 0) {
      const err = makeError(Error, "ERR_TEST_FAILURE", failures.length + " test(s) failed");
      err.failures = failures;
      throw err;
    }
    return { totalTests, passedTests, failed: failures.length };
  }

  module.exports = { test, describe, suite, it, before, after, beforeEach, afterEach, mock, run };
});
