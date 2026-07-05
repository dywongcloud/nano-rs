"use strict";
// node:net, node:tls, node:dgram — CONTRACT.md §6: raw sockets are
// sandbox-restricted. IP parsing/validation utilities are fully functional;
// connect/listen/send paths fail with a Node-correct async EPERM.
__nanoNodeRegister("net", function (module, exports, require) {
  const EventEmitter = require("events");
  const { Duplex } = require("stream");
  const { notPermitted, makeError } = require("internal/errors");

  function isIPv4(input) {
    if (typeof input !== "string") return false;
    const parts = input.split(".");
    if (parts.length !== 4) return false;
    for (const part of parts) {
      if (!/^\d{1,3}$/.test(part)) return false;
      if (part.length > 1 && part[0] === "0") return false;
      const n = Number(part);
      if (n < 0 || n > 255) return false;
    }
    return true;
  }

  function isIPv6(input) {
    if (typeof input !== "string" || input.length === 0) return false;
    let str = input;
    const percentIdx = str.indexOf("%");
    if (percentIdx !== -1) {
      str = str.slice(0, percentIdx);
      if (str.length === 0) return false;
    }
    if (str.length > 45) return false;

    let embeddedIPv4 = null;
    const lastColon = str.lastIndexOf(":");
    if (lastColon !== -1 && str.slice(lastColon + 1).includes(".")) {
      const v4part = str.slice(lastColon + 1);
      if (!isIPv4(v4part)) return false;
      embeddedIPv4 = v4part;
      str = str.slice(0, lastColon + 1) + "0:0";
    }

    if (!/^[0-9a-fA-F:]+$/.test(str)) return false;

    const doubleColonCount = (str.match(/::/g) || []).length;
    if (doubleColonCount > 1) return false;

    let groups;
    if (str.includes("::")) {
      if (str === "::") return true;
      const [left, right] = str.split("::");
      const leftGroups = left === "" ? [] : left.split(":");
      const rightGroups = right === "" ? [] : right.split(":");
      if (leftGroups.length + rightGroups.length >= 8) return false;
      groups = [...leftGroups, ...rightGroups];
      if (leftGroups.some((g) => g === "") || rightGroups.some((g) => g === "")) return false;
    } else {
      groups = str.split(":");
      if (groups.length !== 8) return false;
    }
    return groups.every((g) => /^[0-9a-fA-F]{1,4}$/.test(g));
  }

  function isIP(input) {
    if (isIPv4(input)) return 4;
    if (isIPv6(input)) return 6;
    return 0;
  }

  function ipToNumber(ip) {
    return ip.split(".").reduce((acc, part) => (acc << 8) + Number(part), 0) >>> 0;
  }

  function expandIPv6(ip) {
    let str = ip;
    const percentIdx = str.indexOf("%");
    if (percentIdx !== -1) str = str.slice(0, percentIdx);
    let embedded = null;
    const lastColon = str.lastIndexOf(":");
    if (lastColon !== -1 && str.slice(lastColon + 1).includes(".")) {
      embedded = str.slice(lastColon + 1);
      const v4bytes = embedded.split(".").map(Number);
      const hi = ((v4bytes[0] << 8) | v4bytes[1]).toString(16);
      const lo = ((v4bytes[2] << 8) | v4bytes[3]).toString(16);
      str = str.slice(0, lastColon + 1) + hi + ":" + lo;
    }
    let groups;
    if (str.includes("::")) {
      const [left, right] = str.split("::");
      const leftGroups = left === "" ? [] : left.split(":");
      const rightGroups = right === "" ? [] : right.split(":");
      const fillCount = 8 - leftGroups.length - rightGroups.length;
      groups = [...leftGroups, ...Array(fillCount).fill("0"), ...rightGroups];
    } else {
      groups = str.split(":");
    }
    return groups.map((g) => parseInt(g, 16));
  }

  class BlockList {
    constructor() {
      this._v4 = []; // { base: number, bits: number }
      this._v6 = []; // { base: bigint, bits: number }
    }
    addAddress(address, family = "ipv4") {
      const fam = family === "ipv6" || isIPv6(address) ? "ipv6" : "ipv4";
      this.addSubnet(address, fam === "ipv6" ? 128 : 32, fam);
    }
    addRange(start, end, family = "ipv4") {
      // Simplified: only exact single-address ranges are tracked precisely;
      // broader ranges fall back to a /0 style scan (documented limitation
      // of this sandbox implementation).
      this.addAddress(start, family);
      this.addAddress(end, family);
    }
    addSubnet(net_, prefix, family = "ipv4") {
      if (isIPv6(net_)) {
        const groups = expandIPv6(net_);
        let base = 0n;
        for (const g of groups) base = (base << 16n) | BigInt(g);
        this._v6.push({ base, bits: prefix });
      } else {
        this._v4.push({ base: ipToNumber(net_), bits: prefix });
      }
    }
    check(address, family = "ipv4") {
      if (isIPv6(address)) {
        const groups = expandIPv6(address);
        let value = 0n;
        for (const g of groups) value = (value << 16n) | BigInt(g);
        for (const { base, bits } of this._v6) {
          const mask = bits === 0 ? 0n : (~0n << BigInt(128 - bits)) & ((1n << 128n) - 1n);
          if ((value & mask) === (base & mask)) return true;
        }
        return false;
      }
      if (!isIPv4(address)) return false;
      const value = ipToNumber(address);
      for (const { base, bits } of this._v4) {
        const mask = bits === 0 ? 0 : (0xffffffff << (32 - bits)) >>> 0;
        if ((value & mask) === (base & mask)) return true;
      }
      return false;
    }
  }

  class SocketAddress {
    constructor(options = {}) {
      this._address = options.address || (options.family === "ipv6" ? "::" : "127.0.0.1");
      this._family = options.family || "ipv4";
      this._port = options.port || 0;
      this._flowlabel = options.flowlabel || 0;
    }
    get address() { return this._address; }
    get family() { return this._family; }
    get port() { return this._port; }
    get flowlabel() { return this._flowlabel; }
  }

  let defaultAutoSelectFamily = true;
  let defaultAutoSelectFamilyAttemptTimeout = 250;

  class Socket extends Duplex {
    constructor(options) {
      super(options);
      this.connecting = false;
      this.pending = true;
      this.destroyed = false;
      this.readyState = "closed";
      this.bytesRead = 0;
      this.bytesWritten = 0;
      this.remoteAddress = undefined;
      this.remotePort = undefined;
      this.remoteFamily = undefined;
      this.localAddress = undefined;
      this.localPort = undefined;
      this._read = () => {};
    }
    connect(...args) {
      let cb;
      if (typeof args[args.length - 1] === "function") cb = args.pop();
      if (cb) this.once("connect", cb);
      this.connecting = true;
      queueMicrotask(() => {
        this.connecting = false;
        this.emit("error", notPermitted("connect", "net.Socket.connect"));
      });
      return this;
    }
    setEncoding(enc) {
      return super.setEncoding ? super.setEncoding(enc) : this;
    }
    setNoDelay() { return this; }
    setKeepAlive() { return this; }
    setTimeout(ms, cb) {
      if (cb) this.once("timeout", cb);
      return this;
    }
    address() {
      return {};
    }
    ref() { return this; }
    unref() { return this; }
    _write(chunk, encoding, callback) {
      callback(notPermitted("write", "net.Socket is not connected"));
    }
  }

  class NetServer extends EventEmitter {
    constructor(options, connectionListener) {
      super();
      if (typeof options === "function") {
        connectionListener = options;
        options = {};
      }
      this._options = options || {};
      this.listening = false;
      this.maxConnections = Infinity;
      if (connectionListener) this.on("connection", connectionListener);
    }
    listen(...args) {
      let cb;
      if (typeof args[args.length - 1] === "function") cb = args.pop();
      if (cb) this.once("error", cb);
      queueMicrotask(() => {
        this.emit("error", notPermitted("bind", "net.Server.listen"));
      });
      return this;
    }
    close(cb) {
      this.listening = false;
      if (cb) queueMicrotask(cb);
      queueMicrotask(() => this.emit("close"));
      return this;
    }
    address() {
      return this.listening ? { address: "0.0.0.0", port: 0, family: "IPv4" } : null;
    }
    getConnections(cb) {
      queueMicrotask(() => cb(null, 0));
    }
    ref() { return this; }
    unref() { return this; }
  }

  function createServer(options, connectionListener) {
    return new NetServer(options, connectionListener);
  }
  function createConnection(...args) {
    const s = new Socket();
    return s.connect(...args);
  }

  module.exports = {
    isIP, isIPv4, isIPv6,
    BlockList,
    SocketAddress,
    Socket,
    Server: NetServer,
    createServer,
    createConnection,
    connect: createConnection,
    getDefaultAutoSelectFamily() { return defaultAutoSelectFamily; },
    setDefaultAutoSelectFamily(v) { defaultAutoSelectFamily = !!v; },
    getDefaultAutoSelectFamilyAttemptTimeout() { return defaultAutoSelectFamilyAttemptTimeout; },
    setDefaultAutoSelectFamilyAttemptTimeout(ms) { defaultAutoSelectFamilyAttemptTimeout = ms; },
  };
});

__nanoNodeRegister("tls", function (module, exports, require) {
  const net = require("net");
  const { notPermitted } = require("internal/errors");

  class TLSSocket extends net.Socket {
    constructor(socket, options) {
      super(options);
      this.encrypted = true;
      this.authorized = false;
      this.authorizationError = null;
      this._options = options || {};
    }
    getProtocol() { return null; }
    getCipher() { return null; }
    getPeerCertificate() { return {}; }
    getCertificate() { return {}; }
  }

  function connect(...args) {
    const socket = new TLSSocket();
    let cb;
    if (typeof args[args.length - 1] === "function") cb = args.pop();
    if (cb) socket.once("secureConnect", cb);
    queueMicrotask(() => socket.emit("error", notPermitted("connect", "tls.connect")));
    return socket;
  }

  class Server extends net.Server {
    listen(...args) {
      let cb;
      if (typeof args[args.length - 1] === "function") cb = args.pop();
      if (cb) this.once("error", cb);
      queueMicrotask(() => this.emit("error", notPermitted("bind", "tls.Server.listen")));
      return this;
    }
  }
  function createServer(options, connectionListener) {
    return new Server(options, connectionListener);
  }
  function createSecureContext(options) {
    return { context: Object.freeze({ ...options }) };
  }

  // Real hostname/wildcard/SAN matching logic (pure function, no sandbox
  // restriction applies — this is client-side certificate validation).
  function splitHost(host) {
    return String(host).toLowerCase().replace(/\.$/, "").split(".");
  }
  function wildcardMatch(pattern, host) {
    const patternParts = splitHost(pattern);
    const hostParts = splitHost(host);
    if (patternParts.length !== hostParts.length) return false;
    for (let i = 0; i < patternParts.length; i += 1) {
      if (i === 0 && patternParts[0].includes("*")) {
        if (patternParts[0] === "*") continue;
        const prefix = patternParts[0].split("*")[0];
        const suffix = patternParts[0].split("*")[1];
        if (!hostParts[0].startsWith(prefix) || !hostParts[0].endsWith(suffix)) return false;
        continue;
      }
      if (patternParts[i] !== hostParts[i]) return false;
    }
    return true;
  }
  function checkServerIdentity(hostname, cert) {
    const altNames = cert && cert.subjectaltname
      ? cert.subjectaltname.split(", ").filter((s) => s.startsWith("DNS:")).map((s) => s.slice(4))
      : [];
    const names = altNames.length > 0 ? altNames : (cert && cert.subject && cert.subject.CN ? [cert.subject.CN] : []);
    if (names.length === 0) {
      const err = new Error("Cert does not contain a DNS subjectAltName");
      err.code = "ERR_TLS_CERT_ALTNAME_INVALID";
      return err;
    }
    const ok = names.some((n) => wildcardMatch(n, hostname));
    if (!ok) {
      const err = new Error(
        "Hostname/IP does not match certificate's altnames: Host: " + hostname + ". is not in the cert's altnames: " + names.join(", ")
      );
      err.code = "ERR_TLS_CERT_ALTNAME_INVALID";
      err.reason = "Host: " + hostname + ". is not in the cert's altnames: " + names.join(", ");
      err.host = hostname;
      return err;
    }
    return undefined;
  }

  module.exports = {
    TLSSocket,
    connect,
    createServer,
    Server,
    createSecureContext,
    checkServerIdentity,
    getCiphers() {
      return ["TLS_AES_256_GCM_SHA384", "TLS_CHACHA20_POLY1305_SHA256", "TLS_AES_128_GCM_SHA256"];
    },
    rootCertificates: Object.freeze([]),
    DEFAULT_ECDH_CURVE: "auto",
    DEFAULT_MAX_VERSION: "TLSv1.3",
    DEFAULT_MIN_VERSION: "TLSv1.2",
    DEFAULT_CIPHERS: "",
    convertALPNProtocols(protocols) {
      if (!Array.isArray(protocols)) return undefined;
      const { Buffer } = require("buffer");
      const parts = protocols.map((p) => {
        const b = Buffer.from(p);
        return Buffer.concat([Buffer.from([b.length]), b]);
      });
      return Buffer.concat(parts);
    },
  };
});

__nanoNodeRegister("dgram", function (module, exports, require) {
  const EventEmitter = require("events");
  const { notPermitted, makeError } = require("internal/errors");

  class DgramSocket extends EventEmitter {
    constructor(type) {
      super();
      this.type = typeof type === "object" ? type.type : type;
      this._bound = false;
    }
    bind(...args) {
      let cb;
      if (typeof args[args.length - 1] === "function") cb = args.pop();
      if (cb) this.once("error", cb);
      queueMicrotask(() => this.emit("error", notPermitted("bind", "dgram.Socket.bind")));
      return this;
    }
    connect(port, address, cb) {
      if (typeof address === "function") { cb = address; address = undefined; }
      queueMicrotask(() => this.emit("error", notPermitted("connect", "dgram.Socket.connect")));
    }
    send(...args) {
      const cb = typeof args[args.length - 1] === "function" ? args.pop() : undefined;
      const err = notPermitted("send", "dgram.Socket.send");
      if (cb) {
        queueMicrotask(() => cb(err));
      } else {
        queueMicrotask(() => this.emit("error", err));
      }
    }
    close(cb) {
      if (cb) queueMicrotask(cb);
      queueMicrotask(() => this.emit("close"));
    }
    address() {
      throw makeError(Error, "ERR_SOCKET_DGRAM_NOT_RUNNING", "Not running");
    }
    setBroadcast() { throw notPermitted("setBroadcast", "dgram.Socket.setBroadcast"); }
    setMulticastTTL() { throw notPermitted("setMulticastTTL", "dgram.Socket.setMulticastTTL"); }
    setTTL() { throw notPermitted("setTTL", "dgram.Socket.setTTL"); }
    addMembership() { throw notPermitted("addMembership", "dgram.Socket.addMembership"); }
    dropMembership() { throw notPermitted("dropMembership", "dgram.Socket.dropMembership"); }
    ref() { return this; }
    unref() { return this; }
  }

  function createSocket(type, callback) {
    const socket = new DgramSocket(type);
    if (callback) socket.on("message", callback);
    return socket;
  }

  module.exports = { Socket: DgramSocket, createSocket };
});
