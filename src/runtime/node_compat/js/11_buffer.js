"use strict";
// node:buffer — full Buffer implementation over Uint8Array (Node v22 semantics).
__nanoNodeRegister("buffer", function (module, exports, require) {
  const { makeError, codes } = require("internal/errors");

  const kMaxLength = 4294967296; // 2^32 (64-bit platforms)
  const kStringMaxLength = 536870888; // V8 string max
  const INSPECT_MAX_BYTES = 50;

  const utf8Encoder = new TextEncoder();
  const utf8Decoder = new TextDecoder("utf-8");
  const utf16Decoder = new TextDecoder("utf-16le");

  // ---------------------------------------------------------------------
  // Encoding utilities
  // ---------------------------------------------------------------------
  const HEX_CHARS = "0123456789abcdef";
  const B64_STD = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  const B64_URL = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
  const B64_LUT = new Int8Array(256).fill(-1);
  for (let i = 0; i < 64; i += 1) {
    B64_LUT[B64_STD.charCodeAt(i)] = i;
  }
  B64_LUT[45] = 62; // '-' (base64url)
  B64_LUT[95] = 63; // '_' (base64url)

  const HEX_LUT = new Int8Array(256).fill(-1);
  for (let i = 0; i < 16; i += 1) {
    HEX_LUT["0123456789abcdef".charCodeAt(i)] = i;
    HEX_LUT["0123456789ABCDEF".charCodeAt(i)] = i;
  }

  function normalizeEncoding(enc) {
    if (enc === undefined || enc === null) {
      return "utf8";
    }
    const e = String(enc).toLowerCase();
    switch (e) {
      case "utf8": case "utf-8": return "utf8";
      case "utf16le": case "utf-16le": case "ucs2": case "ucs-2": return "utf16le";
      case "latin1": case "binary": return "latin1";
      case "ascii": return "ascii";
      case "base64": return "base64";
      case "base64url": return "base64url";
      case "hex": return "hex";
      default: return undefined;
    }
  }

  function assertEncoding(enc) {
    const n = normalizeEncoding(enc);
    if (n === undefined) {
      throw makeError(TypeError, "ERR_UNKNOWN_ENCODING", "Unknown encoding: " + enc);
    }
    return n;
  }

  function encodeUtf8(str) {
    return utf8Encoder.encode(str);
  }

  function encodeLatin1(str) {
    const out = new Uint8Array(str.length);
    for (let i = 0; i < str.length; i += 1) {
      out[i] = str.charCodeAt(i) & 0xff;
    }
    return out;
  }

  function encodeAscii(str) {
    const out = new Uint8Array(str.length);
    for (let i = 0; i < str.length; i += 1) {
      out[i] = str.charCodeAt(i) & 0xff;
    }
    return out;
  }

  function encodeUtf16le(str) {
    const out = new Uint8Array(str.length * 2);
    for (let i = 0; i < str.length; i += 1) {
      const c = str.charCodeAt(i);
      out[i * 2] = c & 0xff;
      out[i * 2 + 1] = c >>> 8;
    }
    return out;
  }

  function encodeHex(str) {
    const len = str.length >>> 1;
    const out = new Uint8Array(len);
    let produced = 0;
    for (let i = 0; i < len; i += 1) {
      const hi = HEX_LUT[str.charCodeAt(i * 2)];
      const lo = HEX_LUT[str.charCodeAt(i * 2 + 1)];
      if (hi === -1 || lo === -1) {
        break;
      }
      out[produced] = (hi << 4) | lo;
      produced += 1;
    }
    return produced === len ? out : out.subarray(0, produced);
  }

  function encodeBase64(str) {
    // Tolerant decoder: skips whitespace and invalid characters, accepts
    // both standard and url-safe alphabets, ignores padding (Node behavior).
    const codesArr = [];
    for (let i = 0; i < str.length; i += 1) {
      const v = B64_LUT[str.charCodeAt(i)];
      if (v !== -1) {
        codesArr.push(v);
      } else if (str[i] === "=") {
        break;
      }
    }
    const outLen = Math.floor((codesArr.length * 3) / 4);
    const out = new Uint8Array(outLen);
    let o = 0;
    for (let i = 0; i + 1 < codesArr.length; i += 4) {
      const a = codesArr[i];
      const b = codesArr[i + 1];
      const c = i + 2 < codesArr.length ? codesArr[i + 2] : 0;
      const d = i + 3 < codesArr.length ? codesArr[i + 3] : 0;
      if (o < outLen) out[o++] = (a << 2) | (b >>> 4);
      if (i + 2 < codesArr.length && o < outLen) out[o++] = ((b & 15) << 4) | (c >>> 2);
      if (i + 3 < codesArr.length && o < outLen) out[o++] = ((c & 3) << 6) | d;
    }
    return out;
  }

  function encodeString(str, encoding) {
    switch (encoding) {
      case "utf8": return encodeUtf8(str);
      case "latin1": return encodeLatin1(str);
      case "ascii": return encodeAscii(str);
      case "utf16le": return encodeUtf16le(str);
      case "hex": return encodeHex(str);
      case "base64": case "base64url": return encodeBase64(str);
      default:
        throw makeError(TypeError, "ERR_UNKNOWN_ENCODING", "Unknown encoding: " + encoding);
    }
  }

  function decodeLatin1(buf) {
    let out = "";
    const CHUNK = 4096;
    for (let i = 0; i < buf.length; i += CHUNK) {
      out += String.fromCharCode.apply(null, buf.subarray(i, Math.min(i + CHUNK, buf.length)));
    }
    return out;
  }

  function decodeAscii(buf) {
    let out = "";
    const CHUNK = 4096;
    const masked = new Uint8Array(buf.length);
    for (let i = 0; i < buf.length; i += 1) {
      masked[i] = buf[i] & 0x7f;
    }
    for (let i = 0; i < masked.length; i += CHUNK) {
      out += String.fromCharCode.apply(null, masked.subarray(i, Math.min(i + CHUNK, masked.length)));
    }
    return out;
  }

  function decodeHex(buf) {
    let out = "";
    for (let i = 0; i < buf.length; i += 1) {
      out += HEX_CHARS[buf[i] >>> 4] + HEX_CHARS[buf[i] & 15];
    }
    return out;
  }

  function decodeBase64Generic(buf, alphabet, pad) {
    let out = "";
    let i = 0;
    for (; i + 2 < buf.length; i += 3) {
      const n = (buf[i] << 16) | (buf[i + 1] << 8) | buf[i + 2];
      out += alphabet[(n >>> 18) & 63] + alphabet[(n >>> 12) & 63] + alphabet[(n >>> 6) & 63] + alphabet[n & 63];
    }
    const rem = buf.length - i;
    if (rem === 1) {
      const n = buf[i] << 16;
      out += alphabet[(n >>> 18) & 63] + alphabet[(n >>> 12) & 63] + (pad ? "==" : "");
    } else if (rem === 2) {
      const n = (buf[i] << 16) | (buf[i + 1] << 8);
      out += alphabet[(n >>> 18) & 63] + alphabet[(n >>> 12) & 63] + alphabet[(n >>> 6) & 63] + (pad ? "=" : "");
    }
    return out;
  }

  function decodeString(buf, encoding) {
    switch (encoding) {
      case "utf8": return utf8Decoder.decode(buf);
      case "latin1": return decodeLatin1(buf);
      case "ascii": return decodeAscii(buf);
      case "utf16le": return utf16Decoder.decode(buf.length % 2 === 0 ? buf : buf.subarray(0, buf.length - 1));
      case "hex": return decodeHex(buf);
      case "base64": return decodeBase64Generic(buf, B64_STD, true);
      case "base64url": return decodeBase64Generic(buf, B64_URL, false);
      default:
        throw makeError(TypeError, "ERR_UNKNOWN_ENCODING", "Unknown encoding: " + encoding);
    }
  }

  // ---------------------------------------------------------------------
  // Buffer class
  // ---------------------------------------------------------------------
  // Realm-robust brand checks (instanceof fails across vm/host realms).
  function isAnyArrayBuffer(value) {
    if (value instanceof ArrayBuffer) return true;
    const tag = Object.prototype.toString.call(value);
    return tag === "[object ArrayBuffer]" || tag === "[object SharedArrayBuffer]";
  }

  class Buffer extends Uint8Array {
    static from(value, encodingOrOffset, length) {
      if (typeof value === "string") {
        const enc = assertEncoding(encodingOrOffset);
        const bytes = encodeString(value, enc);
        return wrap(bytes.buffer === undefined ? new Uint8Array(bytes) : bytes);
      }
      if (isAnyArrayBuffer(value)) {
        // Shares memory, like Node.
        const offset = encodingOrOffset === undefined ? 0 : Number(encodingOrOffset);
        const len = length === undefined ? value.byteLength - offset : Number(length);
        if (offset < 0 || offset > value.byteLength) {
          throw makeError(RangeError, "ERR_BUFFER_OUT_OF_BOUNDS", '"offset" is outside of buffer bounds');
        }
        if (len < 0 || offset + len > value.byteLength) {
          throw makeError(RangeError, "ERR_BUFFER_OUT_OF_BOUNDS", '"length" is outside of buffer bounds');
        }
        return new Buffer(value, offset, len);
      }
      if (ArrayBuffer.isView(value)) {
        // Copies data (TypedArray source), like Node.
        if (value instanceof Uint8Array ||
            Object.prototype.toString.call(value) === "[object Uint8Array]") {
          const copy = new Buffer(value.byteLength);
          copy.set(new Uint8Array(value.buffer, value.byteOffset, value.byteLength));
          return copy;
        }
        // Non-u8 TypedArray: copy element values (Node semantics).
        const arrLike = value;
        const copy = allocUnsafe(arrLike.length);
        for (let i = 0; i < arrLike.length; i += 1) {
          copy[i] = Number(arrLike[i]) & 0xff;
        }
        return copy;
      }
      if (Array.isArray(value)) {
        const copy = allocUnsafe(value.length);
        for (let i = 0; i < value.length; i += 1) {
          copy[i] = Number(value[i]) & 0xff;
        }
        return copy;
      }
      if (value !== null && typeof value === "object") {
        if (typeof value.valueOf === "function" && value.valueOf() !== value) {
          return Buffer.from(value.valueOf(), encodingOrOffset, length);
        }
        if (value.type === "Buffer" && Array.isArray(value.data)) {
          return Buffer.from(value.data);
        }
        if (typeof value[Symbol.toPrimitive] === "function") {
          return Buffer.from(value[Symbol.toPrimitive]("string"), encodingOrOffset, length);
        }
      }
      throw makeError(
        TypeError,
        "ERR_INVALID_ARG_TYPE",
        "The first argument must be of type string or an instance of Buffer, ArrayBuffer, or Array or an Array-like Object. Received " +
          (value === null ? "null" : typeof value)
      );
    }

    static of(...items) {
      return Buffer.from(items);
    }

    static alloc(size, fill, encoding) {
      validateSize(size);
      const buf = new Buffer(size);
      if (fill !== undefined && fill !== 0 && size > 0) {
        buf.fill(fill, 0, size, encoding);
      }
      return buf;
    }

    static allocUnsafe(size) {
      validateSize(size);
      return new Buffer(size);
    }

    static allocUnsafeSlow(size) {
      validateSize(size);
      return new Buffer(size);
    }

    static isBuffer(obj) {
      return obj instanceof Buffer;
    }

    static isEncoding(encoding) {
      return typeof encoding === "string" && normalizeEncoding(encoding) !== undefined;
    }

    static byteLength(value, encoding) {
      if (typeof value !== "string") {
        if (ArrayBuffer.isView(value) || value instanceof ArrayBuffer) {
          return value.byteLength;
        }
        throw makeError(
          TypeError,
          "ERR_INVALID_ARG_TYPE",
          'The "string" argument must be of type string or an instance of Buffer or ArrayBuffer. Received ' + typeof value
        );
      }
      const enc = normalizeEncoding(encoding) || "utf8";
      switch (enc) {
        case "latin1": case "ascii": return value.length;
        case "utf16le": return value.length * 2;
        case "hex": return value.length >>> 1;
        case "base64": case "base64url": {
          let len = value.length;
          while (len > 0 && (value[len - 1] === "=" || value[len - 1] === " " || value[len - 1] === "\n")) {
            len -= 1;
          }
          return Math.floor((len * 3) / 4);
        }
        default: {
          // utf8: precise length without allocating the full encoding
          let bytes = 0;
          for (let i = 0; i < value.length; i += 1) {
            const c = value.codePointAt(i);
            if (c > 0xffff) i += 1;
            bytes += c < 0x80 ? 1 : c < 0x800 ? 2 : c < 0x10000 ? 3 : 4;
          }
          return bytes;
        }
      }
    }

    static compare(a, b) {
      if (!(a instanceof Uint8Array) || !(b instanceof Uint8Array)) {
        throw makeError(
          TypeError,
          "ERR_INVALID_ARG_TYPE",
          'The "buf1", "buf2" arguments must be one of type Buffer or Uint8Array'
        );
      }
      return compareBytes(a, 0, a.length, b, 0, b.length);
    }

    static concat(list, totalLength) {
      if (!Array.isArray(list)) {
        throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", '"list" argument must be an Array of Buffers');
      }
      if (list.length === 0) {
        return Buffer.alloc(0);
      }
      let total = totalLength;
      if (total === undefined) {
        total = 0;
        for (const b of list) {
          total += b.length;
        }
      } else {
        total = Number(total) >>> 0;
      }
      const out = Buffer.allocUnsafe(total);
      let pos = 0;
      for (const b of list) {
        if (!(b instanceof Uint8Array)) {
          throw makeError(
            TypeError,
            "ERR_INVALID_ARG_TYPE",
            '"list" argument must be an Array of Buffers or Uint8Arrays'
          );
        }
        const n = Math.min(b.length, total - pos);
        out.set(n === b.length ? b : b.subarray(0, n), pos);
        pos += n;
        if (pos >= total) break;
      }
      if (pos < total) {
        out.fill(0, pos, total);
      }
      return out;
    }

    static copyBytesFrom(view, offset, length) {
      if (!ArrayBuffer.isView(view)) {
        throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", '"view" must be a TypedArray');
      }
      const off = offset === undefined ? 0 : Number(offset);
      const viewLength = view.length !== undefined ? view.length : view.byteLength;
      const len = length === undefined ? viewLength - off : Number(length);
      const bytesPer = view.BYTES_PER_ELEMENT || 1;
      const start = view.byteOffset + off * bytesPer;
      const byteLen = Math.max(0, Math.min(len * bytesPer, view.byteLength - off * bytesPer));
      const src = new Uint8Array(view.buffer, start, byteLen);
      const out = Buffer.allocUnsafe(byteLen);
      out.set(src);
      return out;
    }

    // -------------------------------------------------------------------
    toString(encoding, start, end) {
      const enc = assertEncoding(encoding);
      const s = start === undefined ? 0 : Math.max(0, Math.min(this.length, Number(start) | 0));
      const e = end === undefined ? this.length : Math.max(s, Math.min(this.length, Number(end) | 0));
      if (e <= s) {
        return "";
      }
      return decodeString(this.subarray(s, e), enc);
    }

    write(string, offset, length, encoding) {
      if (typeof string !== "string") {
        throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", '"string" must be a string');
      }
      let off = 0;
      let len = this.length;
      let enc = "utf8";
      if (typeof offset === "string") {
        enc = assertEncoding(offset);
      } else if (offset !== undefined) {
        off = Number(offset) >>> 0;
        if (typeof length === "string") {
          enc = assertEncoding(length);
          len = this.length - off;
        } else if (length !== undefined) {
          len = Number(length) >>> 0;
          if (encoding !== undefined) {
            enc = assertEncoding(encoding);
          }
        } else {
          len = this.length - off;
        }
      }
      if (off > this.length) {
        throw makeError(RangeError, "ERR_OUT_OF_RANGE", '"offset" is outside of buffer bounds');
      }
      len = Math.min(len, this.length - off);
      const bytes = encodeString(string, enc);
      let toWrite = Math.min(bytes.length, len);
      if (enc === "utf16le") {
        toWrite -= toWrite % 2;
      }
      this.set(bytes.subarray(0, toWrite), off);
      return toWrite;
    }

    equals(other) {
      if (!(other instanceof Uint8Array)) {
        throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", '"otherBuffer" must be a Buffer or Uint8Array');
      }
      if (this === other) return true;
      if (this.length !== other.length) return false;
      return compareBytes(this, 0, this.length, other, 0, other.length) === 0;
    }

    compare(target, targetStart, targetEnd, sourceStart, sourceEnd) {
      if (!(target instanceof Uint8Array)) {
        throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", '"target" must be a Buffer or Uint8Array');
      }
      const ts = targetStart === undefined ? 0 : Number(targetStart) >>> 0;
      const te = targetEnd === undefined ? target.length : Number(targetEnd) >>> 0;
      const ss = sourceStart === undefined ? 0 : Number(sourceStart) >>> 0;
      const se = sourceEnd === undefined ? this.length : Number(sourceEnd) >>> 0;
      if (ts > target.length || te > target.length || ss > this.length || se > this.length) {
        throw makeError(RangeError, "ERR_OUT_OF_RANGE", "out of range index");
      }
      return compareBytes(this, ss, se, target, ts, te);
    }

    copy(target, targetStart, sourceStart, sourceEnd) {
      if (!(target instanceof Uint8Array)) {
        throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", '"target" must be a Buffer or Uint8Array');
      }
      const ts = targetStart === undefined ? 0 : Number(targetStart) >>> 0;
      const ss = sourceStart === undefined ? 0 : Number(sourceStart) >>> 0;
      let se = sourceEnd === undefined ? this.length : Number(sourceEnd) >>> 0;
      if (ts >= target.length || ss >= se) {
        return 0;
      }
      se = Math.min(se, this.length, ss + (target.length - ts));
      const chunk = this.subarray(ss, se);
      target.set(chunk, ts);
      return chunk.length;
    }

    slice(start, end) {
      return this.subarray(start, end);
    }

    subarray(start, end) {
      const view = super.subarray(start, end);
      // Ensure Buffer prototype (super.subarray on a subclass already returns
      // the subclass via species; enforce defensively).
      return view instanceof Buffer ? view : wrap(view);
    }

    fill(value, start, end, encoding) {
      let s = 0;
      let e = this.length;
      let enc;
      if (typeof start === "string") {
        enc = assertEncoding(start);
      } else {
        if (start !== undefined) s = Number(start) >>> 0;
        if (typeof end === "string") {
          enc = assertEncoding(end);
        } else {
          if (end !== undefined) e = Number(end) >>> 0;
          if (encoding !== undefined) enc = assertEncoding(encoding);
        }
      }
      if (s > this.length || e > this.length) {
        throw makeError(RangeError, "ERR_OUT_OF_RANGE", "out of range index");
      }
      if (e <= s) {
        return this;
      }
      if (typeof value === "number") {
        super.fill(value & 0xff, s, e);
        return this;
      }
      let pattern;
      if (typeof value === "string") {
        pattern = encodeString(value, enc || "utf8");
        if (pattern.length === 0) {
          super.fill(0, s, e);
          return this;
        }
      } else if (value instanceof Uint8Array) {
        pattern = value;
        if (pattern.length === 0) {
          super.fill(0, s, e);
          return this;
        }
      } else {
        throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", "fill value must be string, number, or Uint8Array");
      }
      for (let i = s; i < e; i += 1) {
        this[i] = pattern[(i - s) % pattern.length];
      }
      return this;
    }

    indexOf(value, byteOffset, encoding) {
      return bidirectionalIndexOf(this, value, byteOffset, encoding, true);
    }

    lastIndexOf(value, byteOffset, encoding) {
      return bidirectionalIndexOf(this, value, byteOffset, encoding, false);
    }

    includes(value, byteOffset, encoding) {
      return this.indexOf(value, byteOffset, encoding) !== -1;
    }

    toJSON() {
      return { type: "Buffer", data: Array.from(this) };
    }

    swap16() {
      if (this.length % 2 !== 0) {
        throw makeError(RangeError, "ERR_INVALID_BUFFER_SIZE", "Buffer size must be a multiple of 16-bits");
      }
      for (let i = 0; i < this.length; i += 2) {
        const t = this[i];
        this[i] = this[i + 1];
        this[i + 1] = t;
      }
      return this;
    }

    swap32() {
      if (this.length % 4 !== 0) {
        throw makeError(RangeError, "ERR_INVALID_BUFFER_SIZE", "Buffer size must be a multiple of 32-bits");
      }
      for (let i = 0; i < this.length; i += 4) {
        let t = this[i]; this[i] = this[i + 3]; this[i + 3] = t;
        t = this[i + 1]; this[i + 1] = this[i + 2]; this[i + 2] = t;
      }
      return this;
    }

    swap64() {
      if (this.length % 8 !== 0) {
        throw makeError(RangeError, "ERR_INVALID_BUFFER_SIZE", "Buffer size must be a multiple of 64-bits");
      }
      for (let i = 0; i < this.length; i += 8) {
        for (let j = 0; j < 4; j += 1) {
          const t = this[i + j];
          this[i + j] = this[i + 7 - j];
          this[i + 7 - j] = t;
        }
      }
      return this;
    }
  }

  function wrap(u8) {
    return new Buffer(u8.buffer, u8.byteOffset, u8.byteLength);
  }

  function validateSize(size) {
    if (typeof size !== "number" || Number.isNaN(size) || size < 0) {
      throw makeError(RangeError, "ERR_OUT_OF_RANGE", 'The value of "size" is out of range. It must be >= 0. Received ' + size);
    }
    if (size > kMaxLength) {
      throw makeError(RangeError, "ERR_OUT_OF_RANGE", '"size" exceeds kMaxLength');
    }
  }

  function compareBytes(a, as, ae, b, bs, be) {
    const alen = ae - as;
    const blen = be - bs;
    const n = Math.min(alen, blen);
    for (let i = 0; i < n; i += 1) {
      if (a[as + i] !== b[bs + i]) {
        return a[as + i] < b[bs + i] ? -1 : 1;
      }
    }
    if (alen < blen) return -1;
    if (alen > blen) return 1;
    return 0;
  }

  function bidirectionalIndexOf(buffer, value, byteOffset, encoding, forward) {
    if (typeof byteOffset === "string") {
      encoding = byteOffset;
      byteOffset = undefined;
    }
    let needle;
    if (typeof value === "number") {
      needle = new Uint8Array([value & 0xff]);
    } else if (typeof value === "string") {
      needle = encodeString(value, assertEncoding(encoding));
    } else if (value instanceof Uint8Array) {
      needle = value;
    } else {
      throw makeError(
        TypeError,
        "ERR_INVALID_ARG_TYPE",
        'The "value" argument must be one of type number or string or an instance of Buffer or Uint8Array'
      );
    }

    let start;
    if (forward) {
      start = byteOffset === undefined ? 0 : Math.trunc(Number(byteOffset));
      if (start < 0) start = Math.max(0, buffer.length + start);
      if (Number.isNaN(start)) start = 0;
      if (needle.length === 0) return Math.min(start, buffer.length);
      for (let i = start; i <= buffer.length - needle.length; i += 1) {
        let found = true;
        for (let j = 0; j < needle.length; j += 1) {
          if (buffer[i + j] !== needle[j]) {
            found = false;
            break;
          }
        }
        if (found) return i;
      }
      return -1;
    }
    start = byteOffset === undefined ? buffer.length - needle.length : Math.trunc(Number(byteOffset));
    if (Number.isNaN(start)) start = buffer.length - needle.length;
    if (start < 0) start = buffer.length + start;
    if (needle.length === 0) return Math.max(0, Math.min(start, buffer.length));
    for (let i = Math.min(start, buffer.length - needle.length); i >= 0; i -= 1) {
      let found = true;
      for (let j = 0; j < needle.length; j += 1) {
        if (buffer[i + j] !== needle[j]) {
          found = false;
          break;
        }
      }
      if (found) return i;
    }
    return -1;
  }

  const allocUnsafe = Buffer.allocUnsafe;

  // ---------------------------------------------------------------------
  // Numeric read/write methods (spec-table driven, no codegen)
  // ---------------------------------------------------------------------
  function boundsCheck(buf, offset, width) {
    const off = Number(offset);
    if (!Number.isInteger(off)) {
      throw makeError(RangeError, "ERR_OUT_OF_RANGE", 'The value of "offset" is out of range. Received ' + offset);
    }
    if (off < 0 || off + width > buf.length) {
      throw makeError(
        RangeError,
        "ERR_OUT_OF_RANGE",
        'The value of "offset" is out of range. It must be >= 0 and <= ' + (buf.length - width) + ". Received " + off
      );
    }
    return off;
  }

  function dv(buf) {
    return new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
  }

  const NUM_METHODS = [
    ["UInt8", 1, "getUint8", "setUint8", null],
    ["UInt16LE", 2, "getUint16", "setUint16", true],
    ["UInt16BE", 2, "getUint16", "setUint16", false],
    ["UInt32LE", 4, "getUint32", "setUint32", true],
    ["UInt32BE", 4, "getUint32", "setUint32", false],
    ["Int8", 1, "getInt8", "setInt8", null],
    ["Int16LE", 2, "getInt16", "setInt16", true],
    ["Int16BE", 2, "getInt16", "setInt16", false],
    ["Int32LE", 4, "getInt32", "setInt32", true],
    ["Int32BE", 4, "getInt32", "setInt32", false],
    ["FloatLE", 4, "getFloat32", "setFloat32", true],
    ["FloatBE", 4, "getFloat32", "setFloat32", false],
    ["DoubleLE", 8, "getFloat64", "setFloat64", true],
    ["DoubleBE", 8, "getFloat64", "setFloat64", false],
    ["BigUInt64LE", 8, "getBigUint64", "setBigUint64", true],
    ["BigUInt64BE", 8, "getBigUint64", "setBigUint64", false],
    ["BigInt64LE", 8, "getBigInt64", "setBigInt64", true],
    ["BigInt64BE", 8, "getBigInt64", "setBigInt64", false],
  ];
  for (const [name, width, getter, setter, little] of NUM_METHODS) {
    Buffer.prototype["read" + name] = function (offset) {
      const off = boundsCheck(this, offset === undefined ? 0 : offset, width);
      const view = dv(this);
      return little === null ? view[getter](off) : view[getter](off, little);
    };
    Buffer.prototype["write" + name] = function (value, offset) {
      const off = boundsCheck(this, offset === undefined ? 0 : offset, width);
      const view = dv(this);
      if (little === null) {
        view[setter](off, value);
      } else {
        view[setter](off, value, little);
      }
      return off + width;
    };
  }
  // Aliases matching Node (readUIntLE family handled below; float aliases exact).
  Buffer.prototype.readBigUint64LE = Buffer.prototype.readBigUInt64LE;
  Buffer.prototype.readBigUint64BE = Buffer.prototype.readBigUInt64BE;
  Buffer.prototype.writeBigUint64LE = Buffer.prototype.writeBigUInt64LE;
  Buffer.prototype.writeBigUint64BE = Buffer.prototype.writeBigUInt64BE;
  Buffer.prototype.readUint8 = Buffer.prototype.readUInt8;
  Buffer.prototype.readUint16LE = Buffer.prototype.readUInt16LE;
  Buffer.prototype.readUint16BE = Buffer.prototype.readUInt16BE;
  Buffer.prototype.readUint32LE = Buffer.prototype.readUInt32LE;
  Buffer.prototype.readUint32BE = Buffer.prototype.readUInt32BE;
  Buffer.prototype.writeUint8 = Buffer.prototype.writeUInt8;
  Buffer.prototype.writeUint16LE = Buffer.prototype.writeUInt16LE;
  Buffer.prototype.writeUint16BE = Buffer.prototype.writeUInt16BE;
  Buffer.prototype.writeUint32LE = Buffer.prototype.writeUInt32LE;
  Buffer.prototype.writeUint32BE = Buffer.prototype.writeUInt32BE;

  // Variable-width integer read/write (1..6 bytes).
  function checkByteLength(byteLength) {
    const n = Number(byteLength);
    if (!Number.isInteger(n) || n < 1 || n > 6) {
      throw makeError(RangeError, "ERR_OUT_OF_RANGE", '"byteLength" must be an integer in [1, 6]');
    }
    return n;
  }
  Buffer.prototype.readUIntLE = function (offset, byteLength) {
    const n = checkByteLength(byteLength);
    const off = boundsCheck(this, offset, n);
    let val = 0;
    let mul = 1;
    for (let i = 0; i < n; i += 1) {
      val += this[off + i] * mul;
      mul *= 256;
    }
    return val;
  };
  Buffer.prototype.readUIntBE = function (offset, byteLength) {
    const n = checkByteLength(byteLength);
    const off = boundsCheck(this, offset, n);
    let val = 0;
    for (let i = 0; i < n; i += 1) {
      val = val * 256 + this[off + i];
    }
    return val;
  };
  Buffer.prototype.readIntLE = function (offset, byteLength) {
    const n = checkByteLength(byteLength);
    const unsigned = this.readUIntLE(offset, n);
    const limit = 2 ** (8 * n - 1);
    return unsigned >= limit ? unsigned - limit * 2 : unsigned;
  };
  Buffer.prototype.readIntBE = function (offset, byteLength) {
    const n = checkByteLength(byteLength);
    const unsigned = this.readUIntBE(offset, n);
    const limit = 2 ** (8 * n - 1);
    return unsigned >= limit ? unsigned - limit * 2 : unsigned;
  };
  Buffer.prototype.writeUIntLE = function (value, offset, byteLength) {
    const n = checkByteLength(byteLength);
    const off = boundsCheck(this, offset, n);
    let v = Number(value);
    for (let i = 0; i < n; i += 1) {
      this[off + i] = v & 0xff;
      v = Math.floor(v / 256);
    }
    return off + n;
  };
  Buffer.prototype.writeUIntBE = function (value, offset, byteLength) {
    const n = checkByteLength(byteLength);
    const off = boundsCheck(this, offset, n);
    let v = Number(value);
    for (let i = n - 1; i >= 0; i -= 1) {
      this[off + i] = v & 0xff;
      v = Math.floor(v / 256);
    }
    return off + n;
  };
  Buffer.prototype.writeIntLE = function (value, offset, byteLength) {
    const n = checkByteLength(byteLength);
    const limit = 2 ** (8 * n);
    let v = Number(value);
    if (v < 0) v += limit;
    return this.writeUIntLE(v, offset, n);
  };
  Buffer.prototype.writeIntBE = function (value, offset, byteLength) {
    const n = checkByteLength(byteLength);
    const limit = 2 ** (8 * n);
    let v = Number(value);
    if (v < 0) v += limit;
    return this.writeUIntBE(v, offset, n);
  };
  Buffer.prototype.readUintLE = Buffer.prototype.readUIntLE;
  Buffer.prototype.readUintBE = Buffer.prototype.readUIntBE;
  Buffer.prototype.writeUintLE = Buffer.prototype.writeUIntLE;
  Buffer.prototype.writeUintBE = Buffer.prototype.writeUIntBE;

  // Custom inspect
  const customInspect = Symbol.for("nodejs.util.inspect.custom");
  Buffer.prototype[customInspect] = function () {
    const max = INSPECT_MAX_BYTES;
    const shown = this.subarray(0, max);
    let hex = "";
    for (let i = 0; i < shown.length; i += 1) {
      hex += (i > 0 ? " " : "") + HEX_CHARS[shown[i] >>> 4] + HEX_CHARS[shown[i] & 15];
    }
    const more = this.length > max ? " ... " + (this.length - max) + " more bytes" : "";
    return "<Buffer " + hex + more + ">";
  };

  Buffer.poolSize = 8192;

  // Real Node's Buffer statics are assigned properties, hence ENUMERABLE —
  // and packages depend on that: safer-buffer (used by iconv-lite →
  // body-parser → express) clones Buffer via `for (key in Buffer)`, which
  // skips ES-class statics (non-enumerable by spec). Re-flag them to match
  // real Node exactly (verified against Node v22: poolSize, from,
  // copyBytesFrom, of, alloc, allocUnsafe, allocUnsafeSlow, isBuffer,
  // compare, isEncoding, concat, byteLength are all for..in-visible).
  for (const key of Object.getOwnPropertyNames(Buffer)) {
    if (key === "length" || key === "name" || key === "prototype") continue;
    const desc = Object.getOwnPropertyDescriptor(Buffer, key);
    if (desc && !desc.enumerable && desc.configurable) {
      Object.defineProperty(Buffer, key, { ...desc, enumerable: true });
    }
  }

  // ---------------------------------------------------------------------
  // Module exports
  // ---------------------------------------------------------------------
  function SlowBuffer(size) {
    return Buffer.allocUnsafeSlow(size);
  }
  SlowBuffer.prototype = Buffer.prototype;

  function atob_(input) {
    const str = String(input).replace(/[\t\n\f\r ]/g, "");
    if (!/^[A-Za-z0-9+/]*={0,2}$/.test(str) || str.length % 4 === 1) {
      const err = new Error("The string to be decoded is not correctly encoded.");
      err.name = "InvalidCharacterError";
      err.code = 5;
      throw err;
    }
    return decodeLatin1(encodeBase64(str));
  }

  function btoa_(input) {
    const str = String(input);
    for (let i = 0; i < str.length; i += 1) {
      if (str.charCodeAt(i) > 0xff) {
        const err = new Error("The string to be encoded contains characters outside of the Latin1 range.");
        err.name = "InvalidCharacterError";
        err.code = 5;
        throw err;
      }
    }
    return decodeBase64Generic(encodeLatin1(str), B64_STD, true);
  }

  function isUtf8(input) {
    if (!(input instanceof Uint8Array) && !(input instanceof ArrayBuffer)) {
      throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", '"input" must be a Buffer, TypedArray, or ArrayBuffer');
    }
    const buf = input instanceof ArrayBuffer ? new Uint8Array(input) : input;
    try {
      new TextDecoder("utf-8", { fatal: true }).decode(buf);
      return true;
    } catch (_e) {
      return false;
    }
  }

  function isAscii(input) {
    if (!(input instanceof Uint8Array) && !(input instanceof ArrayBuffer)) {
      throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", '"input" must be a Buffer, TypedArray, or ArrayBuffer');
    }
    const buf = input instanceof ArrayBuffer ? new Uint8Array(input) : input;
    for (let i = 0; i < buf.length; i += 1) {
      if (buf[i] > 0x7f) return false;
    }
    return true;
  }

  function transcode(source, fromEnc, toEnc) {
    if (!(source instanceof Uint8Array)) {
      throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", '"source" must be a Buffer or Uint8Array');
    }
    const from = assertEncoding(fromEnc);
    const to = assertEncoding(toEnc);
    const supported = new Set(["utf8", "latin1", "ascii", "utf16le"]);
    if (!supported.has(from) || !supported.has(to)) {
      throw makeError(Error, "ERR_UNKNOWN_ENCODING", "Unable to transcode Buffer from " + from + " to " + to);
    }
    const str = decodeString(source, from);
    if (to === "latin1" || to === "ascii") {
      // Unmappable characters become '?' (ICU behavior in Node).
      let out = "";
      for (const ch of str) {
        out += ch.codePointAt(0) > 0xff ? "?" : ch;
      }
      return wrap(encodeLatin1(out));
    }
    return wrap(encodeString(str, to));
  }

  function resolveObjectURL() {
    const { unsupported } = require("internal/errors");
    throw unsupported("buffer.resolveObjectURL");
  }

  class File extends Blob {
    constructor(fileBits, fileName, options) {
      super(fileBits, options);
      this._name = String(fileName);
      this._lastModified = options && options.lastModified !== undefined ? Number(options.lastModified) : Date.now();
    }
    get name() {
      return this._name;
    }
    get lastModified() {
      return this._lastModified;
    }
    get [Symbol.toStringTag]() {
      return "File";
    }
  }

  module.exports = {
    Buffer,
    SlowBuffer,
    kMaxLength,
    kStringMaxLength,
    constants: { MAX_LENGTH: kMaxLength, MAX_STRING_LENGTH: kStringMaxLength },
    INSPECT_MAX_BYTES,
    atob: atob_,
    btoa: btoa_,
    isUtf8,
    isAscii,
    transcode,
    resolveObjectURL,
    Blob: typeof Blob !== "undefined" ? Blob : undefined,
    File: typeof Blob !== "undefined" ? File : undefined,
    __installGlobals(g) {
      g.Buffer = Buffer;
      if (typeof g.atob !== "function") g.atob = atob_;
      if (typeof g.btoa !== "function") g.btoa = btoa_;
    },
  };
});
