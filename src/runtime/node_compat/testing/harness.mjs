// Test harness for the NANO Node.js compatibility layer.
//
// Emulates the NANO isolate inside real Node.js: a vm context with dynamic
// code generation DISABLED (matching the runtime's eval ban), the WinterTC
// globals the Rust runtime binds, and a faithful __nano_node_host stub
// implemented over real Node crypto/zlib plus an in-memory VFS.
//
// Usage (from a test script):
//   import { createEnv } from "./harness.mjs";
//   const { require: nanoRequire, context } = createEnv([
//     "10_events.js", "21_stream.js",           // layer files to load (js/ dir)
//   ]);
//   const events = nanoRequire("node:events");
//
// Any builtin id NOT provided by the loaded layer files is backed by the
// REAL Node.js builtin, so modules can be tested in isolation against
// authentic peers before the whole layer exists.
import { readFileSync, readdirSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import vm from "node:vm";
import { createRequire } from "node:module";
import nodeCrypto from "node:crypto";
import nodeZlib from "node:zlib";
import nodeOs from "node:os";

const hereDir = path.dirname(fileURLToPath(import.meta.url));
const jsDir = path.join(hereDir, "..", "js");
const realRequire = createRequire(import.meta.url);

// Real-Node-backed ids used as fallbacks for not-yet-authored peers.
const FALLBACK_IDS = [
  "assert", "assert/strict", "buffer", "child_process", "cluster", "console",
  "constants", "crypto", "dgram", "diagnostics_channel", "dns", "dns/promises",
  "domain", "events", "fs", "fs/promises", "http", "http2", "https",
  "inspector", "module", "net", "os", "path", "path/posix", "path/win32",
  "perf_hooks", "process", "punycode", "querystring", "readline",
  "readline/promises", "repl", "stream", "stream/consumers", "stream/promises",
  "stream/web", "string_decoder", "sys", "timers", "timers/promises", "tls",
  "tty", "url", "util", "util/types", "v8", "vm", "worker_threads", "zlib",
];

function makeMemoryVfs() {
  const files = new Map(); // path -> { data: Uint8Array, mtimeMs, birthtimeMs }
  const dirs = new Set(["/"]);
  const norm = (p) => {
    if (typeof p !== "string" || p.length === 0) throw uv("EINVAL", "open", p);
    let s = p.startsWith("/") ? p : "/" + p;
    s = path.posix.normalize(s);
    return s === "/" ? s : s.replace(/\/+$/, "");
  };
  const uv = (code, syscall, p) => {
    const errnos = { ENOENT: -2, EEXIST: -17, ENOTDIR: -20, EISDIR: -21, EINVAL: -22, ENOTEMPTY: -39 };
    const e = new Error(`${code}: ${syscall} '${p}'`);
    e.code = code; e.errno = errnos[code] ?? -22; e.syscall = syscall; e.path = p;
    return e;
  };
  const parentOf = (p) => path.posix.dirname(p);
  const childrenOf = (p) => {
    const prefix = p === "/" ? "/" : p + "/";
    const names = new Set();
    for (const f of files.keys()) {
      if (f.startsWith(prefix)) names.add(f.slice(prefix.length).split("/")[0]);
    }
    for (const d of dirs) {
      if (d !== "/" && d.startsWith(prefix)) names.add(d.slice(prefix.length).split("/")[0]);
    }
    return [...names].sort();
  };
  return {
    read(p) {
      p = norm(p);
      const f = files.get(p);
      if (!f) throw uv("ENOENT", "open", p);
      return new Uint8Array(f.data);
    },
    write(p, data) {
      p = norm(p);
      if (dirs.has(p)) throw uv("EISDIR", "open", p);
      const now = Date.now();
      const prev = files.get(p);
      files.set(p, { data: new Uint8Array(data), mtimeMs: now, birthtimeMs: prev ? prev.birthtimeMs : now });
      let d = parentOf(p);
      while (!dirs.has(d)) { dirs.add(d); d = parentOf(d); }
    },
    exists(p) { p = norm(p); return files.has(p) || dirs.has(p); },
    unlink(p) { p = norm(p); if (!files.delete(p)) throw uv("ENOENT", "unlink", p); },
    mkdir(p, recursive) {
      p = norm(p);
      if (files.has(p)) throw uv("EEXIST", "mkdir", p);
      if (dirs.has(p)) { if (!recursive) throw uv("EEXIST", "mkdir", p); return; }
      const parent = parentOf(p);
      if (!dirs.has(parent)) {
        if (!recursive) throw uv("ENOENT", "mkdir", p);
        this.mkdir(parent, true);
      }
      dirs.add(p);
    },
    rmdir(p) {
      p = norm(p);
      if (!dirs.has(p)) throw files.has(p) ? uv("ENOTDIR", "rmdir", p) : uv("ENOENT", "rmdir", p);
      if (childrenOf(p).length > 0) throw uv("ENOTEMPTY", "rmdir", p);
      dirs.delete(p);
    },
    readdir(p) {
      p = norm(p);
      if (files.has(p)) throw uv("ENOTDIR", "scandir", p);
      if (!dirs.has(p)) {
        if (childrenOf(p).length === 0) throw uv("ENOENT", "scandir", p);
      }
      return childrenOf(p);
    },
    stat(p) {
      p = norm(p);
      const f = files.get(p);
      if (f) return { size: f.data.byteLength, mtimeMs: f.mtimeMs, birthtimeMs: f.birthtimeMs, isFile: true, isDirectory: false };
      if (dirs.has(p) || childrenOf(p).length > 0) {
        return { size: 0, mtimeMs: 0, birthtimeMs: 0, isFile: false, isDirectory: true };
      }
      throw uv("ENOENT", "stat", p);
    },
    rename(from, to) { const d = this.read(from); this.write(to, d); this.unlink(from); },
    copyFile(from, to) { this.write(to, this.read(from)); },
  };
}

function zlibKindSync(kind, data, level) {
  const opts = level >= 0 ? { level } : {};
  switch (kind) {
    case "gzip": return nodeZlib.gzipSync(data, opts);
    case "gunzip": return nodeZlib.gunzipSync(data);
    case "deflate": return nodeZlib.deflateSync(data, opts);
    case "inflate": return nodeZlib.inflateSync(data);
    case "deflateRaw": return nodeZlib.deflateRawSync(data, opts);
    case "inflateRaw": return nodeZlib.inflateRawSync(data);
    case "unzip": return nodeZlib.unzipSync(data);
    case "brotliCompress": return nodeZlib.brotliCompressSync(data);
    case "brotliDecompress": return nodeZlib.brotliDecompressSync(data);
    default: { const e = new Error("unknown zlib kind: " + kind); e.code = "ERR_INVALID_ARG_VALUE"; throw e; }
  }
}

function makeHost(vfs, envVars) {
  const zstreams = new Map();
  let znext = 1;
  const toU8 = (b) => new Uint8Array(b.buffer ? b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength) : b);
  const hashAlg = (a) => ({ md5: "md5", sha1: "sha1", sha224: "sha224", sha256: "sha256", sha384: "sha384", sha512: "sha512" }[a]);
  return {
    cryptoDigest: (alg, data) => {
      const a = hashAlg(alg);
      if (!a) { const e = new TypeError("Invalid digest: " + alg); e.code = "ERR_CRYPTO_INVALID_DIGEST"; throw e; }
      return toU8(nodeCrypto.createHash(a).update(data).digest());
    },
    cryptoHmac: (alg, key, data) => {
      const a = hashAlg(alg);
      if (!a) { const e = new TypeError("Invalid digest: " + alg); e.code = "ERR_CRYPTO_INVALID_DIGEST"; throw e; }
      return toU8(nodeCrypto.createHmac(a, key).update(data).digest());
    },
    cryptoPbkdf2: (pw, salt, iter, keylen, alg) => toU8(nodeCrypto.pbkdf2Sync(pw, salt, iter, keylen, alg)),
    cryptoScrypt: (pw, salt, N, r, p, keylen) =>
      toU8(nodeCrypto.scryptSync(pw, salt, keylen, { N, r, p, maxmem: 512 * 1024 * 1024 })),
    cryptoHkdf: (alg, ikm, salt, info, keylen) =>
      new Uint8Array(nodeCrypto.hkdfSync(alg, ikm, salt, info, keylen)),
    cryptoRandomBytes: (n) => toU8(nodeCrypto.randomBytes(n)),
    cryptoTimingSafeEqual: (a, b) => {
      if (a.byteLength !== b.byteLength) {
        const e = new RangeError("Input buffers must have the same byte length");
        e.code = "ERR_CRYPTO_TIMING_SAFE_EQUAL_LENGTH"; throw e;
      }
      return nodeCrypto.timingSafeEqual(a, b);
    },
    cryptoCipher: (op, algo, key, iv, data, aad, tag) => {
      const isGcm = algo.endsWith("-gcm");
      if (op === "encrypt") {
        const c = nodeCrypto.createCipheriv(algo, key, iv);
        if (isGcm && aad) c.setAAD(aad);
        const out = Buffer.concat([c.update(data), c.final()]);
        return { data: toU8(out), tag: isGcm ? toU8(c.getAuthTag()) : undefined };
      }
      const d = nodeCrypto.createDecipheriv(algo, key, iv);
      if (isGcm) {
        if (!tag) { const e = new Error("GCM decrypt requires auth tag"); e.code = "ERR_CRYPTO_INVALID_STATE"; throw e; }
        d.setAuthTag(tag);
        if (aad) d.setAAD(aad);
      }
      const out = Buffer.concat([d.update(data), d.final()]);
      return { data: toU8(out), tag: undefined };
    },
    cryptoRsaGenerate: (bits) => {
      const { privateKey, publicKey } = nodeCrypto.generateKeyPairSync("rsa", {
        modulusLength: bits,
        privateKeyEncoding: { type: "pkcs8", format: "pem" },
        publicKeyEncoding: { type: "spki", format: "pem" },
      });
      return { privatePem: privateKey, publicPem: publicKey };
    },
    cryptoRsaSign: (padding, hash, privatePem, data, saltLength) =>
      toU8(nodeCrypto.sign(hash, data, {
        key: privatePem,
        padding: padding === "pss" ? nodeCrypto.constants.RSA_PKCS1_PSS_PADDING : nodeCrypto.constants.RSA_PKCS1_PADDING,
        saltLength: padding === "pss" ? saltLength : undefined,
      })),
    cryptoRsaVerify: (padding, hash, publicPem, data, sig, saltLength) =>
      nodeCrypto.verify(hash, data, {
        key: publicPem,
        padding: padding === "pss" ? nodeCrypto.constants.RSA_PKCS1_PSS_PADDING : nodeCrypto.constants.RSA_PKCS1_PADDING,
        saltLength: padding === "pss" ? saltLength : undefined,
      }, sig),
    cryptoRsaEncrypt: (padding, hash, publicPem, data) =>
      toU8(nodeCrypto.publicEncrypt({
        key: publicPem,
        padding: padding === "oaep" ? nodeCrypto.constants.RSA_PKCS1_OAEP_PADDING : nodeCrypto.constants.RSA_PKCS1_PADDING,
        oaepHash: padding === "oaep" ? hash : undefined,
      }, data)),
    cryptoRsaDecrypt: (padding, hash, privatePem, data) =>
      toU8(nodeCrypto.privateDecrypt({
        key: privatePem,
        padding: padding === "oaep" ? nodeCrypto.constants.RSA_PKCS1_OAEP_PADDING : nodeCrypto.constants.RSA_PKCS1_PADDING,
        oaepHash: padding === "oaep" ? hash : undefined,
      }, data)),
    cryptoEcGenerate: (curve) => {
      const namedCurve = curve === "p384" ? "P-384" : "P-256";
      const { privateKey, publicKey } = nodeCrypto.generateKeyPairSync("ec", {
        namedCurve,
        privateKeyEncoding: { type: "pkcs8", format: "pem" },
        publicKeyEncoding: { type: "spki", format: "pem" },
      });
      return { privatePem: privateKey, publicPem: publicKey };
    },
    cryptoEcSign: (curve, hash, privatePem, data) =>
      toU8(nodeCrypto.sign(hash, data, { key: privatePem, dsaEncoding: "der" })),
    cryptoEcVerify: (curve, hash, publicPem, data, sig) =>
      nodeCrypto.verify(hash, data, { key: publicPem, dsaEncoding: "der" }, sig),
    cryptoEd25519Generate: () => {
      const { privateKey, publicKey } = nodeCrypto.generateKeyPairSync("ed25519");
      return {
        privatePkcs8: toU8(privateKey.export({ type: "pkcs8", format: "der" })),
        publicRaw: toU8(publicKey.export({ type: "spki", format: "der" })).slice(-32),
      };
    },
    cryptoEd25519Sign: (pkcs8, data) =>
      toU8(nodeCrypto.sign(null, data, nodeCrypto.createPrivateKey({ key: Buffer.from(pkcs8), type: "pkcs8", format: "der" }))),
    cryptoEd25519Verify: (publicRaw, data, sig) => {
      const spkiPrefix = Buffer.from("302a300506032b6570032100", "hex");
      const key = nodeCrypto.createPublicKey({ key: Buffer.concat([spkiPrefix, Buffer.from(publicRaw)]), type: "spki", format: "der" });
      return nodeCrypto.verify(null, data, key, sig);
    },
    zlibSync: (kind, data, level) => toU8(zlibKindSync(kind, data, level)),
    zlibCreate: (kind, level) => { const id = znext++; zstreams.set(id, { kind, level, chunks: [] }); return id; },
    zlibPush: (id, chunk, finish) => {
      const s = zstreams.get(id);
      if (!s) { const e = new Error("invalid zlib handle"); e.code = "ERR_ZLIB_INITIALIZATION_FAILED"; throw e; }
      s.chunks.push(Buffer.from(chunk));
      if (!finish) return new Uint8Array(0);
      zstreams.delete(id);
      return toU8(zlibKindSync(s.kind, Buffer.concat(s.chunks), s.level));
    },
    zlibFree: (id) => { zstreams.delete(id); },
    fsReadFile: (p) => vfs.read(p),
    fsWriteFile: (p, d) => vfs.write(p, d),
    fsExists: (p) => vfs.exists(p),
    fsUnlink: (p) => vfs.unlink(p),
    fsMkdir: (p, r) => vfs.mkdir(p, r),
    fsRmdir: (p) => vfs.rmdir(p),
    fsReaddir: (p) => vfs.readdir(p),
    fsStat: (p) => vfs.stat(p),
    fsRename: (a, b) => vfs.rename(a, b),
    fsCopyFile: (a, b) => vfs.copyFile(a, b),
    hrtime: () => { const ns = process.hrtime.bigint(); return { sec: Number(ns / 1000000000n), ns: Number(ns % 1000000000n) }; },
    memoryUsage: () => { const m = process.memoryUsage(); return { rss: m.rss, heapTotal: m.heapTotal, heapUsed: m.heapUsed, external: m.external }; },
    hostname: () => "tenant.example.com",
    availableParallelism: () => nodeOs.availableParallelism(),
    getEnv: () => ({ ...envVars }),
    dnsLookup: (host, family) => {
      if (host === "localhost") {
        if (family === 6) return [{ address: "::1", family: 6 }];
        return [{ address: "127.0.0.1", family: 4 }];
      }
      const e = new Error(`getaddrinfo ENOTFOUND ${host}`);
      e.code = "ENOTFOUND"; e.errno = -3008; e.syscall = "getaddrinfo"; e.hostname = host;
      throw e;
    },
  };
}

export function createEnv(layerFiles = [], options = {}) {
  const envVars = options.env ?? { NODE_ENV: "production" };
  const vfs = makeMemoryVfs();
  const host = makeHost(vfs, envVars);

  const sandbox = {
    console, TextEncoder, TextDecoder, URL, URLSearchParams,
    Headers, Request, Response, fetch: options.fetch ?? fetch,
    Blob, FormData, DOMException, structuredClone, performance,
    setTimeout, setInterval, clearTimeout, clearInterval,
    queueMicrotask,
    ReadableStream, WritableStream, TransformStream,
    AbortController, AbortSignal, Event, EventTarget,
    WebAssembly,
    __nano_node_host: host,
    Nano: {
      fs: {
        readFile: async (p) => vfs.read(p),
        writeFile: async (p, d) => vfs.write(p, typeof d === "string" ? new TextEncoder().encode(d) : d),
        exists: async (p) => vfs.exists(p),
        deleteFile: async (p) => vfs.unlink(p),
        listDir: async (p) => vfs.readdir(p),
      },
    },
    _nano_fs: {
      readFileSync: (p) => vfs.read(p),
      writeFileSync: (p, d) => vfs.write(p, typeof d === "string" ? new TextEncoder().encode(d) : d),
      existsSync: (p) => vfs.exists(p),
      unlinkSync: (p) => vfs.unlink(p),
    },
  };
  sandbox.globalThis = sandbox;
  const context = vm.createContext(sandbox, {
    codeGeneration: { strings: false, wasm: true },
  });

  const runFile = (file) => {
    const full = path.isAbsolute(file) ? file : path.join(jsDir, file);
    const code = readFileSync(full, "utf8");
    vm.runInContext(code, context, { filename: path.basename(full) });
  };

  runFile("00_prelude.js");
  runFile("01_errors.js");
  for (const f of layerFiles) {
    if (f === "00_prelude.js" || f === "01_errors.js") continue;
    runFile(f);
  }

  // Back any unregistered builtin with the REAL Node implementation so
  // modules are testable in isolation against authentic peers.
  const isRegistered = context.__nanoNodeIsRegistered;
  const register = context.__nanoNodeRegister;
  for (const id of FALLBACK_IDS) {
    if (!isRegistered(id)) {
      const real = realRequire("node:" + (id === "sys" ? "util" : id));
      register(id, (module) => { module.exports = real; });
    }
  }

  return { require: context.__nanoNodeRequire, context, vfs, host };
}

export function allLayerFiles() {
  if (!existsSync(jsDir)) return [];
  return readdirSync(jsDir).filter((f) => f.endsWith(".js")).sort();
}
