"use strict";
// node:string_decoder — incremental decoder with partial-sequence buffering.
__nanoNodeRegister("string_decoder", function (module, exports, require) {
  const { makeError } = require("internal/errors");

  function normalizeEncoding(enc) {
    const { Buffer } = require("buffer");
    if (enc === undefined || enc === null) return "utf8";
    const e = String(enc).toLowerCase();
    if (!Buffer.isEncoding(e)) {
      throw makeError(TypeError, "ERR_UNKNOWN_ENCODING", "Unknown encoding: " + enc);
    }
    switch (e) {
      case "utf-8": return "utf8";
      case "utf-16le": case "ucs2": case "ucs-2": return "utf16le";
      case "binary": return "latin1";
      default: return e;
    }
  }

  class StringDecoder {
    constructor(encoding) {
      this.encoding = normalizeEncoding(encoding);
      const BufferMod = require("buffer");
      this._Buffer = BufferMod.Buffer;
      switch (this.encoding) {
        case "utf8":
          this._decoder = new TextDecoder("utf-8");
          this._mode = "utf8";
          break;
        case "utf16le":
          this._decoder = new TextDecoder("utf-16le");
          this._mode = "pair";
          this._unit = 2;
          break;
        case "base64":
        case "base64url":
          this._mode = "group";
          this._unit = 3;
          break;
        case "hex":
          this._mode = "pair";
          this._unit = 1; // hex has no partial bytes (byte -> 2 chars)
          break;
        default:
          this._mode = "single";
          break;
      }
      this._pending = new Uint8Array(0);
    }

    get lastNeed() {
      if (this._mode === "utf8") {
        return this._pending.length === 0 ? 0 : utf8SeqLength(this._pending[0]) - this._pending.length;
      }
      return 0;
    }

    get lastTotal() {
      if (this._mode === "utf8" && this._pending.length > 0) {
        return utf8SeqLength(this._pending[0]);
      }
      return 0;
    }

    write(buf) {
      if (typeof buf === "string") {
        return buf;
      }
      // Realm-robust view check (instanceof fails across vm/host realms).
      if (!ArrayBuffer.isView(buf)) {
        throw makeError(
          TypeError,
          "ERR_INVALID_ARG_TYPE",
          'The "buf" argument must be an instance of Buffer, TypedArray, or DataView. Received ' + typeof buf
        );
      }
      const bytes = new Uint8Array(buf.buffer, buf.byteOffset, buf.byteLength);

      switch (this._mode) {
        case "single":
          return this._Buffer.from(bytes).toString(this.encoding);
        case "utf8":
          return this._writeUtf8(bytes);
        case "pair":
          if (this.encoding === "hex") {
            return this._Buffer.from(bytes).toString("hex");
          }
          return this._writeUtf16(bytes);
        case "group":
          return this._writeGrouped(bytes, 3, (b) => this._Buffer.from(b).toString(this.encoding));
        default:
          return "";
      }
    }

    _writeUtf8(bytes) {
      let data = bytes;
      if (this._pending.length > 0) {
        data = new Uint8Array(this._pending.length + bytes.length);
        data.set(this._pending, 0);
        data.set(bytes, this._pending.length);
        this._pending = new Uint8Array(0);
      }
      // Find a trailing incomplete UTF-8 sequence (up to 3 bytes back).
      let end = data.length;
      let cut = end;
      for (let back = 1; back <= 3 && back <= data.length; back += 1) {
        const b = data[end - back];
        if ((b & 0xc0) === 0xc0) {
          // Lead byte found at distance `back`
          const need = utf8SeqLength(b);
          if (need > back) {
            cut = end - back;
          }
          break;
        }
        if ((b & 0x80) === 0) {
          break; // ASCII: sequence complete
        }
        // continuation byte: keep scanning back
      }
      const complete = data.subarray(0, cut);
      this._pending = data.subarray(cut).slice();
      return this._decoder.decode(complete);
    }

    _writeUtf16(bytes) {
      let data = bytes;
      if (this._pending.length > 0) {
        data = new Uint8Array(this._pending.length + bytes.length);
        data.set(this._pending, 0);
        data.set(bytes, this._pending.length);
        this._pending = new Uint8Array(0);
      }
      // Hold back an odd trailing byte, plus a trailing lone high surrogate
      // (Node buffers the lead surrogate until its pair arrives).
      let cut = data.length - (data.length % 2);
      if (cut >= 2) {
        const lastUnit = data[cut - 2] | (data[cut - 1] << 8);
        if (lastUnit >= 0xd800 && lastUnit <= 0xdbff) {
          cut -= 2;
        }
      }
      const complete = data.subarray(0, cut);
      this._pending = data.subarray(cut).slice();
      return this._decoder.decode(complete);
    }

    _writeGrouped(bytes, unit, decode) {
      let data = bytes;
      if (this._pending.length > 0) {
        data = new Uint8Array(this._pending.length + bytes.length);
        data.set(this._pending, 0);
        data.set(bytes, this._pending.length);
        this._pending = new Uint8Array(0);
      }
      const rem = data.length % unit;
      const cut = data.length - rem;
      this._pending = data.subarray(cut).slice();
      return decode(data.subarray(0, cut));
    }

    end(buf) {
      let out = "";
      if (buf !== undefined) {
        out = this.write(buf);
      }
      if (this._pending.length > 0) {
        if (this._mode === "utf8") {
          // Incomplete sequence at end: replacement character (Node behavior).
          out += "�";
        } else if (this.encoding === "hex") {
          out += this._Buffer.from(this._pending).toString("hex");
        } else if (this._mode === "group") {
          out += this._Buffer.from(this._pending).toString(this.encoding);
        } else if (this._mode === "pair") {
          // utf16le lone byte: Node emits the replacement via ucs2 decode of the buffered byte padded
          out += this._Buffer.from(this._pending).toString(this.encoding);
        }
        this._pending = new Uint8Array(0);
      }
      return out;
    }

    text(buf, offset) {
      this._pending = new Uint8Array(0);
      return this.write(buf.subarray(offset));
    }
  }

  function utf8SeqLength(lead) {
    if ((lead & 0x80) === 0) return 1;
    if ((lead & 0xe0) === 0xc0) return 2;
    if ((lead & 0xf0) === 0xe0) return 3;
    if ((lead & 0xf8) === 0xf0) return 4;
    return 1;
  }

  module.exports = { StringDecoder };
});
