"use strict";
// node:http2 — client over global fetch (protocol negotiation is handled
// by the platform; this module provides the API surface), server bridges
// through internal/http-bridge exactly like node:http's Server.
__nanoNodeRegister("http2", function (module, exports, require) {
  const EventEmitter = require("events");
  const http = require("http");

  const constants = Object.freeze({
    NGHTTP2_SESSION_SERVER: 0, NGHTTP2_SESSION_CLIENT: 1,
    NGHTTP2_NO_ERROR: 0, NGHTTP2_PROTOCOL_ERROR: 1, NGHTTP2_INTERNAL_ERROR: 2,
    NGHTTP2_FLOW_CONTROL_ERROR: 3, NGHTTP2_SETTINGS_TIMEOUT: 4,
    NGHTTP2_STREAM_CLOSED: 5, NGHTTP2_FRAME_SIZE_ERROR: 6, NGHTTP2_REFUSED_STREAM: 7,
    NGHTTP2_CANCEL: 8, NGHTTP2_COMPRESSION_ERROR: 9, NGHTTP2_CONNECT_ERROR: 10,
    NGHTTP2_ENHANCE_YOUR_CALM: 11, NGHTTP2_INADEQUATE_SECURITY: 12,
    NGHTTP2_HTTP_1_1_REQUIRED: 13,
    NGHTTP2_FLAG_NONE: 0, NGHTTP2_FLAG_END_STREAM: 1, NGHTTP2_FLAG_END_HEADERS: 4,
    NGHTTP2_FLAG_ACK: 1, NGHTTP2_FLAG_PADDED: 8, NGHTTP2_FLAG_PRIORITY: 32,
    HTTP2_HEADER_STATUS: ":status", HTTP2_HEADER_METHOD: ":method",
    HTTP2_HEADER_AUTHORITY: ":authority", HTTP2_HEADER_SCHEME: ":scheme",
    HTTP2_HEADER_PATH: ":path", HTTP2_HEADER_PROTOCOL: ":protocol",
    HTTP2_HEADER_ACCEPT_ENCODING: "accept-encoding", HTTP2_HEADER_ACCEPT_LANGUAGE: "accept-language",
    HTTP2_HEADER_CONTENT_TYPE: "content-type", HTTP2_HEADER_CONTENT_LENGTH: "content-length",
    HTTP2_HEADER_USER_AGENT: "user-agent", HTTP2_HEADER_HOST: "host",
    HTTP2_HEADER_COOKIE: "cookie", HTTP2_HEADER_SET_COOKIE: "set-cookie",
    HTTP2_METHOD_GET: "GET", HTTP2_METHOD_POST: "POST", HTTP2_METHOD_PUT: "PUT",
    HTTP2_METHOD_DELETE: "DELETE", HTTP2_METHOD_HEAD: "HEAD", HTTP2_METHOD_OPTIONS: "OPTIONS",
    HTTP_STATUS_OK: 200, HTTP_STATUS_NOT_FOUND: 404, HTTP_STATUS_INTERNAL_SERVER_ERROR: 500,
  });

  const sensitiveHeaders = Symbol("nodejs.http2.sensitiveHeaders");

  const DEFAULT_SETTINGS = {
    headerTableSize: 4096,
    enablePush: true,
    initialWindowSize: 65535,
    maxFrameSize: 16384,
    maxConcurrentStreams: 4294967295,
    maxHeaderListSize: 65535,
    enableConnectProtocol: false,
  };

  function getDefaultSettings() {
    return { ...DEFAULT_SETTINGS };
  }

  // Real RFC 7540 §6.5.1 packing: 6-byte units (2-byte identifier + 4-byte value).
  const SETTINGS_IDS = {
    headerTableSize: 1, enablePush: 2, maxConcurrentStreams: 3,
    initialWindowSize: 4, maxFrameSize: 5, maxHeaderListSize: 6,
    enableConnectProtocol: 8,
  };
  function getPackedSettings(settings) {
    // Real Node packs exactly the keys present on the input object — no
    // forced merge with getDefaultSettings() — in Object.keys() order
    // restricted to recognized SETTINGS_IDS.
    const input = settings || {};
    const keys = Object.keys(SETTINGS_IDS).filter((k) => Object.prototype.hasOwnProperty.call(input, k));
    const buf = new Uint8Array(keys.length * 6);
    let offset = 0;
    for (const key of keys) {
      const id = SETTINGS_IDS[key];
      const value = typeof input[key] === "boolean" ? (input[key] ? 1 : 0) : input[key];
      buf[offset] = (id >> 8) & 0xff;
      buf[offset + 1] = id & 0xff;
      buf[offset + 2] = (value >>> 24) & 0xff;
      buf[offset + 3] = (value >>> 16) & 0xff;
      buf[offset + 4] = (value >>> 8) & 0xff;
      buf[offset + 5] = value & 0xff;
      offset += 6;
    }
    const { Buffer } = require("buffer");
    return Buffer.from(buf);
  }
  function getUnpackedSettings(buf) {
    const idToKey = Object.fromEntries(Object.entries(SETTINGS_IDS).map(([k, v]) => [v, k]));
    const out = {};
    for (let i = 0; i + 6 <= buf.length; i += 6) {
      const id = (buf[i] << 8) | buf[i + 1];
      const value = (buf[i + 2] * 0x1000000) + (buf[i + 3] << 16) + (buf[i + 4] << 8) + buf[i + 5];
      const key = idToKey[id];
      if (key) {
        out[key] = key === "enablePush" || key === "enableConnectProtocol" ? !!value : value;
      }
    }
    return out;
  }

  class Http2Stream extends EventEmitter {
    constructor(session) {
      super();
      this.session = session;
      this.destroyed = false;
      this.closed = false;
      this.rstCode = undefined;
      this.aborted = false;
      this.pending = false;
    }
    close(code, callback) {
      this.rstCode = code || constants.NGHTTP2_NO_ERROR;
      this.closed = true;
      if (callback) this.once("close", callback);
      queueMicrotask(() => this.emit("close"));
    }
    destroy(err) {
      this.destroyed = true;
      if (err) queueMicrotask(() => this.emit("error", err));
    }
    priority() {}
  }

  class ClientHttp2Stream extends Http2Stream {
    constructor(session, requestHeaders) {
      super(session);
      this._chunks = [];
      this._ended = false;
      this._headers = requestHeaders;
    }
    end(data) {
      if (data !== undefined) this._chunks.push(data);
      this._dispatch();
    }
    write(data, encoding, cb) {
      this._chunks.push(data);
      if (typeof encoding === "function") cb = encoding;
      if (cb) queueMicrotask(cb);
      return true;
    }
    async _dispatch() {
      const { Buffer } = require("buffer");
      const h = this._headers;
      const authority = h[constants.HTTP2_HEADER_AUTHORITY] || h.host;
      const scheme = h[constants.HTTP2_HEADER_SCHEME] || "https";
      const path = h[constants.HTTP2_HEADER_PATH] || "/";
      const method = h[constants.HTTP2_HEADER_METHOD] || "GET";
      const url = `${scheme}://${authority}${path}`;
      const headers = {};
      for (const [k, v] of Object.entries(h)) {
        if (!k.startsWith(":")) headers[k] = v;
      }
      const body = this._chunks.length > 0 && method !== "GET" && method !== "HEAD"
        ? Buffer.concat(this._chunks.map((c) => (typeof c === "string" ? Buffer.from(c) : Buffer.from(c))))
        : undefined;
      try {
        const response = await fetch(url, { method, headers, body });
        const respHeaders = { [constants.HTTP2_HEADER_STATUS]: response.status };
        response.headers.forEach((v, k) => {
          respHeaders[k] = v;
        });
        this.emit("response", respHeaders, 0);
        const buf = new Uint8Array(await response.arrayBuffer());
        if (buf.length > 0) this.emit("data", Buffer.from(buf));
        this.emit("end");
        queueMicrotask(() => this.emit("close"));
      } catch (err) {
        this.emit("error", err);
      }
    }
    setEncoding() {}
    resume() {}
    pause() {}
  }

  class ClientHttp2Session extends EventEmitter {
    constructor(authority, options) {
      super();
      this._authority = typeof authority === "string" ? authority : authority.toString();
      this.closed = false;
      this.destroyed = false;
      this.alpnProtocol = "h2";
      this.socket = { encrypted: this._authority.startsWith("https:") };
      this.encrypted = this.socket.encrypted;
      this.type = constants.NGHTTP2_SESSION_CLIENT;
      queueMicrotask(() => this.emit("connect", this, {}));
    }
    request(headers, options) {
      const merged = { ...headers };
      if (!merged[constants.HTTP2_HEADER_AUTHORITY]) {
        merged[constants.HTTP2_HEADER_AUTHORITY] = new URL(this._authority).host;
      }
      if (!merged[constants.HTTP2_HEADER_SCHEME]) {
        merged[constants.HTTP2_HEADER_SCHEME] = new URL(this._authority).protocol.replace(":", "");
      }
      if (!merged[constants.HTTP2_HEADER_METHOD]) {
        merged[constants.HTTP2_HEADER_METHOD] = "GET";
      }
      if (!merged[constants.HTTP2_HEADER_PATH]) {
        merged[constants.HTTP2_HEADER_PATH] = "/";
      }
      const stream = new ClientHttp2Stream(this, merged);
      if (options && options.endStream) {
        queueMicrotask(() => stream.end());
      }
      return stream;
    }
    close(callback) {
      this.closed = true;
      if (callback) this.once("close", callback);
      queueMicrotask(() => this.emit("close"));
    }
    destroy(err) {
      this.destroyed = true;
      if (err) queueMicrotask(() => this.emit("error", err));
      this.close();
    }
    goaway(code, lastStreamID, opaqueData) {
      queueMicrotask(() => this.emit("goaway", code || 0, lastStreamID || 0, opaqueData));
    }
    ping(payload, callback) {
      const cb = typeof payload === "function" ? payload : callback;
      queueMicrotask(() => cb(null, 0, new Uint8Array(8)));
      return true;
    }
    settings(settings, callback) {
      if (callback) queueMicrotask(() => callback(null, settings || {}, 0));
      return true;
    }
    setTimeout(ms, cb) {
      if (cb) this.once("timeout", cb);
    }
    ref() {}
    unref() {}
  }

  function connect(authority, options, listener) {
    if (typeof options === "function") {
      listener = options;
      options = {};
    }
    const session = new ClientHttp2Session(authority, options);
    if (listener) session.once("connect", listener);
    return session;
  }

  // ---------------------------------------------------------------------
  // Server (compat API): wraps node:http's bridge-backed Server, adding
  // http2-shaped request/response wrappers plus a 'stream' event.
  // ---------------------------------------------------------------------
  class Http2ServerRequest extends EventEmitter {
    constructor(incomingMessage, stream) {
      super();
      this.stream = stream;
      this._im = incomingMessage;
      this.headers = { ...incomingMessage.headers, [constants.HTTP2_HEADER_PATH]: incomingMessage.url, [constants.HTTP2_HEADER_METHOD]: incomingMessage.method };
      this.method = incomingMessage.method;
      this.url = incomingMessage.url;
      this.httpVersion = "2.0";
      this.socket = incomingMessage.socket;
      this.aborted = false;
      this.complete = incomingMessage.complete;
      incomingMessage.on("data", (chunk) => this.emit("data", chunk));
      incomingMessage.on("end", () => this.emit("end"));
      incomingMessage.on("error", (err) => this.emit("error", err));
    }
    setTimeout(ms, cb) {
      this._im.setTimeout(ms, cb);
      return this;
    }
    [Symbol.asyncIterator]() {
      return this._im[Symbol.asyncIterator] ? this._im[Symbol.asyncIterator]() : this._im.pipe ? this._im : undefined;
    }
  }

  class Http2ServerResponse extends EventEmitter {
    constructor(serverResponse, stream) {
      super();
      this.stream = stream;
      this._res = serverResponse;
      this.headersSent = false;
      this.finished = false;
      serverResponse.on("finish", () => {
        this.finished = true;
        this.emit("finish");
      });
      serverResponse.on("close", () => this.emit("close"));
    }
    get statusCode() {
      return this._res.statusCode;
    }
    set statusCode(v) {
      this._res.statusCode = v;
    }
    setHeader(name, value) {
      this._res.setHeader(name, value);
      return this;
    }
    getHeader(name) {
      return this._res.getHeader(name);
    }
    getHeaders() {
      return this._res.getHeaders();
    }
    hasHeader(name) {
      return this._res.hasHeader(name);
    }
    removeHeader(name) {
      this._res.removeHeader(name);
    }
    writeHead(status, headers) {
      this.headersSent = true;
      this._res.writeHead(status, headers);
      return this;
    }
    write(chunk, encoding, cb) {
      this.headersSent = true;
      return this._res.write(chunk, encoding, cb);
    }
    end(chunk, encoding, cb) {
      this.headersSent = true;
      this.finished = true;
      return this._res.end(chunk, encoding, cb);
    }
    createPushResponse(headers, callback) {
      queueMicrotask(() => callback(new Error("Push streams are not supported by the NANO runtime")));
    }
    stream_respond(headers, options) {
      this.writeHead(headers[constants.HTTP2_HEADER_STATUS] || 200, headers);
    }
  }

  class ServerHttp2Stream extends Http2Stream {
    constructor(session, request, response) {
      super(session);
      this._response = response;
    }
    respond(headers, options) {
      const status = (headers && headers[constants.HTTP2_HEADER_STATUS]) || 200;
      const plain = { ...headers };
      delete plain[constants.HTTP2_HEADER_STATUS];
      this._response.writeHead(status, plain);
      if (options && options.endStream) {
        this._response.end();
      }
    }
    respondWithFile() {
      throw require("internal/errors").unsupported("ServerHttp2Stream.respondWithFile");
    }
    additionalHeaders() {}
    end(data) {
      this._response.end(data);
    }
    write(data, encoding, cb) {
      return this._response.write(data, encoding, cb);
    }
  }

  function createServer(options, requestListener) {
    if (typeof options === "function") {
      requestListener = options;
      options = {};
    }
    const server = http.createServer(options);
    if (requestListener) server.on("request", requestListener);
    const originalEmit = server.emit.bind(server);
    server.emit = (event, ...args) => {
      if (event === "request") {
        const [im, sr] = args;
        const stream = new ServerHttp2Stream(server, im, sr);
        const req2 = new Http2ServerRequest(im, stream);
        const res2 = new Http2ServerResponse(sr, stream);
        originalEmit("stream", stream, req2.headers, 0);
        return originalEmit("request", req2, res2);
      }
      return originalEmit(event, ...args);
    };
    return server;
  }
  function createSecureServer(options, requestListener) {
    return createServer(options, requestListener);
  }

  module.exports = {
    constants,
    sensitiveHeaders,
    getDefaultSettings,
    getPackedSettings,
    getUnpackedSettings,
    connect,
    createServer,
    createSecureServer,
    Http2Session: EventEmitter,
    ClientHttp2Session,
    ClientHttp2Stream,
    Http2Stream,
    ServerHttp2Stream,
    Http2ServerRequest,
    Http2ServerResponse,
  };
});
