"use strict";
// node:querystring — legacy query string codec (Node v22 semantics).
__nanoNodeRegister("querystring", function (module, exports, require) {
  // Node's escape set: alphanumerics and - . _ ~ ! ' ( ) * remain unescaped
  // (matches encodeURIComponent minus nothing; Node uses its own table where
  // ! ' ( ) * are NOT escaped, same as encodeURIComponent).
  const noEscape = new Int8Array([
    // 0-127: 1 = no escape
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 1, 1, 0, // ! ' ( ) * - .
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, // 0-9
    0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // A-O
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 1, // P-Z _
    0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // a-o
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 0, // p-z ~
  ]);

  const hexTable = [];
  for (let i = 0; i < 256; i += 1) {
    hexTable[i] = "%" + ((i < 16 ? "0" : "") + i.toString(16)).toUpperCase();
  }

  function qsEscape(str) {
    if (typeof str !== "string") {
      if (typeof str === "object") {
        str = String(str);
      } else {
        str += "";
      }
    }
    let out = "";
    let lastPos = 0;
    for (let i = 0; i < str.length; i += 1) {
      let c = str.charCodeAt(i);
      if (c < 0x80 && noEscape[c] === 1) {
        continue;
      }
      if (i > lastPos) {
        out += str.slice(lastPos, i);
      }
      lastPos = i + 1;
      if (c < 0x80) {
        out += hexTable[c];
      } else if (c < 0x800) {
        out += hexTable[0xc0 | (c >> 6)] + hexTable[0x80 | (c & 0x3f)];
      } else if (c < 0xd800 || c >= 0xe000) {
        out += hexTable[0xe0 | (c >> 12)] + hexTable[0x80 | ((c >> 6) & 0x3f)] + hexTable[0x80 | (c & 0x3f)];
      } else {
        // Surrogate pair
        i += 1;
        if (i >= str.length) {
          const err = new URIError("URI malformed");
          throw err;
        }
        const c2 = str.charCodeAt(i) & 0x3ff;
        lastPos = i + 1;
        c = 0x10000 + (((c & 0x3ff) << 10) | c2);
        out += hexTable[0xf0 | (c >> 18)] + hexTable[0x80 | ((c >> 12) & 0x3f)] +
               hexTable[0x80 | ((c >> 6) & 0x3f)] + hexTable[0x80 | (c & 0x3f)];
      }
    }
    if (lastPos === 0) {
      return str;
    }
    if (lastPos < str.length) {
      out += str.slice(lastPos);
    }
    return out;
  }

  function unescapeBuffer(s, decodeSpaces) {
    // Tolerant percent-decoder (never throws, like Node).
    const out = [];
    let i = 0;
    while (i < s.length) {
      const c = s.charCodeAt(i);
      if (c === 43 && decodeSpaces) {
        out.push(32);
        i += 1;
      } else if (c === 37 && i + 2 < s.length + 1) {
        const hi = hexVal(s.charCodeAt(i + 1));
        const lo = hexVal(s.charCodeAt(i + 2));
        if (hi !== -1 && lo !== -1) {
          out.push((hi << 4) | lo);
          i += 3;
        } else {
          out.push(37);
          i += 1;
        }
      } else {
        // Encode this UTF-16 unit as UTF-8 bytes
        if (c < 0x80) {
          out.push(c);
          i += 1;
        } else {
          let cp = c;
          let consumed = 1;
          if (c >= 0xd800 && c < 0xdc00 && i + 1 < s.length) {
            const c2 = s.charCodeAt(i + 1);
            if (c2 >= 0xdc00 && c2 < 0xe000) {
              cp = 0x10000 + ((c - 0xd800) << 10) + (c2 - 0xdc00);
              consumed = 2;
            }
          }
          if (cp < 0x800) {
            out.push(0xc0 | (cp >> 6), 0x80 | (cp & 0x3f));
          } else if (cp < 0x10000) {
            out.push(0xe0 | (cp >> 12), 0x80 | ((cp >> 6) & 0x3f), 0x80 | (cp & 0x3f));
          } else {
            out.push(0xf0 | (cp >> 18), 0x80 | ((cp >> 12) & 0x3f), 0x80 | ((cp >> 6) & 0x3f), 0x80 | (cp & 0x3f));
          }
          i += consumed;
        }
      }
    }
    return new Uint8Array(out);
  }

  function hexVal(c) {
    if (c >= 48 && c <= 57) return c - 48;
    if (c >= 65 && c <= 70) return c - 55;
    if (c >= 97 && c <= 102) return c - 87;
    return -1;
  }

  const utf8Decoder = new TextDecoder("utf-8");

  function qsUnescape(s, decodeSpaces) {
    try {
      return decodeURIComponent(s);
    } catch (_e) {
      return utf8Decoder.decode(unescapeBuffer(s, decodeSpaces));
    }
  }

  function defaultDecode(s) {
    return utf8Decoder.decode(unescapeBuffer(s, true));
  }

  function stringifyPrimitive(v) {
    if (typeof v === "string") return v;
    if (typeof v === "number" && Number.isFinite(v)) return String(v);
    if (typeof v === "bigint") return String(v);
    if (typeof v === "boolean") return v ? "true" : "false";
    return "";
  }

  function stringify(obj, sep, eq, options) {
    sep = sep || "&";
    eq = eq || "=";
    const encode = options && typeof options.encodeURIComponent === "function"
      ? options.encodeURIComponent
      : qsEscape;

    if (obj === null || typeof obj !== "object") {
      return "";
    }
    const keys = Object.keys(obj);
    let out = "";
    for (let i = 0; i < keys.length; i += 1) {
      const k = keys[i];
      const v = obj[k];
      const ks = encode(stringifyPrimitive(k)) + eq;
      if (Array.isArray(v)) {
        for (let j = 0; j < v.length; j += 1) {
          if (out.length > 0) out += sep;
          out += ks + encode(stringifyPrimitive(v[j]));
        }
      } else {
        if (out.length > 0) out += sep;
        out += ks + encode(stringifyPrimitive(v));
      }
    }
    return out;
  }

  function parse(qs, sep, eq, options) {
    sep = sep || "&";
    eq = eq || "=";
    const obj = Object.create(null);

    if (typeof qs !== "string" || qs.length === 0) {
      return obj;
    }

    const maxKeys = options && typeof options.maxKeys === "number" ? options.maxKeys : 1000;
    const decode = options && typeof options.decodeURIComponent === "function"
      ? options.decodeURIComponent
      : defaultDecode;
    const customDecode = decode !== defaultDecode;

    const pairs = qs.split(sep);
    const limit = maxKeys > 0 ? Math.min(pairs.length, maxKeys) : pairs.length;

    for (let i = 0; i < limit; i += 1) {
      const pair = pairs[i];
      if (pair.length === 0) {
        continue;
      }
      const idx = pair.indexOf(eq);
      let k;
      let v;
      if (idx >= 0) {
        k = pair.slice(0, idx);
        v = pair.slice(idx + eq.length);
      } else {
        k = pair;
        v = "";
      }
      let key;
      let value;
      if (customDecode) {
        try {
          key = decode(k);
        } catch (_e) {
          key = k;
        }
        try {
          value = decode(v);
        } catch (_e) {
          value = v;
        }
      } else {
        key = k.includes("%") || k.includes("+") ? defaultDecode(k) : k;
        value = v.includes("%") || v.includes("+") ? defaultDecode(v) : v;
      }
      const existing = obj[key];
      if (existing === undefined) {
        obj[key] = value;
      } else if (Array.isArray(existing)) {
        existing.push(value);
      } else {
        obj[key] = [existing, value];
      }
    }
    return obj;
  }

  module.exports = {
    parse,
    decode: parse,
    stringify,
    encode: stringify,
    escape: qsEscape,
    unescape: (s) => qsUnescape(s, false),
    unescapeBuffer(s, decodeSpaces) {
      const { Buffer } = require("buffer");
      return Buffer.from(unescapeBuffer(s, !!decodeSpaces));
    },
  };
});
