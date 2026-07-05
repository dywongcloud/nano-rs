"use strict";
// node:http, node:https — client over global fetch, server over
// internal/http-bridge (CONTRACT.md §7). See 35_http_bridge.js for the
// req/response adapter contract this module's _internal exports implement.
__nanoNodeRegister("http", function (module, exports, require) {
  const EventEmitter = require("events");
  const { Readable, Writable } = require("stream");
  const { makeError, codes } = require("internal/errors");

  const METHODS = [
    "ACL", "BIND", "CHECKOUT", "CONNECT", "COPY", "DELETE", "GET", "HEAD",
    "LINK", "LOCK", "M-SEARCH", "MERGE", "MKACTIVITY", "MKCALENDAR", "MKCOL",
    "MOVE", "NOTIFY", "OPTIONS", "PATCH", "POST", "PROPFIND", "PROPPATCH",
    "PURGE", "PUT", "REBIND", "REPORT", "SEARCH", "SOURCE", "SUBSCRIBE",
    "TRACE", "UNBIND", "UNLINK", "UNLOCK", "UNSUBSCRIBE",
  ].sort();

  const STATUS_CODES = {
    100: "Continue", 101: "Switching Protocols", 102: "Processing", 103: "Early Hints",
    200: "OK", 201: "Created", 202: "Accepted", 203: "Non-Authoritative Information",
    204: "No Content", 205: "Reset Content", 206: "Partial Content", 207: "Multi-Status",
    208: "Already Reported", 226: "IM Used",
    300: "Multiple Choices", 301: "Moved Permanently", 302: "Found", 303: "See Other",
    304: "Not Modified", 305: "Use Proxy", 307: "Temporary Redirect", 308: "Permanent Redirect",
    400: "Bad Request", 401: "Unauthorized", 402: "Payment Required", 403: "Forbidden",
    404: "Not Found", 405: "Method Not Allowed", 406: "Not Acceptable",
    407: "Proxy Authentication Required", 408: "Request Timeout", 409: "Conflict",
    410: "Gone", 411: "Length Required", 412: "Precondition Failed",
    413: "Payload Too Large", 414: "URI Too Long", 415: "Unsupported Media Type",
    416: "Range Not Satisfiable", 417: "Expectation Failed", 418: "I'm a Teapot",
    421: "Misdirected Request", 422: "Unprocessable Entity", 423: "Locked",
    424: "Failed Dependency", 425: "Too Early", 426: "Upgrade Required",
    428: "Precondition Required", 429: "Too Many Requests",
    431: "Request Header Fields Too Large", 451: "Unavailable For Legal Reasons",
    500: "Internal Server Error", 501: "Not Implemented", 502: "Bad Gateway",
    503: "Service Unavailable", 504: "Gateway Timeout", 505: "HTTP Version Not Supported",
    506: "Variant Also Negotiates", 507: "Insufficient Storage", 508: "Loop Detected",
    509: "Bandwidth Limit Exceeded", 510: "Not Extended", 511: "Network Authentication Required",
  };

  const TOKEN_RE = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/;
  function validateHeaderName(name) {
    if (typeof name !== "string" || !TOKEN_RE.test(name)) {
      throw makeError(TypeError, "ERR_INVALID_HTTP_TOKEN", 'Header name must be a valid HTTP token ["' + name + '"]');
    }
  }
  function validateHeaderValue(name, value) {
    if (value === undefined) {
      throw makeError(TypeError, "ERR_HTTP_INVALID_HEADER_VALUE", "Invalid value for header " + JSON.stringify(name));
    }
    const s = String(value);
    // eslint-disable-next-line no-control-regex
    if (/[^\t\x20-\x7e\x80-\xff]/.test(s)) {
      throw makeError(TypeError, "ERR_INVALID_CHAR", "Invalid character in header content [" + JSON.stringify(name) + "]");
    }
  }

  // ---------------------------------------------------------------------
  // IncomingMessage
  // ---------------------------------------------------------------------
  class IncomingMessage extends Readable {
    constructor(options) {
      super();
      this.httpVersion = "1.1";
      this.httpVersionMajor = 1;
      this.httpVersionMinor = 1;
      this.complete = false;
      this.headers = {};
      this.headersDistinct = {};
      this.rawHeaders = [];
      this.trailers = {};
      this.rawTrailers = [];
      this.socket = { remoteAddress: "127.0.0.1", remotePort: 0, encrypted: !!(options && options.encrypted) };
      this.connection = this.socket;
      this.aborted = false;
      this.statusCode = undefined;
      this.statusMessage = undefined;
      this.method = undefined;
      this.url = undefined;
      this._bodyPushed = false;
    }
    setTimeout(ms, cb) {
      if (cb) this.once("timeout", cb);
      return this;
    }
    destroy(err) {
      super.destroy(err);
      return this;
    }
    _read() {
      if (this._bodyPushed) return;
      this._bodyPushed = true;
      if (this._pendingBody && this._pendingBody.length > 0) {
        this.push(this._pendingBody);
      }
      this.push(null);
      this.complete = true;
    }
  }

  function setHeadersFromPlainObject(msg, headersObj) {
    for (const [k, v] of Object.entries(headersObj || {})) {
      const lower = k.toLowerCase();
      if (Array.isArray(v)) {
        msg.headers[lower] = v.join(", ");
        msg.headersDistinct[lower] = v;
        for (const vv of v) msg.rawHeaders.push(k, vv);
      } else {
        msg.headers[lower] = String(v);
        msg.headersDistinct[lower] = [String(v)];
        msg.rawHeaders.push(k, String(v));
      }
    }
  }

  // ---------------------------------------------------------------------
  // OutgoingMessage / ServerResponse
  // ---------------------------------------------------------------------
  class OutgoingMessage extends Writable {
    constructor() {
      super();
      this._headers = new Map(); // lowercased name -> {name, value}
      this.headersSent = false;
      this.finished = false;
      this.sendDate = true;
      this.chunkedEncoding = false;
    }
    setHeader(name, value) {
      if (this.headersSent) {
        throw new codes.ERR_HTTP_HEADERS_SENT("set");
      }
      validateHeaderName(name);
      validateHeaderValue(name, value);
      this._headers.set(name.toLowerCase(), { name, value });
      return this;
    }
    getHeader(name) {
      const entry = this._headers.get(String(name).toLowerCase());
      return entry ? entry.value : undefined;
    }
    getHeaders() {
      const out = Object.create(null);
      for (const { name, value } of this._headers.values()) {
        out[name.toLowerCase()] = value;
      }
      return out;
    }
    getHeaderNames() {
      return [...this._headers.values()].map((e) => e.name.toLowerCase());
    }
    hasHeader(name) {
      return this._headers.has(String(name).toLowerCase());
    }
    removeHeader(name) {
      if (this.headersSent) {
        throw new codes.ERR_HTTP_HEADERS_SENT("remove");
      }
      this._headers.delete(String(name).toLowerCase());
    }
    appendHeader(name, value) {
      const existing = this.getHeader(name);
      if (existing === undefined) {
        this.setHeader(name, value);
      } else {
        const merged = Array.isArray(existing) ? existing.concat(value) : [existing].concat(value);
        this.setHeader(name, merged);
      }
      return this;
    }
    flushHeaders() {
      this.headersSent = true;
    }
    addTrailers() {}
    setTimeout(ms, cb) {
      if (cb) this.once("timeout", cb);
      return this;
    }
  }

  class ServerResponse extends OutgoingMessage {
    constructor(req, onComplete) {
      super();
      this.req = req;
      this.statusCode = 200;
      this.statusMessage = undefined;
      this._onComplete = onComplete;
      this._bodyChunks = [];
      this._ended = false;
    }
    writeHead(statusCode, statusMessage, headers) {
      if (typeof statusMessage === "object" && statusMessage !== null) {
        headers = statusMessage;
        statusMessage = undefined;
      }
      this.statusCode = statusCode;
      if (statusMessage !== undefined) this.statusMessage = statusMessage;
      if (headers) {
        if (Array.isArray(headers)) {
          for (let i = 0; i < headers.length; i += 2) {
            this.setHeader(headers[i], headers[i + 1]);
          }
        } else {
          for (const [k, v] of Object.entries(headers)) {
            this.setHeader(k, v);
          }
        }
      }
      this.headersSent = true;
      return this;
    }
    writeContinue() {}
    write(chunk, encoding, callback) {
      if (typeof encoding === "function") {
        callback = encoding;
        encoding = undefined;
      }
      this.headersSent = true;
      const { Buffer } = require("buffer");
      this._bodyChunks.push(typeof chunk === "string" ? Buffer.from(chunk, encoding || "utf8") : Buffer.from(chunk));
      if (callback) queueMicrotask(callback);
      return true;
    }
    end(chunk, encoding, callback) {
      if (typeof chunk === "function") {
        callback = chunk;
        chunk = undefined;
      } else if (typeof encoding === "function") {
        callback = encoding;
        encoding = undefined;
      }
      if (this._ended) {
        if (callback) queueMicrotask(callback);
        return this;
      }
      if (chunk !== undefined) this.write(chunk, encoding);
      this._ended = true;
      this.finished = true;
      this.headersSent = true;
      const { Buffer } = require("buffer");
      const body = Buffer.concat(this._bodyChunks);
      const headerEntries = [...this._headers.values()].map((e) => [e.name.toLowerCase(), String(e.value)]);
      this._onComplete({
        status: this.statusCode,
        statusText: this.statusMessage || STATUS_CODES[this.statusCode] || "",
        headers: headerEntries,
        body: new Uint8Array(body.buffer, body.byteOffset, body.byteLength),
      });
      this.emit("finish");
      queueMicrotask(() => this.emit("close"));
      if (callback) queueMicrotask(callback);
      return this;
    }
  }

  // ---------------------------------------------------------------------
  // ClientRequest
  // ---------------------------------------------------------------------
  class ClientRequest extends Writable {
    constructor(urlOrOptions, optionsOrCb, cb) {
      super();
      let options;
      if (typeof urlOrOptions === "string" || urlOrOptions instanceof URL) {
        const u = typeof urlOrOptions === "string" ? new URL(urlOrOptions) : urlOrOptions;
        options = { protocol: u.protocol, hostname: u.hostname, port: u.port, path: u.pathname + u.search };
        if (typeof optionsOrCb === "object" && optionsOrCb !== null) {
          Object.assign(options, optionsOrCb);
        } else if (typeof optionsOrCb === "function") {
          cb = optionsOrCb;
        }
      } else {
        options = { ...urlOrOptions };
        if (typeof optionsOrCb === "function") {
          cb = optionsOrCb;
        }
      }

      const protocol = options.protocol || (options._defaultHttps ? "https:" : "http:");
      if (protocol !== "http:" && protocol !== "https:") {
        throw new codes.ERR_INVALID_PROTOCOL(protocol, "Protocol '" + protocol + "' not supported. Expected 'http:'");
      }

      this.method = (options.method || "GET").toUpperCase();
      this.path = options.path || "/";
      this._headersMap = new Map();
      this.headersSent = false;
      this.finished = false;
      this.aborted = false;
      this._bodyChunks = [];

      const host = options.hostname || options.host || "localhost";
      const port = options.port ? ":" + options.port : "";
      this._url = protocol + "//" + host + port + this.path;

      if (options.headers) {
        for (const [k, v] of Object.entries(options.headers)) {
          this.setHeader(k, v);
        }
      }
      if (options.auth) {
        this.setHeader("authorization", "Basic " + require("buffer").Buffer.from(options.auth).toString("base64"));
      }

      if (cb) this.once("response", cb);
      if (options.timeout) this.setTimeout(options.timeout);
      if (options.signal) {
        options.signal.addEventListener("abort", () => this.destroy(), { once: true });
      }
    }
    setHeader(name, value) {
      validateHeaderName(name);
      validateHeaderValue(name, value);
      this._headersMap.set(name.toLowerCase(), { name, value });
      return this;
    }
    getHeader(name) {
      const e = this._headersMap.get(String(name).toLowerCase());
      return e ? e.value : undefined;
    }
    removeHeader(name) {
      this._headersMap.delete(String(name).toLowerCase());
    }
    setTimeout(ms, cb) {
      if (cb) this.once("timeout", cb);
      return this;
    }
    flushHeaders() {
      this.headersSent = true;
    }
    _write(chunk, encoding, callback) {
      const { Buffer } = require("buffer");
      this._bodyChunks.push(typeof chunk === "string" ? Buffer.from(chunk, encoding) : Buffer.from(chunk));
      callback();
    }
    abort() {
      this.aborted = true;
      this.destroy();
    }
    end(chunk, encoding, callback) {
      if (typeof chunk === "function") {
        callback = chunk;
        chunk = undefined;
      }
      super.end(chunk, encoding, () => {
        this._dispatch();
        if (callback) callback();
      });
      return this;
    }
    async _dispatch() {
      this.headersSent = true;
      const { Buffer } = require("buffer");
      const headers = {};
      for (const { name, value } of this._headersMap.values()) {
        headers[name] = String(value);
      }
      const body = this._bodyChunks.length > 0 && this.method !== "GET" && this.method !== "HEAD"
        ? Buffer.concat(this._bodyChunks)
        : undefined;
      try {
        const response = await fetch(this._url, { method: this.method, headers, body });
        const im = new IncomingMessage({ encrypted: this._url.startsWith("https:") });
        im.statusCode = response.status;
        im.statusMessage = response.statusText || STATUS_CODES[response.status] || "";
        im.httpVersion = "1.1";
        const headersObj = {};
        response.headers.forEach((v, k) => {
          headersObj[k] = headersObj[k] !== undefined ? headersObj[k] + ", " + v : v;
        });
        setHeadersFromPlainObject(im, headersObj);
        const buf = new Uint8Array(await response.arrayBuffer());
        im._pendingBody = buf;
        this.emit("response", im);
      } catch (err) {
        this.emit("error", err);
      }
    }
  }

  function request(urlOrOptions, optionsOrCb, cb) {
    const req = new ClientRequest(urlOrOptions, optionsOrCb, cb);
    return req;
  }
  function get(urlOrOptions, optionsOrCb, cb) {
    const req = request(urlOrOptions, optionsOrCb, cb);
    req.end();
    return req;
  }

  // ---------------------------------------------------------------------
  // Server
  // ---------------------------------------------------------------------
  class Server extends EventEmitter {
    constructor(options, requestListener) {
      super();
      if (typeof options === "function") {
        requestListener = options;
        options = {};
      }
      this._options = options || {};
      this._listening = false;
      if (requestListener) this.on("request", requestListener);
      this.timeout = 0;
      this.headersTimeout = 60000;
      this.requestTimeout = 300000;
      this.keepAliveTimeout = 5000;
      this.maxHeadersCount = null;
    }
    listen(...args) {
      let port;
      let cb;
      for (const a of args) {
        if (typeof a === "function") cb = a;
        else if (typeof a === "number" || typeof a === "string") port = a;
      }
      this._port = port || 0;
      this._listening = true;
      require("internal/http-bridge").registerServer(this, "http");
      if (cb) this.once("listening", cb);
      queueMicrotask(() => this.emit("listening"));
      return this;
    }
    close(cb) {
      this._listening = false;
      require("internal/http-bridge").unregisterServer(this);
      if (cb) queueMicrotask(() => cb());
      queueMicrotask(() => this.emit("close"));
      return this;
    }
    address() {
      if (!this._listening) return null;
      return { address: "0.0.0.0", port: this._port || 0, family: "IPv4" };
    }
    setTimeout(ms, cb) {
      this.timeout = ms;
      if (cb) this.on("timeout", cb);
      return this;
    }
    getConnections(cb) {
      queueMicrotask(() => cb(null, 0));
    }
  }

  function createServer(options, requestListener) {
    return new Server(options, requestListener);
  }

  class Agent {
    constructor(options) {
      this.options = options || {};
      this.maxSockets = (options && options.maxSockets) || Infinity;
      this.maxFreeSockets = (options && options.maxFreeSockets) || 256;
      this.sockets = {};
      this.freeSockets = {};
      this.requests = {};
    }
    destroy() {}
  }
  const globalAgent = new Agent();

  // ---------------------------------------------------------------------
  // _internal — the contract internal/http-bridge relies on
  // ---------------------------------------------------------------------
  function createIncomingMessage(reqShape) {
    const im = new IncomingMessage({ encrypted: reqShape.url.startsWith("https:") });
    im.method = reqShape.method;
    let path = reqShape.url;
    try {
      const u = new URL(reqShape.url);
      path = u.pathname + u.search;
      im.headers.host = u.host;
    } catch (_e) {
      // reqShape.url already relative
    }
    im.url = path;
    setHeadersFromPlainObject(im, reqShape.headers);
    if (im.headers.host === undefined && reqShape.headers && reqShape.headers.host) {
      im.headers.host = reqShape.headers.host;
    }
    im._pendingBody = reqShape.body || new Uint8Array(0);
    return im;
  }

  function createServerResponse(req, onComplete) {
    return new ServerResponse(req, onComplete);
  }

  module.exports = {
    METHODS,
    STATUS_CODES,
    Agent,
    globalAgent,
    Server,
    IncomingMessage,
    OutgoingMessage,
    ServerResponse,
    ClientRequest,
    createServer,
    request,
    get,
    validateHeaderName,
    validateHeaderValue,
    maxHeaderSize: 16384,
    setMaxIdleHTTPParsers() {},
    _internal: { createIncomingMessage, createServerResponse },
  };
});

__nanoNodeRegister("https", function (module, exports, require) {
  const http = require("http");
  const { Agent: HttpAgent } = http;

  class Agent extends HttpAgent {}
  const globalAgent = new Agent();

  function request(urlOrOptions, optionsOrCb, cb) {
    let options = urlOrOptions;
    if (typeof urlOrOptions === "string" || urlOrOptions instanceof URL) {
      const u = typeof urlOrOptions === "string" ? new URL(urlOrOptions) : urlOrOptions;
      options = { hostname: u.hostname, port: u.port || 443, path: u.pathname + u.search, protocol: "https:" };
      if (typeof optionsOrCb === "object" && optionsOrCb !== null) Object.assign(options, optionsOrCb);
      else if (typeof optionsOrCb === "function") cb = optionsOrCb;
    } else {
      options = { protocol: "https:", port: 443, ...urlOrOptions };
      if (typeof optionsOrCb === "function") cb = optionsOrCb;
    }
    return http.request(options, cb);
  }
  function get(urlOrOptions, optionsOrCb, cb) {
    const req = request(urlOrOptions, optionsOrCb, cb);
    req.end();
    return req;
  }

  function createServer(options, requestListener) {
    const server = http.createServer(options, requestListener);
    const origListen = server.listen.bind(server);
    server.listen = (...args) => {
      const result = origListen(...args);
      require("internal/http-bridge").registerServer(server, "https");
      return result;
    };
    return server;
  }

  module.exports = { Agent, globalAgent, request, get, createServer, Server: http.Server };
});
