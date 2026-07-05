"use strict";
// Global installation (CONTRACT.md §5). Runs once per context, after every
// builtin module has registered. Order matters: internal/web first (other
// modules assume EventTarget/AbortController exist), then buffer (installs
// the full Buffer, superseding the Rust stub), then timers/process, then
// the console upgrade, then the handler bridge, then the full `require`.
(function (g) {
  const web = g.__nanoNodeRequire("internal/web");
  web.__installGlobals(g);

  const buffer = g.__nanoNodeRequire("buffer");
  buffer.__installGlobals(g);

  const timers = g.__nanoNodeRequire("timers");
  timers.__installGlobals(g);

  const process = g.__nanoNodeRequire("process");
  process.__installGlobals(g);

  const internalConsole = g.__nanoNodeRequire("internal/console");
  internalConsole.upgradeGlobalConsole(g);

  // Handler bridge: resolved by the Rust runtime after script evaluation
  // when neither __nano_user_fetch nor a bare `fetch` global handler is
  // found (CONTRACT.md §7).
  Object.defineProperty(g, "__nano_resolve_handler", {
    value: function __nano_resolve_handler() {
      if (typeof g.__nano_user_fetch === "function") {
        return g.__nano_user_fetch;
      }
      if (typeof g.module === "object" && g.module !== null) {
        const exported = g.module.exports;
        if (typeof exported === "function") {
          return undefined; // not a {fetch} shape; nothing bridgeable here
        }
        if (exported && typeof exported.fetch === "function") {
          return exported.fetch.bind(exported);
        }
        if (exported && exported.default && typeof exported.default.fetch === "function") {
          return exported.default.fetch.bind(exported.default);
        }
      }
      const bridge = g.__nanoNodeRequire("internal/http-bridge");
      return bridge.buildHandlerAdapter();
    },
    writable: false,
    enumerable: false,
    configurable: false,
  });

  // Full require(): supersedes the Rust fs-only require installed by
  // fs_polyfill.rs. Resolves any registered builtin (bare or `node:`
  // prefixed); CommonJS-shaped module/exports/__dirname/__filename are
  // provided for bundled applications that expect them.
  Object.defineProperty(g, "require", {
    value: g.__nanoNodeRequire,
    writable: true,
    enumerable: false,
    configurable: true,
  });
  if (g.module === undefined) {
    Object.defineProperty(g, "module", {
      value: { exports: {} },
      writable: true,
      enumerable: false,
      configurable: true,
    });
  }
  if (g.exports === undefined) {
    Object.defineProperty(g, "exports", {
      get() { return g.module.exports; },
      set(v) { g.module.exports = v; },
      enumerable: false,
      configurable: true,
    });
  }
  if (g.__dirname === undefined) {
    Object.defineProperty(g, "__dirname", { value: "/", writable: false, enumerable: false, configurable: true });
  }
  if (g.__filename === undefined) {
    Object.defineProperty(g, "__filename", { value: "/handler.js", writable: false, enumerable: false, configurable: true });
  }
})(globalThis);
