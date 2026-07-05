"use strict";
// internal/http-bridge — connects node-style http.Server listeners to
// NANO's fetch-handler execution model (CONTRACT.md §7).
__nanoNodeRegister("internal/http-bridge", function (module, exports, require) {
  let servers = []; // { server, kind }

  function registerServer(server, kind) {
    servers = servers.filter((e) => e.server !== server);
    servers.push({ server, kind: kind || "http" });
  }
  function unregisterServer(server) {
    servers = servers.filter((e) => e.server !== server);
  }
  function hasServer() {
    return servers.length > 0;
  }
  function getPrimaryServer() {
    return servers.length > 0 ? servers[servers.length - 1] : undefined;
  }

  function decodeBase64(b64) {
    const { Buffer } = require("buffer");
    return new Uint8Array(Buffer.from(b64, "base64"));
  }
  function encodeBase64(bytes) {
    const { Buffer } = require("buffer");
    return Buffer.from(bytes).toString("base64");
  }

  function abortErrorFor() {
    const err = new Error("The operation was aborted");
    err.name = "AbortError";
    err.code = "ABORT_ERR";
    return err;
  }

  /// Converts a WinterTC `Request` (the object NANO's Rust runtime passes to
  /// `fetch(request)` handlers) into the plain shape the internal adapter
  /// dispatches on. `request.body` is already base64 text (or null) on the
  /// Request objects the Rust runtime constructs — see request.rs.
  function requestToShape(request) {
    const headers = {};
    if (request && request.headers && typeof request.headers.forEach === "function") {
      request.headers.forEach((value, key) => { headers[key] = value; });
    }
    return {
      method: (request && request.method) || "GET",
      url: (request && request.url) || "/",
      headers,
      body: (request && typeof request.body === "string" && request.body.length > 0)
        ? request.body
        : null,
    };
  }

  /// Adapts NANO's `fetch(request)` calling convention to a registered
  /// `http.createServer(handler)` listener: builds an `IncomingMessage` from
  /// the WinterTC `request`, collects the `ServerResponse` output, and
  /// resolves the same `{ status, headers, body }` Response-shape a
  /// `fetch(request)` handler would return (CONTRACT.md §7).
  function buildHandlerAdapter() {
    const primary = getPrimaryServer();
    if (!primary) return undefined;

    return async function adapter(request) {
      const reqShape = requestToShape(request);
      const httpMod = require("http");
      const body = reqShape.body ? decodeBase64(reqShape.body) : null;
      const req = httpMod._internal.createIncomingMessage({
        method: reqShape.method,
        url: reqShape.url,
        headers: reqShape.headers || {},
        body,
      });

      const timeoutMs = 30000;
      let settled = false;
      const result = await new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          if (!settled) {
            settled = true;
            reject(makeStreamPrematureClose());
          }
        }, timeoutMs);
        timer.unref?.();

        const res = httpMod._internal.createServerResponse(req, (respShape) => {
          if (settled) return;
          settled = true;
          clearTimeout(timer);
          resolve(respShape);
        });

        try {
          primary.server.emit("request", req, res);
        } catch (err) {
          if (!settled) {
            settled = true;
            clearTimeout(timer);
            resolve({
              status: 500,
              headers: [["content-type", "text/plain"]],
              body: new TextEncoder().encode("Internal Server Error: " + err.message),
            });
          }
        }
      });

      return adaptToResponseObject(result);
    };
  }

  function makeStreamPrematureClose() {
    const { codes } = require("internal/errors");
    return new codes.ERR_STREAM_PREMATURE_CLOSE();
  }

  /// respShape: { status, headers: plain object, body: string }
  function adaptToResponseObject(internalResp) {
    const decoder = new TextDecoder("utf-8", { fatal: true });
    const headers = {};
    for (const [k, v] of internalResp.headers) {
      if (headers[k] !== undefined) {
        headers[k] = headers[k] + ", " + v;
      } else {
        headers[k] = v;
      }
    }
    let body;
    try {
      body = decoder.decode(internalResp.body || new Uint8Array(0));
    } catch (_e) {
      headers["x-nano-body-encoding"] = "base64";
      body = encodeBase64(internalResp.body || new Uint8Array(0));
    }
    return { status: internalResp.status, headers, body };
  }

  function __resetForTest() {
    servers = [];
  }

  module.exports = {
    registerServer,
    unregisterServer,
    hasServer,
    getPrimaryServer,
    buildHandlerAdapter,
    adaptToResponseObject,
    __resetForTest,
  };
});
