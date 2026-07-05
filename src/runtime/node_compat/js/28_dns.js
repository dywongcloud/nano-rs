"use strict";
// node:dns, node:dns/promises — dns.lookup via __nano_node_host.dnsLookup;
// dns.resolve* via DNS-over-HTTPS (fetch to Cloudflare's resolver), since
// the isolate sandbox has no raw UDP socket access (CONTRACT.md §6).
__nanoNodeRegister("dns", function (module, exports, require) {
  const { makeError } = require("internal/errors");
  const host = globalThis.__nano_node_host;

  const NODATA = "ENODATA", FORMERR = "EFORMERR", SERVFAIL = "ESERVFAIL",
    NOTFOUND = "ENOTFOUND", NOTIMP = "ENOTIMP", REFUSED = "EREFUSED",
    BADQUERY = "EBADQUERY", BADNAME = "EBADNAME", BADFAMILY = "EBADFAMILY",
    BADRESP = "EBADRESP", CONNREFUSED = "ECONNREFUSED", TIMEOUT = "ETIMEOUT",
    EOF = "EOF", FILE = "EFILE", NOMEM = "ENOMEM", DESTRUCTION = "EDESTRUCTION",
    BADSTR = "EBADSTR", BADFLAGS = "EBADFLAGS", NONAME = "ENONAME",
    BADHINTS = "EBADHINTS", NOTINITIALIZED = "ENOTINITIALIZED",
    LOADIPHLPAPI = "ELOADIPHLPAPI", ADDRGETNETWORKPARAMS = "EADDRGETNETWORKPARAMS",
    CANCELLED = "ECANCELLED";

  const RR_TYPE_NUM = { A: 1, NS: 2, CNAME: 5, SOA: 6, PTR: 12, MX: 15, TXT: 16, AAAA: 28, SRV: 33, NAPTR: 35, CAA: 257 };

  function dohEndpoint() {
    return "https://cloudflare-dns.com/dns-query";
  }

  async function dohQuery(name, type, fetchImpl) {
    const url = `${dohEndpoint()}?name=${encodeURIComponent(name)}&type=${type}`;
    const doFetch = fetchImpl || fetch;
    let response;
    try {
      response = await doFetch(url, { headers: { accept: "application/dns-json" } });
    } catch (e) {
      const err = new Error(`queryA ${SERVFAIL} ${name}`);
      err.code = SERVFAIL;
      err.syscall = "queryA";
      err.hostname = name;
      throw err;
    }
    if (!response.ok) {
      const err = new Error(`query${type} ${SERVFAIL} ${name}`);
      err.code = SERVFAIL;
      err.syscall = "query" + type;
      err.hostname = name;
      throw err;
    }
    const json = await response.json();
    if (json.Status === 3 /* NXDOMAIN */) {
      const err = new Error(`query${type} ${NOTFOUND} ${name}`);
      err.code = NOTFOUND;
      err.syscall = "query" + type;
      err.hostname = name;
      throw err;
    }
    if (json.Status !== 0) {
      const err = new Error(`query${type} ${SERVFAIL} ${name}`);
      err.code = SERVFAIL;
      err.syscall = "query" + type;
      err.hostname = name;
      throw err;
    }
    const wantType = RR_TYPE_NUM[type];
    const answers = (json.Answer || []).filter((a) => a.type === wantType);
    if (answers.length === 0) {
      const err = new Error(`query${type} ${NODATA} ${name}`);
      err.code = NODATA;
      err.syscall = "query" + type;
      err.hostname = name;
      throw err;
    }
    return answers;
  }

  function lookup(hostname, options, callback) {
    if (typeof options === "function") {
      callback = options;
      options = {};
    } else if (typeof options === "number") {
      options = { family: options };
    }
    options = options || {};
    if (typeof callback !== "function") {
      throw makeError(TypeError, "ERR_INVALID_CALLBACK", "Callback must be a function");
    }
    queueMicrotask(() => {
      try {
        const results = host.dnsLookup(hostname, options.family || 0);
        if (options.all) {
          callback(null, results);
        } else {
          callback(null, results[0].address, results[0].family);
        }
      } catch (e) {
        callback(e);
      }
    });
  }

  function lookupService(address, port, callback) {
    queueMicrotask(() => {
      callback(null, address, "unknown");
    });
  }

  function makeResolver(fetchImpl) {
    function resolve4(name, options, cb) {
      if (typeof options === "function") { cb = options; options = {}; }
      const ttl = options && options.ttl;
      dohQuery(name, "A", fetchImpl).then(
        (answers) => cb(null, ttl ? answers.map((a) => ({ address: a.data, ttl: a.TTL })) : answers.map((a) => a.data)),
        (err) => cb(err)
      );
    }
    function resolve6(name, options, cb) {
      if (typeof options === "function") { cb = options; options = {}; }
      const ttl = options && options.ttl;
      dohQuery(name, "AAAA", fetchImpl).then(
        (answers) => cb(null, ttl ? answers.map((a) => ({ address: a.data, ttl: a.TTL })) : answers.map((a) => a.data)),
        (err) => cb(err)
      );
    }
    function resolveCname(name, cb) {
      dohQuery(name, "CNAME", fetchImpl).then(
        (answers) => cb(null, answers.map((a) => a.data.replace(/\.$/, ""))),
        (err) => cb(err)
      );
    }
    function resolveNs(name, cb) {
      dohQuery(name, "NS", fetchImpl).then(
        (answers) => cb(null, answers.map((a) => a.data.replace(/\.$/, ""))),
        (err) => cb(err)
      );
    }
    function resolvePtr(name, cb) {
      dohQuery(name, "PTR", fetchImpl).then(
        (answers) => cb(null, answers.map((a) => a.data.replace(/\.$/, ""))),
        (err) => cb(err)
      );
    }
    function resolveMx(name, cb) {
      dohQuery(name, "MX", fetchImpl).then(
        (answers) => cb(null, answers.map((a) => {
          const [priority, exchange] = a.data.split(" ");
          return { priority: Number(priority), exchange: exchange.replace(/\.$/, "") };
        })),
        (err) => cb(err)
      );
    }
    function resolveTxt(name, cb) {
      dohQuery(name, "TXT", fetchImpl).then(
        (answers) => cb(null, answers.map((a) => [a.data.replace(/^"|"$/g, "")])),
        (err) => cb(err)
      );
    }
    function resolveSrv(name, cb) {
      dohQuery(name, "SRV", fetchImpl).then(
        (answers) => cb(null, answers.map((a) => {
          const [priority, weight, port, target] = a.data.split(" ");
          return { priority: Number(priority), weight: Number(weight), port: Number(port), name: target.replace(/\.$/, "") };
        })),
        (err) => cb(err)
      );
    }
    function resolveSoa(name, cb) {
      dohQuery(name, "SOA", fetchImpl).then(
        (answers) => {
          const parts = answers[0].data.split(" ");
          cb(null, {
            nsname: parts[0].replace(/\.$/, ""),
            hostmaster: parts[1].replace(/\.$/, ""),
            serial: Number(parts[2]),
            refresh: Number(parts[3]),
            retry: Number(parts[4]),
            expire: Number(parts[5]),
            minttl: Number(parts[6]),
          });
        },
        (err) => cb(err)
      );
    }
    function resolveNaptr(name, cb) {
      dohQuery(name, "NAPTR", fetchImpl).then(
        (answers) => cb(null, answers.map((a) => {
          const m = /^(\d+)\s+(\d+)\s+"([^"]*)"\s+"([^"]*)"\s+"([^"]*)"\s+(.*)$/.exec(a.data);
          if (!m) return { order: 0, preference: 0, flags: "", service: "", regexp: "", replacement: a.data };
          return {
            order: Number(m[1]), preference: Number(m[2]), flags: m[3],
            service: m[4], regexp: m[5], replacement: m[6].replace(/\.$/, ""),
          };
        })),
        (err) => cb(err)
      );
    }
    function resolveCaa(name, cb) {
      dohQuery(name, "CAA", fetchImpl).then(
        (answers) => cb(null, answers.map((a) => {
          const m = /^(\d+)\s+(\w+)\s+"(.*)"$/.exec(a.data);
          if (!m) return { critical: 0, issue: a.data };
          const key = m[2];
          return { critical: Number(m[1]), [key]: m[3] };
        })),
        (err) => cb(err)
      );
    }
    function resolveAny(name, cb) {
      cb(makeError(Error, "ERR_UNSUPPORTED_OPERATION", "resolveAny (ANY queries) is not supported over DoH"));
    }
    function reverse(ip, cb) {
      const arpaName = ip.split(".").reverse().join(".") + ".in-addr.arpa";
      resolvePtr(arpaName, cb);
    }

    return {
      resolve4, resolve6, resolveCname, resolveNs, resolvePtr, resolveMx,
      resolveTxt, resolveSrv, resolveSoa, resolveNaptr, resolveCaa, resolveAny, reverse,
      resolve(name, rrtype, cb) {
        if (typeof rrtype === "function") { cb = rrtype; rrtype = "A"; }
        const fn = { A: resolve4, AAAA: resolve6, CNAME: resolveCname, MX: resolveMx, NS: resolveNs, TXT: resolveTxt, SRV: resolveSrv, PTR: resolvePtr, SOA: resolveSoa, NAPTR: resolveNaptr, CAA: resolveCaa }[rrtype];
        if (!fn) return cb(makeError(Error, "ERR_INVALID_ARG_VALUE", "Unknown rrtype: " + rrtype));
        return fn(name, cb);
      },
    };
  }

  class Resolver {
    constructor(options) {
      this._servers = ["1.1.1.1", "1.0.0.1"];
      this._fetch = options && options.fetch;
      Object.assign(this, makeResolver(this._fetch));
      this._pending = new Set();
    }
    setServers(servers) {
      this._servers = servers.slice();
    }
    getServers() {
      return this._servers.slice();
    }
    setLocalAddress() {}
    cancel() {
      // Best-effort: DoH requests already in flight cannot be aborted without
      // the caller's own AbortController; documented limitation.
    }
  }

  let defaultResultOrder = "verbatim";

  const defaultResolver = new Resolver();

  module.exports = {
    lookup,
    lookupService,
    Resolver,
    promises: null, // set lazily below
    setServers(servers) { defaultResolver.setServers(servers); },
    getServers() { return defaultResolver.getServers(); },
    setDefaultResultOrder(order) { defaultResultOrder = order; },
    getDefaultResultOrder() { return defaultResultOrder; },
    NODATA, FORMERR, SERVFAIL, NOTFOUND, NOTIMP, REFUSED, BADQUERY, BADNAME,
    BADFAMILY, BADRESP, CONNREFUSED, TIMEOUT, EOF, FILE, NOMEM, DESTRUCTION,
    BADSTR, BADFLAGS, NONAME, BADHINTS, NOTINITIALIZED, LOADIPHLPAPI,
    ADDRGETNETWORKPARAMS, CANCELLED,
    ADDRCONFIG: 0, ALL: 0x10, V4MAPPED: 0x8,
    ...makeResolver(),
  };

  Object.defineProperty(module.exports, "promises", {
    configurable: true,
    enumerable: true,
    get() {
      return require("dns/promises");
    },
  });
});

__nanoNodeRegister("dns/promises", function (module, exports, require) {
  const dns = require("dns");

  function promisify1(fn) {
    return (...args) => new Promise((resolve, reject) => {
      fn(...args, (err, result) => (err ? reject(err) : resolve(result)));
    });
  }

  class Resolver extends dns.Resolver {
    constructor(options) {
      super(options);
    }
  }
  for (const method of ["resolve4", "resolve6", "resolveCname", "resolveNs", "resolvePtr",
    "resolveMx", "resolveTxt", "resolveSrv", "resolveSoa", "resolveNaptr", "resolveCaa",
    "resolveAny", "reverse", "resolve"]) {
    const orig = Resolver.prototype[method];
    Resolver.prototype[method] = function (...args) {
      return new Promise((resolve, reject) => {
        orig.call(this, ...args, (err, result) => (err ? reject(err) : resolve(result)));
      });
    };
  }

  module.exports = {
    Resolver,
    lookup: (hostname, options) => new Promise((resolve, reject) => {
      dns.lookup(hostname, options || {}, (err, address, family) => {
        if (err) reject(err);
        else if (options && options.all) resolve(address);
        else resolve({ address, family });
      });
    }),
    lookupService: promisify1(dns.lookupService),
    resolve4: promisify1(dns.resolve4),
    resolve6: promisify1(dns.resolve6),
    resolveCname: promisify1(dns.resolveCname),
    resolveNs: promisify1(dns.resolveNs),
    resolvePtr: promisify1(dns.resolvePtr),
    resolveMx: promisify1(dns.resolveMx),
    resolveTxt: promisify1(dns.resolveTxt),
    resolveSrv: promisify1(dns.resolveSrv),
    resolveSoa: promisify1(dns.resolveSoa),
    resolveNaptr: promisify1(dns.resolveNaptr),
    resolveCaa: promisify1(dns.resolveCaa),
    resolveAny: promisify1(dns.resolveAny),
    resolve: promisify1(dns.resolve),
    reverse: promisify1(dns.reverse),
    setServers: dns.setServers,
    getServers: dns.getServers,
    setDefaultResultOrder: dns.setDefaultResultOrder,
    getDefaultResultOrder: dns.getDefaultResultOrder,
  };
});
