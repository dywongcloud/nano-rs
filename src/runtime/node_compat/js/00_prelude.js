"use strict";
// NANO Node.js compatibility layer — module registry and loader.
//
// Registration protocol (CONTRACT.md §2): builtin module files call
// __nanoNodeRegister(id, factory) at top level. Factories are instantiated
// lazily on first require() with CommonJS circular-import semantics.
(function (globalThis) {
  if (globalThis.__nanoNodeRegister) {
    return; // idempotent across repeated binds on a reused context
  }

  const factories = Object.create(null); // id -> factory(module, exports, require)
  const cache = Object.create(null);     // id -> module record { exports, loaded }

  function normalizeId(id) {
    if (typeof id !== "string" || id.length === 0) {
      const err = new TypeError(
        "The \"id\" argument must be a non-empty string. Received " + String(id)
      );
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    return id.startsWith("node:") ? id.slice(5) : id;
  }

  function register(id, factory) {
    if (typeof factory !== "function") {
      const err = new TypeError(
        "__nanoNodeRegister: factory for \"" + id + "\" must be a function"
      );
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    factories[normalizeId(id)] = factory;
  }

  function nodeRequire(rawId) {
    const id = normalizeId(rawId);
    const cached = cache[id];
    if (cached !== undefined) {
      return cached.exports; // may be partial during circular loads — by design
    }
    const factory = factories[id];
    if (factory === undefined) {
      const err = new Error("Cannot find module '" + rawId + "'");
      err.code = "MODULE_NOT_FOUND";
      throw err;
    }
    const moduleRecord = { id, exports: {}, loaded: false };
    cache[id] = moduleRecord;
    try {
      factory(moduleRecord, moduleRecord.exports, nodeRequire);
    } catch (e) {
      delete cache[id]; // failed load must be retryable, not poisoned
      throw e;
    }
    moduleRecord.loaded = true;
    return moduleRecord.exports;
  }

  nodeRequire.resolve = function resolve(rawId) {
    const id = normalizeId(rawId);
    if (factories[id] === undefined) {
      const err = new Error("Cannot find module '" + rawId + "'");
      err.code = "MODULE_NOT_FOUND";
      throw err;
    }
    return "node:" + id;
  };

  function isRegistered(rawId) {
    let id;
    try {
      id = normalizeId(rawId);
    } catch (_e) {
      return false;
    }
    return factories[id] !== undefined;
  }

  function registeredIds() {
    return Object.keys(factories)
      .filter((id) => !id.startsWith("internal/"))
      .sort();
  }

  Object.defineProperty(globalThis, "__nanoNodeRegister", {
    value: register, writable: false, enumerable: false, configurable: false,
  });
  Object.defineProperty(globalThis, "__nanoNodeRequire", {
    value: nodeRequire, writable: false, enumerable: false, configurable: false,
  });
  Object.defineProperty(globalThis, "__nanoNodeIsRegistered", {
    value: isRegistered, writable: false, enumerable: false, configurable: false,
  });
  Object.defineProperty(globalThis, "__nanoNodeBuiltinIds", {
    value: registeredIds, writable: false, enumerable: false, configurable: false,
  });
})(globalThis);
