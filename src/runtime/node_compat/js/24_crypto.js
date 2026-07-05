"use strict";
// node:crypto — over __nano_node_host (CONTRACT.md §4) plus Web Crypto re-export.
__nanoNodeRegister("crypto", function (module, exports, require) {
  const { makeError, codes, unsupported } = require("internal/errors");
  const { Transform } = require("stream");
  const host = globalThis.__nano_node_host;

  const HASHES = ["md5", "sha1", "sha224", "sha256", "sha384", "sha512"];
  const HASH_SIZES = { md5: 16, sha1: 20, sha224: 28, sha256: 32, sha384: 48, sha512: 64 };
  const CIPHERS = [
    "aes-128-gcm", "aes-192-gcm", "aes-256-gcm",
    "aes-128-cbc", "aes-192-cbc", "aes-256-cbc",
    "aes-128-ctr", "aes-192-ctr", "aes-256-ctr",
  ];
  const CURVES = { "prime256v1": "p256", "P-256": "p256", "secp384r1": "p384", "P-384": "p384" };

  function normalizeHash(alg) {
    if (typeof alg !== "string") {
      throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", 'The "algorithm" argument must be of type string');
    }
    const n = alg.toLowerCase().replace("-", "").replace("sha", "sha");
    const canon = alg.toLowerCase().replace(/-/g, "");
    if (HASHES.includes(canon)) return canon;
    throw makeError(TypeError, "ERR_CRYPTO_INVALID_DIGEST", "Invalid digest: " + alg);
  }

  function toBytes(data, encoding) {
    const { Buffer } = require("buffer");
    if (typeof data === "string") {
      return new Uint8Array(Buffer.from(data, encoding || "utf8"));
    }
    if (ArrayBuffer.isView(data)) {
      return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
    }
    if (data instanceof ArrayBuffer) {
      return new Uint8Array(data);
    }
    throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", 'The "data" argument must be of type string or an instance of Buffer, TypedArray, DataView, or ArrayBuffer');
  }

  function encodeOut(bytes, encoding) {
    const { Buffer } = require("buffer");
    const buf = Buffer.from(bytes);
    return encoding ? buf.toString(encoding) : buf;
  }

  // ---------------------------------------------------------------------
  // Hash / Hmac (also usable as Transform streams, matching Node)
  // ---------------------------------------------------------------------
  class Hash extends Transform {
    constructor(algorithm, options) {
      super(options);
      this._algorithm = normalizeHash(algorithm);
      this._chunks = [];
      this._finalized = false;
    }
    update(data, inputEncoding) {
      if (this._finalized) {
        throw makeError(Error, "ERR_CRYPTO_HASH_FINALIZED", "Digest already called");
      }
      this._chunks.push(toBytes(data, inputEncoding));
      return this;
    }
    digest(encoding) {
      if (this._finalized) {
        throw makeError(Error, "ERR_CRYPTO_HASH_FINALIZED", "Digest already called");
      }
      this._finalized = true;
      const total = this._chunks.reduce((n, c) => n + c.length, 0);
      const combined = new Uint8Array(total);
      let off = 0;
      for (const c of this._chunks) {
        combined.set(c, off);
        off += c.length;
      }
      const out = host.cryptoDigest(this._algorithm, combined);
      return encodeOut(out, encoding);
    }
    copy() {
      const h = new Hash(this._algorithm);
      h._chunks = this._chunks.map((c) => c.slice());
      return h;
    }
    _transform(chunk, encoding, callback) {
      this.update(chunk);
      callback();
    }
    _flush(callback) {
      this.push(this.digest());
      callback();
    }
  }

  class Hmac extends Transform {
    constructor(algorithm, key, options) {
      super(options);
      this._algorithm = normalizeHash(algorithm);
      this._key = toBytes(key);
      this._chunks = [];
      this._finalized = false;
    }
    update(data, inputEncoding) {
      if (this._finalized) {
        throw makeError(Error, "ERR_CRYPTO_HASH_FINALIZED", "Digest already called");
      }
      this._chunks.push(toBytes(data, inputEncoding));
      return this;
    }
    digest(encoding) {
      if (this._finalized) {
        throw makeError(Error, "ERR_CRYPTO_HASH_FINALIZED", "Digest already called");
      }
      this._finalized = true;
      const total = this._chunks.reduce((n, c) => n + c.length, 0);
      const combined = new Uint8Array(total);
      let off = 0;
      for (const c of this._chunks) {
        combined.set(c, off);
        off += c.length;
      }
      const out = host.cryptoHmac(this._algorithm, this._key, combined);
      return encodeOut(out, encoding);
    }
    _transform(chunk, encoding, callback) {
      this.update(chunk);
      callback();
    }
    _flush(callback) {
      this.push(this.digest());
      callback();
    }
  }

  function createHash(algorithm, options) {
    return new Hash(algorithm, options);
  }
  function createHmac(algorithm, key, options) {
    return new Hmac(algorithm, key, options);
  }

  // ---------------------------------------------------------------------
  // Random
  // ---------------------------------------------------------------------
  function randomBytes(size, callback) {
    if (typeof size !== "number" || size < 0 || !Number.isInteger(size)) {
      throw makeError(RangeError, "ERR_OUT_OF_RANGE", 'The value of "size" is out of range. Received ' + size);
    }
    const { Buffer } = require("buffer");
    if (typeof callback === "function") {
      queueMicrotask(() => {
        try {
          callback(null, Buffer.from(host.cryptoRandomBytes(size)));
        } catch (e) {
          callback(e);
        }
      });
      return undefined;
    }
    return Buffer.from(host.cryptoRandomBytes(size));
  }

  function randomFillSync(buffer, offset = 0, size) {
    const view = ArrayBuffer.isView(buffer) ? buffer : new Uint8Array(buffer);
    const len = size === undefined ? view.byteLength - offset : size;
    const bytes = host.cryptoRandomBytes(len);
    const target = new Uint8Array(view.buffer, view.byteOffset + offset, len);
    target.set(bytes);
    return buffer;
  }

  function randomFill(buffer, offset, size, callback) {
    if (typeof offset === "function") {
      callback = offset; offset = 0; size = undefined;
    } else if (typeof size === "function") {
      callback = size; size = undefined;
    }
    queueMicrotask(() => {
      try {
        randomFillSync(buffer, offset, size);
        callback(null, buffer);
      } catch (e) {
        callback(e);
      }
    });
  }

  function randomInt(min, max, callback) {
    if (typeof max === "function") {
      callback = max;
      max = min;
      min = 0;
    }
    if (max === undefined) {
      max = min;
      min = 0;
    }
    if (!Number.isInteger(min) || !Number.isInteger(max) || max <= min) {
      const err = makeError(RangeError, "ERR_OUT_OF_RANGE", 'The value of "max" is out of range. It must be greater than the value of "min"');
      if (callback) { queueMicrotask(() => callback(err)); return undefined; }
      throw err;
    }
    const range = max - min;
    const bits = Math.ceil(Math.log2(range));
    const bytes = Math.ceil(bits / 8);
    const mask = bits < 32 ? (1 << bits) - 1 : 0xffffffff;

    function draw() {
      let value;
      do {
        const buf = host.cryptoRandomBytes(bytes);
        value = 0;
        for (let i = 0; i < bytes; i += 1) {
          value = (value << 8) | buf[i];
        }
        value = (value >>> 0) & mask;
      } while (value >= range);
      return value + min;
    }

    if (callback) {
      queueMicrotask(() => {
        try {
          callback(null, draw());
        } catch (e) {
          callback(e);
        }
      });
      return undefined;
    }
    return draw();
  }

  function randomUUID() {
    const bytes = host.cryptoRandomBytes(16);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
    return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
  }

  function timingSafeEqual(a, b) {
    const av = ArrayBuffer.isView(a) ? new Uint8Array(a.buffer, a.byteOffset, a.byteLength) : null;
    const bv = ArrayBuffer.isView(b) ? new Uint8Array(b.buffer, b.byteOffset, b.byteLength) : null;
    if (!av || !bv) {
      throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", "The arguments must be Buffer, TypedArray, or DataView instances");
    }
    if (av.length !== bv.length) {
      throw makeError(RangeError, "ERR_CRYPTO_TIMING_SAFE_EQUAL_LENGTH", "Input buffers must have the same byte length");
    }
    return host.cryptoTimingSafeEqual(av, bv);
  }

  // ---------------------------------------------------------------------
  // KDFs
  // ---------------------------------------------------------------------
  function pbkdf2Sync(password, salt, iterations, keylen, digest) {
    const { Buffer } = require("buffer");
    return Buffer.from(host.cryptoPbkdf2(toBytes(password), toBytes(salt), iterations, keylen, normalizeHash(digest)));
  }
  function pbkdf2(password, salt, iterations, keylen, digest, callback) {
    queueMicrotask(() => {
      try {
        callback(null, pbkdf2Sync(password, salt, iterations, keylen, digest));
      } catch (e) {
        callback(e);
      }
    });
  }

  function scryptSync(password, salt, keylen, options) {
    const { Buffer } = require("buffer");
    const opts = options || {};
    const N = opts.N || opts.cost || 16384;
    const r = opts.r || opts.blockSize || 8;
    const p = opts.p || opts.parallelization || 1;
    const maxmem = opts.maxmem || 32 * 1024 * 1024;
    if (128 * N * r > maxmem) {
      throw makeError(RangeError, "ERR_CRYPTO_INVALID_SCRYPT_PARAMS", "Invalid scrypt params: memory limit exceeded");
    }
    return Buffer.from(host.cryptoScrypt(toBytes(password), toBytes(salt), N, r, p, keylen));
  }
  function scrypt(password, salt, keylen, options, callback) {
    if (typeof options === "function") {
      callback = options;
      options = {};
    }
    queueMicrotask(() => {
      try {
        callback(null, scryptSync(password, salt, keylen, options));
      } catch (e) {
        callback(e);
      }
    });
  }

  function hkdfSync(digest, ikm, salt, info, keylen) {
    return host.cryptoHkdf(normalizeHash(digest), toBytes(ikm), toBytes(salt), toBytes(info), keylen).buffer;
  }
  function hkdf(digest, ikm, salt, info, keylen, callback) {
    queueMicrotask(() => {
      try {
        callback(null, hkdfSync(digest, ikm, salt, info, keylen));
      } catch (e) {
        callback(e);
      }
    });
  }

  // ---------------------------------------------------------------------
  // KeyObject
  // ---------------------------------------------------------------------
  const kKeyData = Symbol("keyData");

  class KeyObject {
    constructor(type, data) {
      this.type = type; // 'secret' | 'public' | 'private'
      this[kKeyData] = data;
    }
    static from(cryptoKey) {
      throw unsupported("KeyObject.from(CryptoKey)");
    }
    get symmetricKeySize() {
      return this.type === "secret" ? this[kKeyData].raw.length : undefined;
    }
    get asymmetricKeyType() {
      return this.type === "secret" ? undefined : this[kKeyData].kind;
    }
    get asymmetricKeyDetails() {
      if (this.type === "secret") return undefined;
      const d = this[kKeyData];
      if (d.kind === "rsa") return { modulusLength: d.modulusLength || 2048, publicExponent: 65537n };
      if (d.kind === "ec") return { namedCurve: d.curve };
      return {};
    }
    export(options = {}) {
      const d = this[kKeyData];
      if (this.type === "secret") {
        const { Buffer } = require("buffer");
        const buf = Buffer.from(d.raw);
        if (options.format === "jwk") {
          return { kty: "oct", k: buf.toString("base64url") };
        }
        return buf;
      }
      const pem = this.type === "private" ? d.privatePem : d.publicPem;
      if (options.format === "der") {
        const b64 = pem.replace(/-----[^-]+-----/g, "").replace(/\s+/g, "");
        const { Buffer } = require("buffer");
        return Buffer.from(b64, "base64");
      }
      return pem;
    }
    equals(other) {
      if (!(other instanceof KeyObject) || other.type !== this.type) return false;
      if (this.type === "secret") {
        try {
          return timingSafeEqual(new Uint8Array(this[kKeyData].raw), new Uint8Array(other[kKeyData].raw));
        } catch (_e) {
          return false;
        }
      }
      const a = this.type === "private" ? this[kKeyData].privatePem : this[kKeyData].publicPem;
      const b = this.type === "private" ? other[kKeyData].privatePem : other[kKeyData].publicPem;
      return a === b;
    }
    get [Symbol.toStringTag]() {
      return "KeyObject";
    }
  }

  function createSecretKey(key, encoding) {
    return new KeyObject("secret", { raw: toBytes(key, encoding) });
  }

  function pemFrom(key, formatHint) {
    if (typeof key === "string") return key;
    if (key instanceof KeyObject) return key.export();
    if (key && typeof key.key === "string") return key.key;
    if (key && ArrayBuffer.isView(key.key)) {
      const { Buffer } = require("buffer");
      const b64 = Buffer.from(key.key).toString("base64");
      const label = formatHint === "public" ? "PUBLIC KEY" : "PRIVATE KEY";
      return `-----BEGIN ${label}-----\n${b64.match(/.{1,64}/g).join("\n")}\n-----END ${label}-----`;
    }
    if (ArrayBuffer.isView(key)) {
      const { Buffer } = require("buffer");
      const b64 = Buffer.from(key).toString("base64");
      const label = formatHint === "public" ? "PUBLIC KEY" : "PRIVATE KEY";
      return `-----BEGIN ${label}-----\n${b64.match(/.{1,64}/g).join("\n")}\n-----END ${label}-----`;
    }
    throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", "Invalid key argument");
  }

  // Minimal definite-length DER TLV reader (no dynamic codegen: pure byte
  // arithmetic). Sufficient for the PKCS8/SPKI AlgorithmIdentifier prefix
  // every key we handle starts with.
  function readTlv(bytes, offset) {
    const tag = bytes[offset];
    let lenByte = bytes[offset + 1];
    let lenOffset = offset + 2;
    let length;
    if (lenByte < 0x80) {
      length = lenByte;
    } else {
      const numBytes = lenByte & 0x7f;
      length = 0;
      for (let i = 0; i < numBytes; i += 1) {
        length = (length << 8) | bytes[lenOffset + i];
      }
      lenOffset += numBytes;
    }
    return { tag, valueStart: lenOffset, valueEnd: lenOffset + length, next: lenOffset + length };
  }

  function bytesToHex(bytes) {
    let out = "";
    for (let i = 0; i < bytes.length; i += 1) {
      out += bytes[i].toString(16).padStart(2, "0");
    }
    return out;
  }

  const OID_RSA = "2a864886f70d010101";
  const OID_EC_PUBLIC_KEY = "2a8648ce3d0201";
  const OID_ED25519 = "2b6570";
  const CURVE_OIDS = { "2a8648ce3d030107": "p256", "2b81040022": "p384" };

  // Extract the AlgorithmIdentifier OID (and curve OID, for EC keys) from a
  // PKCS8 PrivateKeyInfo or SPKI SubjectPublicKeyInfo DER structure. Works
  // for both: the algorithm-identifier SEQUENCE is the first (SPKI) or
  // second (PKCS8, after the INTEGER version) child of the outer SEQUENCE.
  function parseKeyAlgorithm(der) {
    const outer = readTlv(der, 0);
    let pos = outer.valueStart;
    while (pos < outer.valueEnd) {
      const child = readTlv(der, pos);
      if (child.tag === 0x30) {
        const oidTlv = readTlv(der, child.valueStart);
        const oid = bytesToHex(der.subarray(oidTlv.valueStart, oidTlv.valueEnd));
        let curveOid = null;
        if (oidTlv.next < child.valueEnd) {
          const paramTlv = readTlv(der, oidTlv.next);
          if (paramTlv.tag === 0x06) {
            curveOid = bytesToHex(der.subarray(paramTlv.valueStart, paramTlv.valueEnd));
          }
        }
        return { oid, curveOid };
      }
      pos = child.next;
    }
    throw makeError(TypeError, "ERR_INVALID_ARG_VALUE", "Unable to parse key algorithm identifier");
  }

  function detectKind(pem) {
    // SEC1 EC private keys carry the curve as a separate label; PKCS8/SPKI
    // (the format this module always generates) require parsing the OID.
    if (pem.includes("BEGIN EC PRIVATE KEY")) {
      return { kind: "ec", curve: undefined };
    }
    const der = pemToDer(pem);
    const { oid, curveOid } = parseKeyAlgorithm(der);
    if (oid === OID_RSA) return { kind: "rsa" };
    if (oid === OID_ED25519) return { kind: "ed25519" };
    if (oid === OID_EC_PUBLIC_KEY) {
      const curve = curveOid ? CURVE_OIDS[curveOid] : undefined;
      if (!curve) {
        throw makeError(TypeError, "ERR_INVALID_ARG_VALUE", "Unsupported or unrecognized EC curve");
      }
      return { kind: "ec", curve };
    }
    throw makeError(TypeError, "ERR_INVALID_ARG_VALUE", "Unrecognized key algorithm OID: " + oid);
  }

  function createPublicKey(key) {
    if (key instanceof KeyObject) {
      if (key.type === "private") {
        // Derive public from private: not all backends support this directly;
        // callers should pass the original public PEM when possible.
        return new KeyObject("public", { ...key[kKeyData] });
      }
      return key;
    }
    const pem = pemFrom(key, "public");
    const { kind, curve } = detectKind(pem);
    return new KeyObject("public", { publicPem: pem, kind, curve });
  }

  function createPrivateKey(key) {
    if (key instanceof KeyObject) return key;
    const pem = pemFrom(key, "private");
    const { kind, curve } = detectKind(pem);
    return new KeyObject("private", { privatePem: pem, kind, curve });
  }

  function generateKeyPairSync(type, options = {}) {
    let privatePem;
    let publicPem;
    let kind = type;
    let curve;
    let modulusLength;
    if (type === "rsa" || type === "rsa-pss") {
      modulusLength = options.modulusLength || 2048;
      const result = host.cryptoRsaGenerate(modulusLength);
      privatePem = result.privatePem;
      publicPem = result.publicPem;
      kind = "rsa";
    } else if (type === "ec") {
      const named = options.namedCurve;
      curve = CURVES[named] || (named === "p256" || named === "p384" ? named : undefined);
      if (!curve) {
        throw makeError(TypeError, "ERR_INVALID_ARG_VALUE", "Unsupported namedCurve: " + named);
      }
      const result = host.cryptoEcGenerate(curve);
      privatePem = result.privatePem;
      publicPem = result.publicPem;
      kind = "ec";
    } else if (type === "ed25519") {
      const result = host.cryptoEd25519Generate();
      const { Buffer } = require("buffer");
      const b64 = Buffer.from(result.privatePkcs8).toString("base64");
      privatePem = `-----BEGIN PRIVATE KEY-----\n${b64.match(/.{1,64}/g).join("\n")}\n-----END PRIVATE KEY-----`;
      const spkiPrefix = Buffer.from("302a300506032b6570032100", "hex");
      const pubB64 = Buffer.concat([spkiPrefix, Buffer.from(result.publicRaw)]).toString("base64");
      publicPem = `-----BEGIN PUBLIC KEY-----\n${pubB64.match(/.{1,64}/g).join("\n")}\n-----END PUBLIC KEY-----`;
      kind = "ed25519";
    } else {
      throw makeError(TypeError, "ERR_INVALID_ARG_VALUE", "Unsupported key type: " + type);
    }

    const privateKeyObj = new KeyObject("private", { privatePem, publicPem, kind, curve, modulusLength });
    const publicKeyObj = new KeyObject("public", { publicPem, kind, curve, modulusLength });

    const wantsPrivateEncoding = options.privateKeyEncoding;
    const wantsPublicEncoding = options.publicKeyEncoding;
    const privateOut = wantsPrivateEncoding ? privateKeyObj.export(wantsPrivateEncoding) : privateKeyObj;
    const publicOut = wantsPublicEncoding ? publicKeyObj.export(wantsPublicEncoding) : publicKeyObj;
    return { publicKey: publicOut, privateKey: privateOut };
  }

  function generateKeyPair(type, options, callback) {
    if (typeof options === "function") {
      callback = options;
      options = {};
    }
    queueMicrotask(() => {
      try {
        const { publicKey, privateKey } = generateKeyPairSync(type, options);
        callback(null, publicKey, privateKey);
      } catch (e) {
        callback(e);
      }
    });
  }

  function generateKeySync(type, options = {}) {
    if (type === "hmac" || type === "aes") {
      const length = options.length || (type === "aes" ? 256 : 256);
      return createSecretKey(host.cryptoRandomBytes(length / 8));
    }
    throw makeError(TypeError, "ERR_INVALID_ARG_VALUE", "Unsupported key type: " + type);
  }
  function generateKey(type, options, callback) {
    if (typeof options === "function") {
      callback = options;
      options = {};
    }
    queueMicrotask(() => {
      try {
        callback(null, generateKeySync(type, options));
      } catch (e) {
        callback(e);
      }
    });
  }

  // ---------------------------------------------------------------------
  // Sign / Verify
  // ---------------------------------------------------------------------
  const SIGN_ALG_RE = /^(?:RSA-|ecdsa-with-|DSA-)?(SHA(?:1|224|256|384|512))$/i;
  function parseSignAlgorithm(alg) {
    if (alg === null) return null; // ed25519 one-shot uses null
    const m = SIGN_ALG_RE.exec(alg) || /^(sha1|sha224|sha256|sha384|sha512)$/i.exec(alg);
    if (!m) {
      throw makeError(TypeError, "ERR_CRYPTO_INVALID_DIGEST", "Invalid digest: " + alg);
    }
    return m[1].toLowerCase();
  }

  function ieee1363ToDer(sig, curve) {
    const n = curve === "p384" ? 48 : 32;
    const r = sig.subarray(0, n);
    const s = sig.subarray(n, n * 2);
    function encodeInt(bytes) {
      let i = 0;
      while (i < bytes.length - 1 && bytes[i] === 0) i += 1;
      let trimmed = bytes.subarray(i);
      if (trimmed[0] & 0x80) {
        const padded = new Uint8Array(trimmed.length + 1);
        padded.set(trimmed, 1);
        trimmed = padded;
      }
      return trimmed;
    }
    const rEnc = encodeInt(r);
    const sEnc = encodeInt(s);
    const body = new Uint8Array(2 + rEnc.length + 2 + sEnc.length);
    let o = 0;
    body[o++] = 0x02; body[o++] = rEnc.length; body.set(rEnc, o); o += rEnc.length;
    body[o++] = 0x02; body[o++] = sEnc.length; body.set(sEnc, o); o += sEnc.length;
    const out = new Uint8Array(2 + body.length);
    out[0] = 0x30; out[1] = body.length;
    out.set(body, 2);
    return out;
  }

  function derToIeee1363(der, curve) {
    const n = curve === "p384" ? 48 : 32;
    let idx = 2; // skip SEQUENCE tag + len (assume short form; DER sigs here are small)
    if (der[1] & 0x80) idx = 2 + (der[1] & 0x7f);
    function readInt() {
      idx += 1; // skip INTEGER tag
      const len = der[idx]; idx += 1;
      const bytes = der.subarray(idx, idx + len);
      idx += len;
      const trimmed = bytes[0] === 0 && bytes.length > n ? bytes.subarray(1) : bytes;
      const out = new Uint8Array(n);
      out.set(trimmed, n - trimmed.length);
      return out;
    }
    const r = readInt();
    const s = readInt();
    const out = new Uint8Array(n * 2);
    out.set(r, 0);
    out.set(s, n);
    return out;
  }

  class Sign extends Transform {
    constructor(algorithm, options) {
      super(options);
      this._digest = parseSignAlgorithm(algorithm.replace(/^RSA-/, ""));
      this._chunks = [];
    }
    update(data, inputEncoding) {
      this._chunks.push(toBytes(data, inputEncoding));
      return this;
    }
    sign(privateKey, outputEncoding) {
      const total = this._chunks.reduce((n, c) => n + c.length, 0);
      const combined = new Uint8Array(total);
      let off = 0;
      for (const c of this._chunks) { combined.set(c, off); off += c.length; }

      const options = typeof privateKey === "object" && !(privateKey instanceof KeyObject) ? privateKey : {};
      const keyObj = privateKey instanceof KeyObject ? privateKey : (options.key !== undefined ? createPrivateKey(options) : createPrivateKey(privateKey));
      const kind = keyObj[kKeyData].kind;

      let sig;
      if (kind === "rsa") {
        const padding = options.padding === 6 /* RSA_PKCS1_PSS_PADDING */ || options.dsaEncoding === "pss"
          ? "pss" : "pkcs1";
        const saltLength = options.saltLength || HASH_SIZES[this._digest];
        sig = host.cryptoRsaSign(padding, this._digest, keyObj[kKeyData].privatePem, combined, saltLength);
      } else if (kind === "ec") {
        const der = host.cryptoEcSign(keyObj[kKeyData].curve, this._digest, keyObj[kKeyData].privatePem, combined);
        sig = options.dsaEncoding === "ieee-p1363" ? derToIeee1363(der, keyObj[kKeyData].curve) : der;
      } else {
        throw unsupported("Sign.sign for key kind '" + kind + "'");
      }
      return encodeOut(sig, outputEncoding);
    }
  }

  class Verify extends Transform {
    constructor(algorithm, options) {
      super(options);
      this._digest = parseSignAlgorithm(algorithm.replace(/^RSA-/, ""));
      this._chunks = [];
    }
    update(data, inputEncoding) {
      this._chunks.push(toBytes(data, inputEncoding));
      return this;
    }
    verify(publicKey, signature, signatureEncoding) {
      const { Buffer } = require("buffer");
      const total = this._chunks.reduce((n, c) => n + c.length, 0);
      const combined = new Uint8Array(total);
      let off = 0;
      for (const c of this._chunks) { combined.set(c, off); off += c.length; }
      const sigBytes = typeof signature === "string" ? new Uint8Array(Buffer.from(signature, signatureEncoding || "binary")) : toBytes(signature);

      const options = typeof publicKey === "object" && !(publicKey instanceof KeyObject) ? publicKey : {};
      const keyObj = publicKey instanceof KeyObject ? publicKey : (options.key !== undefined ? createPublicKey(options) : createPublicKey(publicKey));
      const kind = keyObj[kKeyData].kind;

      if (kind === "rsa") {
        const padding = options.padding === 6 || options.dsaEncoding === "pss" ? "pss" : "pkcs1";
        const saltLength = options.saltLength || HASH_SIZES[this._digest];
        return host.cryptoRsaVerify(padding, this._digest, keyObj[kKeyData].publicPem, combined, sigBytes, saltLength);
      }
      if (kind === "ec") {
        const der = options.dsaEncoding === "ieee-p1363" ? ieee1363ToDer(sigBytes, keyObj[kKeyData].curve) : sigBytes;
        return host.cryptoEcVerify(keyObj[kKeyData].curve, this._digest, keyObj[kKeyData].publicPem, combined, der);
      }
      throw unsupported("Verify.verify for key kind '" + kind + "'");
    }
  }

  function createSign(algorithm, options) {
    return new Sign(algorithm, options);
  }
  function createVerify(algorithm, options) {
    return new Verify(algorithm, options);
  }

  function sign(algorithm, data, key) {
    const bytes = toBytes(data);
    const options = typeof key === "object" && !(key instanceof KeyObject) ? key : {};
    const keyObj = key instanceof KeyObject ? key : createPrivateKey(key);
    const kind = keyObj[kKeyData].kind;
    const { Buffer } = require("buffer");
    if (kind === "ed25519") {
      const pem = keyObj[kKeyData].privatePem;
      const der = pemToDer(pem);
      // PKCS8 wraps a 32-byte seed inside an OCTET STRING; extract the last 32 bytes.
      const seed = der.subarray(der.length - 32);
      const pkcs8 = der;
      const sig = host.cryptoEd25519Sign(pkcs8, bytes);
      return Buffer.from(sig);
    }
    if (algorithm === null || algorithm === undefined) {
      throw makeError(TypeError, "ERR_INVALID_ARG_VALUE", "algorithm required for non-Ed25519 keys");
    }
    const digest = parseSignAlgorithm(algorithm.replace(/^RSA-/, ""));
    if (kind === "rsa") {
      const padding = options.padding === 6 ? "pss" : "pkcs1";
      const saltLength = options.saltLength || HASH_SIZES[digest];
      return Buffer.from(host.cryptoRsaSign(padding, digest, keyObj[kKeyData].privatePem, bytes, saltLength));
    }
    if (kind === "ec") {
      const der = host.cryptoEcSign(keyObj[kKeyData].curve, digest, keyObj[kKeyData].privatePem, bytes);
      const out = options.dsaEncoding === "ieee-p1363" ? derToIeee1363(der, keyObj[kKeyData].curve) : der;
      return Buffer.from(out);
    }
    throw unsupported("sign() for key kind '" + kind + "'");
  }

  function verify(algorithm, data, key, signature) {
    const bytes = toBytes(data);
    const options = typeof key === "object" && !(key instanceof KeyObject) ? key : {};
    const keyObj = key instanceof KeyObject ? key : createPublicKey(key);
    const kind = keyObj[kKeyData].kind;
    const sigBytes = toBytes(signature);
    if (kind === "ed25519") {
      const pem = keyObj[kKeyData].publicPem;
      const der = pemToDer(pem);
      const publicRaw = der.subarray(der.length - 32);
      return host.cryptoEd25519Verify(publicRaw, bytes, sigBytes);
    }
    const digest = parseSignAlgorithm(algorithm.replace(/^RSA-/, ""));
    if (kind === "rsa") {
      const padding = options.padding === 6 ? "pss" : "pkcs1";
      const saltLength = options.saltLength || HASH_SIZES[digest];
      return host.cryptoRsaVerify(padding, digest, keyObj[kKeyData].publicPem, bytes, sigBytes, saltLength);
    }
    if (kind === "ec") {
      const der = options.dsaEncoding === "ieee-p1363" ? ieee1363ToDer(sigBytes, keyObj[kKeyData].curve) : sigBytes;
      return host.cryptoEcVerify(keyObj[kKeyData].curve, digest, keyObj[kKeyData].publicPem, bytes, der);
    }
    throw unsupported("verify() for key kind '" + kind + "'");
  }

  function pemToDer(pem) {
    const { Buffer } = require("buffer");
    const b64 = pem.replace(/-----[^-]+-----/g, "").replace(/\s+/g, "");
    return new Uint8Array(Buffer.from(b64, "base64"));
  }

  // ---------------------------------------------------------------------
  // Cipher / Decipher
  // ---------------------------------------------------------------------
  class Cipheriv extends Transform {
    constructor(algorithm, key, iv, options) {
      super(options);
      this._algo = algorithm.toLowerCase();
      if (!CIPHERS.includes(this._algo)) {
        throw makeError(TypeError, "ERR_CRYPTO_UNKNOWN_CIPHER", "Unknown cipher: " + algorithm);
      }
      this._key = toBytes(key);
      this._iv = toBytes(iv);
      this._chunks = [];
      this._aad = null;
      this._authTagLength = (options && options.authTagLength) || 16;
      this._finalized = false;
    }
    setAAD(aad, options) {
      this._aad = toBytes(aad);
      if (options && options.plaintextLength !== undefined) {
        this._plaintextLength = options.plaintextLength;
      }
      return this;
    }
    setAutoPadding() {
      return this;
    }
    update(data, inputEncoding, outputEncoding) {
      this._chunks.push(toBytes(data, inputEncoding));
      return outputEncoding ? encodeOut(new Uint8Array(0), outputEncoding) : Buffer_empty();
    }
    final(outputEncoding) {
      if (this._finalized) {
        throw makeError(Error, "ERR_CRYPTO_INVALID_STATE", "final() already called");
      }
      this._finalized = true;
      const total = this._chunks.reduce((n, c) => n + c.length, 0);
      const combined = new Uint8Array(total);
      let off = 0;
      for (const c of this._chunks) { combined.set(c, off); off += c.length; }
      const result = host.cryptoCipher("encrypt", this._algo, this._key, this._iv, combined, this._aad, null);
      this._tag = result.tag ? new Uint8Array(result.tag) : undefined;
      return encodeOut(result.data, outputEncoding);
    }
    getAuthTag() {
      const { Buffer } = require("buffer");
      if (!this._tag) {
        throw makeError(Error, "ERR_CRYPTO_INVALID_STATE", "getAuthTag() can only be called after encryption is finalized");
      }
      return Buffer.from(this._tag.slice(0, this._authTagLength));
    }
    _transform(chunk, encoding, callback) {
      this._chunks.push(toBytes(chunk));
      callback();
    }
    _flush(callback) {
      try {
        callback(null, this.final());
      } catch (e) {
        callback(e);
      }
    }
  }

  class Decipheriv extends Transform {
    constructor(algorithm, key, iv, options) {
      super(options);
      this._algo = algorithm.toLowerCase();
      if (!CIPHERS.includes(this._algo)) {
        throw makeError(TypeError, "ERR_CRYPTO_UNKNOWN_CIPHER", "Unknown cipher: " + algorithm);
      }
      this._key = toBytes(key);
      this._iv = toBytes(iv);
      this._chunks = [];
      this._aad = null;
      this._tag = null;
      this._finalized = false;
    }
    setAAD(aad, options) {
      this._aad = toBytes(aad);
      return this;
    }
    setAuthTag(tag) {
      this._tag = toBytes(tag);
      return this;
    }
    setAutoPadding() {
      return this;
    }
    update(data, inputEncoding, outputEncoding) {
      this._chunks.push(toBytes(data, inputEncoding));
      return outputEncoding ? "" : Buffer_empty();
    }
    final(outputEncoding) {
      if (this._finalized) {
        throw makeError(Error, "ERR_CRYPTO_INVALID_STATE", "final() already called");
      }
      this._finalized = true;
      const total = this._chunks.reduce((n, c) => n + c.length, 0);
      const combined = new Uint8Array(total);
      let off = 0;
      for (const c of this._chunks) { combined.set(c, off); off += c.length; }
      const result = host.cryptoCipher("decrypt", this._algo, this._key, this._iv, combined, this._aad, this._tag);
      return encodeOut(result.data, outputEncoding);
    }
    _transform(chunk, encoding, callback) {
      this._chunks.push(toBytes(chunk));
      callback();
    }
    _flush(callback) {
      try {
        callback(null, this.final());
      } catch (e) {
        callback(e);
      }
    }
  }

  function Buffer_empty() {
    const { Buffer } = require("buffer");
    return Buffer.alloc(0);
  }

  function createCipheriv(algorithm, key, iv, options) {
    return new Cipheriv(algorithm, key, iv, options);
  }
  function createDecipheriv(algorithm, key, iv, options) {
    return new Decipheriv(algorithm, key, iv, options);
  }

  // ---------------------------------------------------------------------
  // RSA encrypt/decrypt (public/private key transport)
  // ---------------------------------------------------------------------
  function resolveRsaOpts(keyLike) {
    const options = typeof keyLike === "object" && !(keyLike instanceof KeyObject) && !ArrayBuffer.isView(keyLike) ? keyLike : {};
    return {
      padding: options.padding === 1 /* RSA_PKCS1_PADDING */ ? "pkcs1" : "oaep",
      hash: options.oaepHash || "sha1",
    };
  }

  function publicEncrypt(key, data) {
    const { Buffer } = require("buffer");
    const keyObj = key instanceof KeyObject ? key : createPublicKey(key);
    const { padding, hash } = resolveRsaOpts(key);
    return Buffer.from(host.cryptoRsaEncrypt(padding, hash, keyObj[kKeyData].publicPem, toBytes(data)));
  }
  function privateDecrypt(key, data) {
    const { Buffer } = require("buffer");
    const keyObj = key instanceof KeyObject ? key : createPrivateKey(key);
    const { padding, hash } = resolveRsaOpts(key);
    return Buffer.from(host.cryptoRsaDecrypt(padding, hash, keyObj[kKeyData].privatePem, toBytes(data)));
  }
  function privateEncrypt(key, data) {
    const { Buffer } = require("buffer");
    const keyObj = key instanceof KeyObject ? key : createPrivateKey(key);
    // Node's privateEncrypt uses PKCS1 signing-style padding; approximate via sign+pack is not
    // equivalent, so route through the pkcs1 RSA primitive the host exposes for encryption.
    return Buffer.from(host.cryptoRsaEncrypt("pkcs1", "sha1", keyObj[kKeyData].publicPem || keyObj[kKeyData].privatePem, toBytes(data)));
  }
  function publicDecrypt(key, data) {
    throw unsupported("crypto.publicDecrypt (RSA raw public-key decrypt)");
  }

  // ---------------------------------------------------------------------
  // getHashes / getCiphers / getCurves
  // ---------------------------------------------------------------------
  function getHashes() {
    return [...HASHES, "rsa-sha1", "rsa-sha256", "rsa-sha384", "rsa-sha512"];
  }
  function getCiphers() {
    return [...CIPHERS];
  }
  function getCurves() {
    return ["prime256v1", "secp384r1"];
  }
  function getCipherInfo(nameOrNid) {
    const name = String(nameOrNid).toLowerCase();
    if (!CIPHERS.includes(name)) return undefined;
    const bits = parseInt(name.split("-")[1], 10);
    const mode = name.split("-")[2];
    return {
      name,
      nid: 0,
      blockSize: mode === "gcm" || mode === "ctr" ? 1 : 16,
      keyLength: bits / 8,
      ivLength: mode === "gcm" ? 12 : 16,
      mode,
    };
  }

  const constants = Object.freeze({
    OPENSSL_VERSION_NUMBER: 0x30000000,
    RSA_PKCS1_PADDING: 1, RSA_NO_PADDING: 3, RSA_PKCS1_OAEP_PADDING: 4,
    RSA_PKCS1_PSS_PADDING: 6, RSA_SSLV23_PADDING: 2, RSA_X931_PADDING: 5,
    RSA_PSS_SALTLEN_DIGEST: -1, RSA_PSS_SALTLEN_AUTO: -2, RSA_PSS_SALTLEN_MAX_SIGN: -3,
    POINT_CONVERSION_COMPRESSED: 2, POINT_CONVERSION_UNCOMPRESSED: 4, POINT_CONVERSION_HYBRID: 6,
    defaultCipherList: "",
    ENGINE_METHOD_ALL: 0xffff,
    SSL_OP_ALL: 0,
  });

  module.exports = {
    Hash, Hmac, createHash, createHmac,
    Sign, Verify, createSign, createVerify, sign, verify,
    Cipher: Cipheriv, Decipher: Decipheriv, Cipheriv, Decipheriv, createCipheriv, createDecipheriv,
    randomBytes, randomFillSync, randomFill, randomInt, randomUUID,
    timingSafeEqual,
    pbkdf2, pbkdf2Sync, scrypt, scryptSync, hkdf, hkdfSync,
    generateKey, generateKeySync, generateKeyPair, generateKeyPairSync,
    createSecretKey, createPublicKey, createPrivateKey, KeyObject,
    publicEncrypt, privateDecrypt, privateEncrypt, publicDecrypt,
    getHashes, getCiphers, getCurves, getCipherInfo,
    constants,
    webcrypto: globalThis.crypto,
    getRandomValues: globalThis.crypto ? globalThis.crypto.getRandomValues.bind(globalThis.crypto) : undefined,
    subtle: globalThis.crypto ? globalThis.crypto.subtle : undefined,
    DiffieHellman: class DiffieHellman {
      constructor() { throw unsupported("crypto.DiffieHellman"); }
    },
    DiffieHellmanGroup: class DiffieHellmanGroup {
      constructor() { throw unsupported("crypto.DiffieHellmanGroup"); }
    },
    createDiffieHellman() { throw unsupported("crypto.createDiffieHellman"); },
    createDiffieHellmanGroup() { throw unsupported("crypto.createDiffieHellmanGroup"); },
    ECDH: class ECDH {
      constructor() { throw unsupported("crypto.ECDH"); }
    },
    createECDH() { throw unsupported("crypto.createECDH"); },
    X509Certificate: class X509Certificate {
      constructor() { throw unsupported("crypto.X509Certificate"); }
    },
    Certificate: class Certificate {
      static exportChallenge() { throw unsupported("crypto.Certificate"); }
      static exportPublicKey() { throw unsupported("crypto.Certificate"); }
      static verifySpkac() { throw unsupported("crypto.Certificate"); }
    },
    checkPrime() { throw unsupported("crypto.checkPrime"); },
    checkPrimeSync() { throw unsupported("crypto.checkPrimeSync"); },
    generatePrime() { throw unsupported("crypto.generatePrime"); },
    generatePrimeSync() { throw unsupported("crypto.generatePrimeSync"); },
    secureHeapUsed() {
      return { total: 0, used: 0, utilization: 0, min: 0 };
    },
    setEngine() { throw unsupported("crypto.setEngine"); },
    setFips(enable) {
      if (enable) throw unsupported("FIPS mode");
    },
    getFips() {
      return 0;
    },
    fips: false,
  };
});
