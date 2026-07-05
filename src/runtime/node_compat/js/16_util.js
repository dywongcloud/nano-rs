"use strict";
// node:util, node:util/types, node:sys (Node v22 semantics).
__nanoNodeRegister("util", function (module, exports, require) {
  const { makeError, codes, UV_ERRNOS } = require("internal/errors");

  const customInspectSymbol = Symbol.for("nodejs.util.inspect.custom");
  const promisifyCustom = Symbol.for("nodejs.util.promisify.custom");

  // ---------------------------------------------------------------------
  // util/types — brand checks via Object.prototype.toString (realm-robust)
  // ---------------------------------------------------------------------
  const toStr = (v) => Object.prototype.toString.call(v);
  const types = {
    isAnyArrayBuffer: (v) => toStr(v) === "[object ArrayBuffer]" || toStr(v) === "[object SharedArrayBuffer]",
    isArrayBuffer: (v) => toStr(v) === "[object ArrayBuffer]",
    isSharedArrayBuffer: (v) => toStr(v) === "[object SharedArrayBuffer]",
    isArrayBufferView: (v) => ArrayBuffer.isView(v),
    isTypedArray: (v) => ArrayBuffer.isView(v) && toStr(v) !== "[object DataView]",
    isDataView: (v) => toStr(v) === "[object DataView]",
    isUint8Array: (v) => toStr(v) === "[object Uint8Array]",
    isUint8ClampedArray: (v) => toStr(v) === "[object Uint8ClampedArray]",
    isUint16Array: (v) => toStr(v) === "[object Uint16Array]",
    isUint32Array: (v) => toStr(v) === "[object Uint32Array]",
    isInt8Array: (v) => toStr(v) === "[object Int8Array]",
    isInt16Array: (v) => toStr(v) === "[object Int16Array]",
    isInt32Array: (v) => toStr(v) === "[object Int32Array]",
    isFloat16Array: (v) => toStr(v) === "[object Float16Array]",
    isFloat32Array: (v) => toStr(v) === "[object Float32Array]",
    isFloat64Array: (v) => toStr(v) === "[object Float64Array]",
    isBigInt64Array: (v) => toStr(v) === "[object BigInt64Array]",
    isBigUint64Array: (v) => toStr(v) === "[object BigUint64Array]",
    isDate: (v) => toStr(v) === "[object Date]",
    isRegExp: (v) => toStr(v) === "[object RegExp]",
    isMap: (v) => toStr(v) === "[object Map]",
    isSet: (v) => toStr(v) === "[object Set]",
    isWeakMap: (v) => toStr(v) === "[object WeakMap]",
    isWeakSet: (v) => toStr(v) === "[object WeakSet]",
    isPromise: (v) => toStr(v) === "[object Promise]",
    isProxy: () => false,
    isNativeError: (v) => v instanceof Error ||
      ["[object Error]", "[object DOMException]"].includes(toStr(v)),
    isBoxedPrimitive: (v) =>
      ["[object Number]", "[object String]", "[object Boolean]", "[object BigInt]", "[object Symbol]"].includes(toStr(v)) &&
      typeof v === "object",
    isNumberObject: (v) => typeof v === "object" && toStr(v) === "[object Number]",
    isStringObject: (v) => typeof v === "object" && toStr(v) === "[object String]",
    isBooleanObject: (v) => typeof v === "object" && toStr(v) === "[object Boolean]",
    isBigIntObject: (v) => typeof v === "object" && toStr(v) === "[object BigInt]",
    isSymbolObject: (v) => typeof v === "object" && toStr(v) === "[object Symbol]",
    isGeneratorFunction: (v) => typeof v === "function" && toStr(v) === "[object GeneratorFunction]",
    isAsyncFunction: (v) => typeof v === "function" && toStr(v) === "[object AsyncFunction]",
    isGeneratorObject: (v) => toStr(v) === "[object Generator]",
    isArgumentsObject: (v) => toStr(v) === "[object Arguments]",
    isMapIterator: (v) => toStr(v) === "[object Map Iterator]",
    isSetIterator: (v) => toStr(v) === "[object Set Iterator]",
    isModuleNamespaceObject: (v) => toStr(v) === "[object Module]",
    isExternal: () => false,
    isKeyObject: (v) => v !== null && typeof v === "object" && v.constructor && v.constructor.name === "KeyObject",
    isCryptoKey: (v) => toStr(v) === "[object CryptoKey]" ||
      (v !== null && typeof v === "object" && v.constructor && v.constructor.name === "CryptoKey"),
  };

  // ---------------------------------------------------------------------
  // inspect
  // ---------------------------------------------------------------------
  const inspectDefaults = {
    showHidden: false,
    depth: 2,
    colors: false,
    customInspect: true,
    showProxy: false,
    maxArrayLength: 100,
    maxStringLength: 10000,
    breakLength: 128,
    compact: 3,
    sorted: false,
    getters: false,
    numericSeparator: false,
  };

  function inspect(value, optsOrShowHidden, depthArg, colorsArg) {
    let opts = { ...inspectDefaults };
    if (typeof optsOrShowHidden === "boolean") {
      opts.showHidden = optsOrShowHidden;
      if (depthArg !== undefined) opts.depth = depthArg;
      if (colorsArg !== undefined) opts.colors = colorsArg;
    } else if (optsOrShowHidden !== null && typeof optsOrShowHidden === "object") {
      opts = { ...opts, ...optsOrShowHidden };
    }
    const seen = new Map();
    let circularIdx = 0;
    return fmtValue(value, opts.depth, opts, seen, () => { circularIdx += 1; return circularIdx; }, false);
  }
  inspect.custom = customInspectSymbol;
  inspect.defaultOptions = inspectDefaults;
  inspect.colors = {
    reset: [0, 0], bold: [1, 22], dim: [2, 22], italic: [3, 23], underline: [4, 24],
    blink: [5, 25], inverse: [7, 27], hidden: [8, 28], strikethrough: [9, 29],
    black: [30, 39], red: [31, 39], green: [32, 39], yellow: [33, 39], blue: [34, 39],
    magenta: [35, 39], cyan: [36, 39], white: [37, 39], gray: [90, 39], grey: [90, 39],
  };
  inspect.styles = {
    special: "cyan", number: "yellow", bigint: "yellow", boolean: "yellow",
    undefined: "grey", null: "bold", string: "green", symbol: "green",
    date: "magenta", regexp: "red", module: "underline",
  };

  function quoteString(str, opts) {
    let limit = opts.maxStringLength;
    let trailer = "";
    if (limit !== null && limit !== Infinity && str.length > limit) {
      const removed = str.length - limit;
      str = str.slice(0, limit);
      trailer = "... " + removed + " more character" + (removed > 1 ? "s" : "");
    }
    let quoted;
    if (!str.includes("'")) {
      quoted = "'" + str + "'";
    } else if (!str.includes('"')) {
      quoted = '"' + str + '"';
    } else {
      quoted = "`" + str + "`";
    }
    quoted = quoted
      .replace(/\\/g, "\\\\")
      .replace(/\n/g, "\\n")
      .replace(/\r/g, "\\r")
      .replace(/\t/g, "\\t")
      // eslint-disable-next-line no-control-regex
      .replace(/[\x00-\x08\x0b\x0c\x0e-\x1f]/g, (c) => "\\x" + c.charCodeAt(0).toString(16).padStart(2, "0"));
    return quoted + trailer;
  }

  function keyDisplay(key) {
    if (typeof key === "symbol") {
      return "[" + String(key) + "]";
    }
    if (/^[A-Za-z_$][A-Za-z0-9_$]*$/.test(key)) {
      return key;
    }
    return "'" + key.replace(/'/g, "\\'") + "'";
  }

  function ctorName(value) {
    const proto = Object.getPrototypeOf(value);
    if (proto === null) {
      return "[Object: null prototype]";
    }
    const c = proto.constructor;
    return c && c.name && c.name !== "Object" ? c.name : "";
  }

  function fmtValue(value, depth, opts, seen, nextCircular, insideCollection) {
    switch (typeof value) {
      case "string": return insideCollection || true === false ? quoteString(value, opts) : quoteString(value, opts);
      case "number": {
        if (Object.is(value, -0)) return "-0";
        return String(value);
      }
      case "bigint": return String(value) + "n";
      case "boolean": return String(value);
      case "undefined": return "undefined";
      case "symbol": return String(value);
      case "function": {
        const name = value.name;
        const kind = toStr(value) === "[object AsyncFunction]" ? "AsyncFunction"
          : toStr(value) === "[object GeneratorFunction]" ? "GeneratorFunction"
          : /^class[\s{]/.test(Function.prototype.toString.call(value)) ? "class" : "Function";
        if (kind === "class") {
          return "[class " + (name || "(anonymous)") + "]";
        }
        return name ? "[" + kind + ": " + name + "]" : "[" + kind + " (anonymous)]";
      }
      default:
        break;
    }
    if (value === null) {
      return "null";
    }

    if (seen.has(value)) {
      let ref = seen.get(value);
      if (ref === 0) {
        ref = nextCircular();
        seen.set(value, ref);
      }
      return "[Circular *" + ref + "]";
    }

    if (opts.customInspect && typeof value[customInspectSymbol] === "function" &&
        value[customInspectSymbol] !== inspect) {
      const custom = value[customInspectSymbol](depth, { ...opts, depth }, inspect);
      return typeof custom === "string" ? custom : fmtValue(custom, depth, opts, seen, nextCircular, false);
    }

    if (types.isDate(value)) {
      return Number.isNaN(value.getTime()) ? "Invalid Date" : value.toISOString();
    }
    if (types.isRegExp(value)) {
      return String(value);
    }
    if (value instanceof Error || types.isNativeError(value)) {
      const stack = value.stack;
      return stack ? String(stack) : "[" + String(value) + "]";
    }

    if (depth !== null && depth < 0) {
      if (Array.isArray(value)) return "[Array]";
      const cn = ctorName(value);
      return "[" + (cn && cn !== "[Object: null prototype]" ? cn : "Object") + "]";
    }

    seen.set(value, 0);
    let result;
    try {
      result = fmtObject(value, depth, opts, seen, nextCircular);
    } finally {
      const ref = seen.get(value);
      if (ref === 0) {
        seen.delete(value);
      } else {
        result = "<ref *" + ref + "> " + result;
        seen.delete(value);
      }
    }
    return result;
  }

  function joinEntries(entries, prefix, open, close, opts, groupValue) {
    if (entries.length === 0) {
      return prefix + (open === "[" ? "[]" : open + close);
    }
    if (groupValue !== undefined && entries.length > 6) {
      entries = groupArrayElements(entries, groupValue, opts);
    }
    const oneLine = prefix + open + " " + entries.join(", ") + " " + close;
    if (oneLine.length <= opts.breakLength && !oneLine.includes("\n")) {
      return oneLine;
    }
    const indented = entries.map((e) => "  " + e.split("\n").join("\n  ")).join(",\n");
    return prefix + open + "\n" + indented + "\n" + close;
  }

  // Port of Node's groupArrayElements: columnizes short array entries.
  function groupArrayElements(output, value, opts) {
    let totalLength = 0;
    let maxLength = 0;
    let i = 0;
    let outputLength = output.length;
    const max = opts.maxArrayLength === null ? Infinity : opts.maxArrayLength;
    if (max < (value ? value.length : output.length)) {
      outputLength = output.length - 1; // last entry is "... more items"
    }
    const separatorSpace = 2;
    const dataLen = new Array(outputLength);
    for (; i < outputLength; i++) {
      const len = output[i].length;
      dataLen[i] = len;
      totalLength += len + separatorSpace;
      if (maxLength < len) maxLength = len;
    }
    const actualMax = maxLength + separatorSpace;
    if (actualMax * 3 + 0 < opts.breakLength &&
        (totalLength / actualMax > 5 || maxLength <= 6)) {
      const approxCharHeights = 2.5;
      const averageBias = Math.sqrt(actualMax - totalLength / output.length);
      const biasedMax = Math.max(actualMax - 3 - averageBias, 1);
      const columns = Math.min(
        Math.round(Math.sqrt(approxCharHeights * biasedMax * outputLength) / biasedMax),
        Math.floor((opts.breakLength - 0) / actualMax),
        (typeof opts.compact === "number" ? opts.compact : 3) * 4,
        15
      );
      if (columns <= 1) {
        return output;
      }
      const tmp = [];
      const maxLineLength = [];
      for (let c = 0; c < columns; c++) {
        let lineLength = 0;
        for (let j = c; j < output.length; j += columns) {
          if (dataLen[j] > lineLength) {
            lineLength = dataLen[j];
          }
        }
        maxLineLength[c] = lineLength + separatorSpace;
      }
      let order = String.prototype.padStart;
      if (value !== undefined && value !== null) {
        for (let j = 0; j < output.length; j++) {
          if (typeof value[j] !== "number" && typeof value[j] !== "bigint") {
            order = String.prototype.padEnd;
            break;
          }
        }
      }
      for (let row = 0; row < outputLength; row += columns) {
        const lineMax = Math.min(row + columns, outputLength);
        let str = "";
        let j = row;
        for (; j < lineMax - 1; j++) {
          const padding = maxLineLength[j - row] + output[j].length - dataLen[j];
          str += order.call(output[j] + ", ", padding, " ");
        }
        if (order === String.prototype.padStart) {
          const padding = maxLineLength[j - row] + output[j].length - dataLen[j] - separatorSpace;
          str += output[j].padStart(padding, " ");
        } else {
          str += output[j];
        }
        tmp.push(str);
      }
      if (max < output.length && outputLength < output.length) {
        tmp.push(output[outputLength]);
      }
      output = tmp;
    }
    return output;
  }

  function fmtObject(value, depth, opts, seen, nextCircular) {
    const nextDepth = depth === null ? null : depth - 1;
    const fv = (v) => fmtValue(v, nextDepth, opts, seen, nextCircular, true);

    if (Array.isArray(value)) {
      const entries = [];
      const max = opts.maxArrayLength === null ? Infinity : opts.maxArrayLength;
      let lastIndex = -1;
      let holeRun = 0;
      const flushHoles = () => {
        if (holeRun > 0) {
          entries.push("<" + holeRun + " empty item" + (holeRun > 1 ? "s" : "") + ">");
          holeRun = 0;
        }
      };
      const len = Math.min(value.length, max);
      for (let i = 0; i < len; i += 1) {
        if (!(i in value)) {
          holeRun += 1;
          continue;
        }
        flushHoles();
        entries.push(fv(value[i]));
        lastIndex = i;
      }
      flushHoles();
      if (value.length > max) {
        entries.push("... " + (value.length - max) + " more item" + (value.length - max > 1 ? "s" : ""));
      }
      for (const key of Object.keys(value)) {
        if (String(Number(key)) === key && Number(key) >= 0 && Number(key) < value.length) continue;
        entries.push(keyDisplay(key) + ": " + fv(value[key]));
      }
      const cn = ctorName(value);
      const prefix = cn && cn !== "Array" ? cn + "(" + value.length + ") " : "";
      return joinEntries(entries, prefix, "[", "]", opts, value);
    }

    if (types.isMap(value)) {
      const entries = [];
      for (const [k, v] of value) {
        entries.push(fv(k) + " => " + fv(v));
      }
      return joinEntries(entries, "Map(" + value.size + ") ", "{", "}", opts);
    }

    if (types.isSet(value)) {
      const entries = [];
      for (const v of value) {
        entries.push(fv(v));
      }
      return joinEntries(entries, "Set(" + value.size + ") ", "{", "}", opts);
    }

    if (types.isTypedArray(value)) {
      const entries = [];
      const max = opts.maxArrayLength === null ? Infinity : opts.maxArrayLength;
      const len = Math.min(value.length, max);
      const isBig = types.isBigInt64Array(value) || types.isBigUint64Array(value);
      for (let i = 0; i < len; i += 1) {
        entries.push(String(value[i]) + (isBig ? "n" : ""));
      }
      if (value.length > max) {
        entries.push("... " + (value.length - max) + " more item" + (value.length - max > 1 ? "s" : ""));
      }
      const name = value.constructor && value.constructor.name ? value.constructor.name : toStr(value).slice(8, -1);
      return joinEntries(entries, name + "(" + value.length + ") ", "[", "]", opts, value);
    }

    if (types.isArrayBuffer(value)) {
      const view = new Uint8Array(value, 0, Math.min(value.byteLength, opts.maxArrayLength || 100));
      let hex = "";
      for (let i = 0; i < view.length; i += 1) {
        hex += (i > 0 ? " " : "") + view[i].toString(16).padStart(2, "0");
      }
      const more = value.byteLength > view.length ? " ... " + (value.byteLength - view.length) + " more bytes" : "";
      return "ArrayBuffer { [Uint8Contents]: <" + hex + more + ">, byteLength: " + value.byteLength + " }";
    }

    if (types.isPromise(value)) {
      return "Promise { <state unknown> }";
    }
    if (types.isWeakMap(value)) {
      return "WeakMap { <items unknown> }";
    }
    if (types.isWeakSet(value)) {
      return "WeakSet { <items unknown> }";
    }
    if (types.isBoxedPrimitive(value)) {
      const inner = value.valueOf();
      const name = toStr(value).slice(8, -1);
      return "[" + name + ": " + fmtValue(inner, nextDepth, opts, seen, nextCircular, true) + "]";
    }

    // Plain / class instance object
    const keys = opts.showHidden ? Object.getOwnPropertyNames(value) : Object.keys(value);
    const symbols = Object.getOwnPropertySymbols(value).filter((s) =>
      opts.showHidden || Object.getOwnPropertyDescriptor(value, s).enumerable
    );
    let allKeys = [...keys, ...symbols];
    if (opts.sorted) {
      allKeys = allKeys.sort((a, b) => String(a).localeCompare(String(b)));
    }
    const entries = [];
    for (const key of allKeys) {
      const desc = Object.getOwnPropertyDescriptor(value, key);
      if (desc && (desc.get || desc.set)) {
        const label = desc.get && desc.set ? "[Getter/Setter]" : desc.get ? "[Getter]" : "[Setter]";
        entries.push(keyDisplay(key) + ": " + label);
      } else {
        entries.push(keyDisplay(key) + ": " + fv(value[key]));
      }
    }
    let cn = ctorName(value);
    let prefix = "";
    if (cn === "[Object: null prototype]") {
      prefix = "[Object: null prototype] ";
    } else if (cn) {
      prefix = cn + " ";
    }
    return joinEntries(entries, prefix, "{", "}", opts);
  }

  // ---------------------------------------------------------------------
  // format
  // ---------------------------------------------------------------------
  function formatWithOptions(inspectOptions, ...args) {
    let fmt = args[0];
    let out = "";
    let i = 1;
    if (typeof fmt === "string" && args.length === 1) {
      return fmt;
    }
    if (typeof fmt === "string" && fmt.includes("%")) {
      let lastPos = 0;
      for (let pos = 0; pos < fmt.length - 1; pos += 1) {
        if (fmt[pos] !== "%") continue;
        const spec = fmt[pos + 1];
        let converted;
        switch (spec) {
          case "s": {
            if (i >= args.length) { pos += 1; continue; }
            const a = args[i];
            if (typeof a === "string") converted = a;
            else if (typeof a === "bigint") converted = a + "n";
            else if (a === null || typeof a !== "object") converted = String(a);
            else converted = inspect(a, { ...inspectOptions, depth: 2 });
            i += 1;
            break;
          }
          case "d": {
            if (i >= args.length) { pos += 1; continue; }
            const a = args[i];
            converted = typeof a === "bigint" ? a + "n"
              : typeof a === "symbol" ? "NaN"
              : typeof a === "object" && a !== null ? "NaN"
              : String(Number(a));
            i += 1;
            break;
          }
          case "i": {
            if (i >= args.length) { pos += 1; continue; }
            const a = args[i];
            converted = typeof a === "bigint" ? a + "n"
              : typeof a === "symbol" ? "NaN"
              : typeof a === "object" && a !== null ? "NaN"
              : String(parseInt(a, 10));
            i += 1;
            break;
          }
          case "f": {
            if (i >= args.length) { pos += 1; continue; }
            const a = args[i];
            converted = typeof a === "symbol" ? "NaN" : String(parseFloat(a));
            i += 1;
            break;
          }
          case "j": {
            if (i >= args.length) { pos += 1; continue; }
            try {
              converted = JSON.stringify(args[i]);
            } catch (_e) {
              converted = "[Circular]";
            }
            i += 1;
            break;
          }
          case "o": {
            if (i >= args.length) { pos += 1; continue; }
            converted = inspect(args[i], { ...inspectOptions, showHidden: true, depth: 4 });
            i += 1;
            break;
          }
          case "O": {
            if (i >= args.length) { pos += 1; continue; }
            converted = inspect(args[i], inspectOptions);
            i += 1;
            break;
          }
          case "c": {
            if (i >= args.length) { pos += 1; continue; }
            i += 1;
            converted = "";
            break;
          }
          case "%":
            out += fmt.slice(lastPos, pos) + "%";
            lastPos = pos + 2;
            pos += 1;
            continue;
          default:
            continue;
        }
        out += fmt.slice(lastPos, pos) + converted;
        lastPos = pos + 2;
        pos += 1;
      }
      out += fmt.slice(lastPos);
    } else if (fmt !== undefined || args.length > 0) {
      if (args.length > 0) {
        out = typeof fmt === "string" ? fmt : inspect(fmt, inspectOptions);
      }
    }
    for (; i < args.length; i += 1) {
      const a = args[i];
      out += (out.length > 0 ? " " : "") + (typeof a === "string" ? a : inspect(a, inspectOptions));
    }
    return out;
  }

  function format(...args) {
    return formatWithOptions({}, ...args);
  }

  // ---------------------------------------------------------------------
  // promisify / callbackify / misc
  // ---------------------------------------------------------------------
  function promisify(original) {
    if (typeof original !== "function") {
      throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", 'The "original" argument must be of type function');
    }
    if (original[promisifyCustom]) {
      const fn = original[promisifyCustom];
      if (typeof fn !== "function") {
        throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", "The [util.promisify.custom] property must be of type function");
      }
      return Object.defineProperty(fn, promisifyCustom, { value: fn, enumerable: false, writable: false, configurable: true });
    }
    const argumentNames = original[Symbol.for("nodejs.util.promisify.customArgs")];
    function fn(...args) {
      return new Promise((resolve, reject) => {
        args.push((err, ...values) => {
          if (err) {
            reject(err);
          } else if (argumentNames !== undefined && values.length > 1) {
            const obj = {};
            for (let k = 0; k < argumentNames.length; k += 1) {
              obj[argumentNames[k]] = values[k];
            }
            resolve(obj);
          } else {
            resolve(values[0]);
          }
        });
        Reflect.apply(original, this, args);
      });
    }
    Object.setPrototypeOf(fn, Object.getPrototypeOf(original));
    return Object.defineProperty(fn, promisifyCustom, { value: fn, enumerable: false, writable: false, configurable: true });
  }
  promisify.custom = promisifyCustom;

  function callbackify(original) {
    if (typeof original !== "function") {
      throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", 'The "original" argument must be of type function');
    }
    function callbackified(...args) {
      const maybeCb = args.pop();
      if (typeof maybeCb !== "function") {
        throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", "The last argument must be of type function");
      }
      const cb = (...cbArgs) => Reflect.apply(maybeCb, this, cbArgs);
      Reflect.apply(original, this, args).then(
        (ret) => queueMicrotask(() => cb(null, ret)),
        (rej) => queueMicrotask(() => {
          if (!rej) {
            const wrapped = makeError(Error, "ERR_FALSY_VALUE_REJECTION", "Promise was rejected with falsy value");
            wrapped.reason = rej;
            cb(wrapped);
          } else {
            cb(rej);
          }
        })
      );
    }
    Object.setPrototypeOf(callbackified, Object.getPrototypeOf(original));
    Object.defineProperties(callbackified, Object.getOwnPropertyDescriptors(original));
    return callbackified;
  }

  function inherits(ctor, superCtor) {
    if (ctor === undefined || ctor === null) {
      throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", 'The "ctor" argument must be of type function');
    }
    if (superCtor === undefined || superCtor === null) {
      throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", 'The "superCtor" argument must be of type function');
    }
    if (superCtor.prototype === undefined) {
      throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", 'The "superCtor.prototype" must be an object');
    }
    Object.defineProperty(ctor, "super_", { value: superCtor, writable: true, configurable: true });
    Object.setPrototypeOf(ctor.prototype, superCtor.prototype);
  }

  function deprecate(fn, msg, code) {
    let warned = false;
    function deprecated(...args) {
      if (!warned) {
        warned = true;
        const warning = code ? "[" + code + "] DeprecationWarning: " + msg : "DeprecationWarning: " + msg;
        if (typeof globalThis.process === "object" && globalThis.process !== null &&
            typeof globalThis.process.emitWarning === "function") {
          globalThis.process.emitWarning(msg, "DeprecationWarning", code);
        } else {
          console.warn(warning);
        }
      }
      if (new.target) {
        return Reflect.construct(fn, args, new.target);
      }
      return Reflect.apply(fn, this, args);
    }
    return deprecated;
  }

  function debuglog(section, cb) {
    let enabled = null;
    const test = () => {
      if (enabled === null) {
        const env = (typeof globalThis.process === "object" && globalThis.process !== null &&
          globalThis.process.env && globalThis.process.env.NODE_DEBUG) || "";
        enabled = env
          .split(",")
          .some((part) => part.trim().toLowerCase() === section.toLowerCase() ||
            new RegExp("^" + part.trim().replace(/[*]/g, ".*") + "$", "i").test(section));
      }
      return enabled;
    };
    const logger = (...args) => {
      if (test()) {
        const pid = (globalThis.process && globalThis.process.pid) || 0;
        console.error(section.toUpperCase() + " " + pid + ": " + format(...args));
      }
    };
    Object.defineProperty(logger, "enabled", { get: test });
    if (typeof cb === "function") {
      cb(logger);
    }
    return logger;
  }

  // ---------------------------------------------------------------------
  // isDeepStrictEqual
  // ---------------------------------------------------------------------
  function isDeepStrictEqual(a, b) {
    return deepEqual(a, b, new Map());
  }

  function deepEqual(a, b, memo) {
    if (Object.is(a, b)) {
      return true;
    }
    if (typeof a !== typeof b) {
      return false;
    }
    if (typeof a === "number") {
      return false; // Object.is covered NaN/-0 already
    }
    if (typeof a !== "object" || a === null || b === null) {
      return false;
    }
    if (Object.getPrototypeOf(a) !== Object.getPrototypeOf(b)) {
      return false;
    }

    const seen = memo.get(a);
    if (seen !== undefined && seen === b) {
      return true;
    }
    memo.set(a, b);

    try {
      if (types.isDate(a)) {
        return Object.is(a.getTime(), b.getTime());
      }
      if (types.isRegExp(a)) {
        return a.source === b.source && a.flags === b.flags;
      }
      if (types.isBoxedPrimitive(a)) {
        return Object.is(a.valueOf(), b.valueOf());
      }
      if (Array.isArray(a)) {
        if (a.length !== b.length) return false;
        for (let i = 0; i < a.length; i += 1) {
          const aHas = i in a;
          const bHas = i in b;
          if (aHas !== bHas) return false;
          if (aHas && !deepEqual(a[i], b[i], memo)) return false;
        }
        return ownPropsEqual(a, b, memo, true);
      }
      if (types.isTypedArray(a) || types.isDataView(a)) {
        if (a.byteLength !== b.byteLength) return false;
        const ua = new Uint8Array(a.buffer, a.byteOffset, a.byteLength);
        const ub = new Uint8Array(b.buffer, b.byteOffset, b.byteLength);
        for (let i = 0; i < ua.length; i += 1) {
          if (ua[i] !== ub[i]) return false;
        }
        return true;
      }
      if (types.isArrayBuffer(a)) {
        if (a.byteLength !== b.byteLength) return false;
        const ua = new Uint8Array(a);
        const ub = new Uint8Array(b);
        for (let i = 0; i < ua.length; i += 1) {
          if (ua[i] !== ub[i]) return false;
        }
        return true;
      }
      if (types.isMap(a)) {
        if (a.size !== b.size) return false;
        return mapEquiv(a, b, memo);
      }
      if (types.isSet(a)) {
        if (a.size !== b.size) return false;
        return setEquiv(a, b, memo);
      }
      if (a instanceof Error) {
        if (a.message !== b.message || a.name !== b.name) return false;
      }
      return ownPropsEqual(a, b, memo, false);
    } finally {
      memo.delete(a);
    }
  }

  function ownPropsEqual(a, b, memo, skipIndices) {
    const aKeys = Object.keys(a).filter((k) => !skipIndices || String(Number(k)) !== k);
    const bKeys = Object.keys(b).filter((k) => !skipIndices || String(Number(k)) !== k);
    if (aKeys.length !== bKeys.length) return false;
    for (const k of aKeys) {
      if (!Object.prototype.hasOwnProperty.call(b, k)) return false;
      if (!deepEqual(a[k], b[k], memo)) return false;
    }
    const aSyms = Object.getOwnPropertySymbols(a).filter((s) => Object.getOwnPropertyDescriptor(a, s).enumerable);
    const bSyms = Object.getOwnPropertySymbols(b).filter((s) => Object.getOwnPropertyDescriptor(b, s).enumerable);
    if (aSyms.length !== bSyms.length) return false;
    for (const s of aSyms) {
      if (!Object.prototype.hasOwnProperty.call(b, s)) return false;
      if (!deepEqual(a[s], b[s], memo)) return false;
    }
    return true;
  }

  function mapEquiv(a, b, memo) {
    const bEntries = [...b.entries()];
    const used = new Set();
    for (const [ak, av] of a) {
      let found = false;
      for (let i = 0; i < bEntries.length; i += 1) {
        if (used.has(i)) continue;
        const [bk, bv] = bEntries[i];
        if (deepEqual(ak, bk, memo) && deepEqual(av, bv, memo)) {
          used.add(i);
          found = true;
          break;
        }
      }
      if (!found) return false;
    }
    return true;
  }

  function setEquiv(a, b, memo) {
    const bItems = [...b];
    const used = new Set();
    for (const av of a) {
      let found = false;
      for (let i = 0; i < bItems.length; i += 1) {
        if (used.has(i)) continue;
        if (deepEqual(av, bItems[i], memo)) {
          used.add(i);
          found = true;
          break;
        }
      }
      if (!found) return false;
    }
    return true;
  }

  // ---------------------------------------------------------------------
  // parseArgs
  // ---------------------------------------------------------------------
  function parseArgs(config = {}) {
    const argv = config.args !== undefined
      ? config.args
      : (globalThis.process && globalThis.process.argv ? globalThis.process.argv.slice(2) : []);
    const options = config.options || {};
    const strict = config.strict !== false;
    const allowPositionals = config.allowPositionals !== undefined ? config.allowPositionals : !strict;
    const returnTokens = !!config.tokens;

    const values = { __proto__: null };
    const positionals = [];
    const tokens = [];

    const shortMap = new Map();
    for (const [name, opt] of Object.entries(options)) {
      if (opt.short) {
        shortMap.set(opt.short, name);
      }
      if (opt.default !== undefined && values[name] === undefined) {
        values[name] = opt.default;
      }
    }

    const unknownOption = (raw) => {
      throw makeError(TypeError, "ERR_PARSE_ARGS_UNKNOWN_OPTION", "Unknown option '" + raw + "'");
    };

    let i = 0;
    let index = -1;
    while (i < argv.length) {
      const arg = argv[i];
      index += 1;
      i += 1;

      if (arg === "--") {
        tokens.push({ kind: "option-terminator", index });
        for (; i < argv.length; i += 1) {
          index += 1;
          tokens.push({ kind: "positional", index, value: argv[i] });
          positionals.push(argv[i]);
        }
        break;
      }

      if (arg.startsWith("--")) {
        let name = arg.slice(2);
        let inlineValue;
        let raw = arg;
        const eqIdx = name.indexOf("=");
        if (eqIdx !== -1) {
          inlineValue = name.slice(eqIdx + 1);
          name = name.slice(0, eqIdx);
        }
        const opt = options[name];
        if (opt === undefined) {
          if (strict) unknownOption(arg);
          setValue(values, name, inlineValue !== undefined ? inlineValue : true, undefined);
          tokens.push({ kind: "option", name, rawName: "--" + name, index, value: inlineValue, inlineValue: inlineValue !== undefined });
          continue;
        }
        if (opt.type === "boolean") {
          if (inlineValue !== undefined && strict) {
            throw makeError(TypeError, "ERR_PARSE_ARGS_INVALID_OPTION_VALUE",
              "Option '--" + name + "' does not take an argument");
          }
          setValue(values, name, true, opt);
          tokens.push({ kind: "option", name, rawName: "--" + name, index, value: undefined, inlineValue: undefined });
        } else {
          let v = inlineValue;
          if (v === undefined) {
            if (i >= argv.length) {
              if (strict) {
                throw makeError(TypeError, "ERR_PARSE_ARGS_INVALID_OPTION_VALUE",
                  "Option '--" + name + " <value>' argument missing");
              }
              v = undefined;
            } else {
              v = argv[i];
              i += 1;
              index += 1;
            }
          }
          setValue(values, name, v, opt);
          tokens.push({ kind: "option", name, rawName: raw.split("=")[0], index: index - (inlineValue === undefined && v !== undefined ? 1 : 0), value: v, inlineValue: inlineValue !== undefined });
        }
        continue;
      }

      if (arg.startsWith("-") && arg !== "-") {
        const group = arg.slice(1);
        let consumed = false;
        for (let gi = 0; gi < group.length; gi += 1) {
          const short = group[gi];
          const name = shortMap.get(short) || short;
          const opt = options[name];
          if (opt === undefined) {
            if (strict) unknownOption("-" + short);
            setValue(values, name, true, undefined);
            tokens.push({ kind: "option", name, rawName: "-" + short, index, value: undefined, inlineValue: undefined });
            continue;
          }
          if (opt.type === "boolean") {
            setValue(values, name, true, opt);
            tokens.push({ kind: "option", name, rawName: "-" + short, index, value: undefined, inlineValue: undefined });
          } else {
            let v;
            if (gi + 1 < group.length) {
              v = group.slice(gi + 1);
              consumed = true;
            } else if (i < argv.length) {
              v = argv[i];
              i += 1;
              index += 1;
            } else if (strict) {
              throw makeError(TypeError, "ERR_PARSE_ARGS_INVALID_OPTION_VALUE",
                "Option '-" + short + " <value>' argument missing");
            }
            setValue(values, name, v, opt);
            tokens.push({ kind: "option", name, rawName: "-" + short, index, value: v, inlineValue: gi + 1 < group.length ? true : undefined });
            if (consumed) break;
          }
        }
        continue;
      }

      if (strict && !allowPositionals) {
        throw makeError(TypeError, "ERR_PARSE_ARGS_UNEXPECTED_POSITIONAL",
          "Unexpected argument '" + arg + "'. This command does not take positional arguments");
      }
      positionals.push(arg);
      tokens.push({ kind: "positional", index, value: arg });
    }

    const result = { values, positionals };
    if (returnTokens) {
      result.tokens = tokens;
    }
    return result;
  }

  function setValue(values, name, value, opt) {
    if (opt && opt.multiple) {
      if (!Array.isArray(values[name]) || values[name] === (opt && opt.default)) {
        values[name] = [];
      }
      values[name].push(value);
    } else {
      values[name] = value;
    }
  }

  // ---------------------------------------------------------------------
  // Misc small APIs
  // ---------------------------------------------------------------------
  function toUSVString(input) {
    return String(input).replace(/[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/g, "�");
  }

  // eslint-disable-next-line no-control-regex
  const vtRegex = /[][[\]()#;?]*(?:(?:(?:(?:;[-a-zA-Z\d\/#&.:=?%@~_]+)*|[a-zA-Z\d]+(?:;[-a-zA-Z\d\/#&.:=?%@~_]*)*)?(?:|\|))|(?:(?:\d{1,4}(?:;\d{0,4})*)?[\dA-PR-TZcf-nq-uy=><~]))/g;
  function stripVTControlCharacters(str) {
    if (typeof str !== "string") {
      throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", 'The "str" argument must be of type string');
    }
    return str.replace(vtRegex, "");
  }

  const SGR = {
    reset: [0, 0], bold: [1, 22], dim: [2, 22], italic: [3, 23], underline: [4, 24],
    blink: [5, 25], inverse: [7, 27], hidden: [8, 28], strikethrough: [9, 29],
    doubleunderline: [21, 24], black: [30, 39], red: [31, 39], green: [32, 39],
    yellow: [33, 39], blue: [34, 39], magenta: [35, 39], cyan: [36, 39], white: [37, 39],
    gray: [90, 39], grey: [90, 39], redBright: [91, 39], greenBright: [92, 39],
    yellowBright: [93, 39], blueBright: [94, 39], magentaBright: [95, 39],
    cyanBright: [96, 39], whiteBright: [97, 39], bgBlack: [40, 49], bgRed: [41, 49],
    bgGreen: [42, 49], bgYellow: [43, 49], bgBlue: [44, 49], bgMagenta: [45, 49],
    bgCyan: [46, 49], bgWhite: [47, 49], none: [0, 0], framed: [51, 54], overlined: [53, 55],
    blackBright: [90, 39],
  };
  function styleText(formatArg, text, options) {
    if (typeof text !== "string") {
      throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", 'The "text" argument must be of type string');
    }
    const validateStream = options === undefined || options.validateStream !== false;
    if (validateStream) {
      const stream = (options && options.stream) ||
        (globalThis.process && globalThis.process.stdout) || null;
      if (!stream || stream.isTTY !== true) {
        return text;
      }
    }
    const fmts = Array.isArray(formatArg) ? formatArg : [formatArg];
    let open = "";
    let close = "";
    for (const f of fmts) {
      const pair = SGR[f];
      if (pair === undefined) {
        throw makeError(TypeError, "ERR_INVALID_ARG_VALUE", "invalid format: " + f);
      }
      open += "[" + pair[0] + "m";
      close = "[" + pair[1] + "m" + close;
    }
    return open + text + close;
  }

  const errnoToName = {};
  for (const [name, num] of Object.entries(UV_ERRNOS)) {
    errnoToName[num] = name;
  }
  function getSystemErrorName(errno) {
    if (typeof errno !== "number") {
      throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", 'The "err" argument must be of type number');
    }
    return errnoToName[errno] || "Unknown system error " + errno;
  }
  function getSystemErrorMap() {
    const map = new Map();
    for (const [name, num] of Object.entries(UV_ERRNOS)) {
      map.set(num, [name, name.toLowerCase()]);
    }
    return map;
  }
  function getSystemErrorMessage(errno) {
    return getSystemErrorName(errno);
  }

  function aborted(signal, resource) {
    if (signal === undefined || typeof signal.addEventListener !== "function") {
      throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", 'The "signal" argument must be an instance of AbortSignal');
    }
    if (resource === null || (typeof resource !== "object" && typeof resource !== "function")) {
      throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", 'The "resource" argument must be an object');
    }
    if (signal.aborted) {
      return Promise.resolve();
    }
    return new Promise((resolve) => {
      signal.addEventListener("abort", () => resolve(), { once: true });
    });
  }

  // MIMEType / MIMEParams
  class MIMEParams {
    constructor() {
      this._map = new Map();
    }
    delete(name) {
      this._map.delete(String(name).toLowerCase());
    }
    get(name) {
      const v = this._map.get(String(name).toLowerCase());
      return v === undefined ? null : v;
    }
    has(name) {
      return this._map.has(String(name).toLowerCase());
    }
    set(name, value) {
      this._map.set(String(name).toLowerCase(), String(value));
    }
    entries() {
      return this._map.entries();
    }
    keys() {
      return this._map.keys();
    }
    values() {
      return this._map.values();
    }
    [Symbol.iterator]() {
      return this._map.entries();
    }
    toString() {
      const parts = [];
      for (const [k, v] of this._map) {
        const needsQuote = v === "" || /[^-!#$%&'*+.^_`|~A-Za-z0-9]/.test(v);
        parts.push(k + "=" + (needsQuote ? '"' + v.replace(/(["\\])/g, "\\$1") + '"' : v));
      }
      return parts.join(";");
    }
  }

  class MIMEType {
    constructor(input) {
      const str = String(input).trim();
      const slash = str.indexOf("/");
      if (slash === -1) {
        throw makeError(TypeError, "ERR_INVALID_MIME_SYNTAX", "The MIME syntax for a MIME type is invalid: " + input);
      }
      const semi = str.indexOf(";");
      const typePart = str.slice(0, slash).trim().toLowerCase();
      const subtypePart = (semi === -1 ? str.slice(slash + 1) : str.slice(slash + 1, semi)).trim().toLowerCase();
      if (!typePart || !subtypePart || /[^-!#$%&'*+.^_`|~A-Za-z0-9]/.test(typePart) || /[^-!#$%&'*+.^_`|~A-Za-z0-9]/.test(subtypePart)) {
        throw makeError(TypeError, "ERR_INVALID_MIME_SYNTAX", "The MIME syntax for a MIME type is invalid: " + input);
      }
      this._type = typePart;
      this._subtype = subtypePart;
      this._params = new MIMEParams();
      if (semi !== -1) {
        for (const kv of str.slice(semi + 1).split(";")) {
          const eqi = kv.indexOf("=");
          if (eqi === -1) continue;
          const k = kv.slice(0, eqi).trim().toLowerCase();
          let v = kv.slice(eqi + 1).trim();
          if (v.startsWith('"') && v.endsWith('"') && v.length >= 2) {
            v = v.slice(1, -1).replace(/\\(.)/g, "$1");
          }
          if (k && !this._params.has(k)) {
            this._params.set(k, v);
          }
        }
      }
    }
    get type() {
      return this._type;
    }
    set type(v) {
      this._type = String(v).toLowerCase();
    }
    get subtype() {
      return this._subtype;
    }
    set subtype(v) {
      this._subtype = String(v).toLowerCase();
    }
    get essence() {
      return this._type + "/" + this._subtype;
    }
    get params() {
      return this._params;
    }
    toString() {
      const p = this._params.toString();
      return this.essence + (p ? ";" + p : "");
    }
    toJSON() {
      return this.toString();
    }
  }

  function transferableAbortController() {
    return new AbortController();
  }
  function transferableAbortSignal(signal) {
    return signal;
  }

  function _extend(target, source) {
    if (source === null || typeof source !== "object") return target;
    for (const key of Object.keys(source)) {
      target[key] = source[key];
    }
    return target;
  }

  module.exports = {
    format,
    formatWithOptions,
    inspect,
    promisify,
    callbackify,
    inherits,
    deprecate,
    debuglog,
    debug: debuglog,
    isDeepStrictEqual,
    types,
    TextEncoder: globalThis.TextEncoder,
    TextDecoder: globalThis.TextDecoder,
    toUSVString,
    stripVTControlCharacters,
    styleText,
    getSystemErrorName,
    getSystemErrorMap,
    getSystemErrorMessage,
    parseArgs,
    aborted,
    MIMEType,
    MIMEParams,
    transferableAbortController,
    transferableAbortSignal,
    parseEnv(content) {
      const out = { __proto__: null };
      for (const line of String(content).split(/\r?\n/)) {
        const m = /^\s*(?:export\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*)\s*$/.exec(line);
        if (!m) continue;
        let v = m[2];
        if ((v.startsWith('"') && v.endsWith('"')) || (v.startsWith("'") && v.endsWith("'"))) {
          v = v.slice(1, -1);
        }
        out[m[1]] = v;
      }
      return out;
    },
    _extend: deprecate(_extend, "The `util._extend` API is deprecated. Please use Object.assign() instead.", "DEP0060"),
    isArray: deprecate(Array.isArray, "The `util.isArray` API is deprecated. Please use `Array.isArray()` instead.", "DEP0044"),
    isBoolean: (v) => typeof v === "boolean",
    isBuffer: (v) => globalThis.Buffer !== undefined && typeof globalThis.Buffer.isBuffer === "function" && globalThis.Buffer.isBuffer(v),
    isNull: (v) => v === null,
    isNullOrUndefined: (v) => v === null || v === undefined,
    isNumber: (v) => typeof v === "number",
    isString: (v) => typeof v === "string",
    isSymbol: (v) => typeof v === "symbol",
    isUndefined: (v) => v === undefined,
    isRegExp: types.isRegExp,
    isObject: (v) => v !== null && typeof v === "object",
    isDate: types.isDate,
    isError: (v) => types.isNativeError(v) || v instanceof Error,
    isFunction: (v) => typeof v === "function",
    isPrimitive: (v) => (typeof v !== "object" && typeof v !== "function") || v === null,
    log(...args) {
      console.log("%s - %s", new Date().toISOString().slice(11, 19), format(...args));
    },
    diff(actual, expected) {
      // Line-level LCS-free diff summary (Node's util.diff is experimental).
      const a = String(actual).split("\n");
      const b = String(expected).split("\n");
      const out = [];
      const max = Math.max(a.length, b.length);
      for (let i = 0; i < max; i += 1) {
        if (a[i] === b[i]) {
          out.push([0, a[i]]);
        } else {
          if (i < a.length) out.push([1, a[i]]);
          if (i < b.length) out.push([-1, b[i]]);
        }
      }
      return out;
    },
    setTraceSigInt() {},
    _errnoException(err, syscall, original) {
      const name = getSystemErrorName(err);
      const e = new Error(syscall + " " + name + (original ? " " + original : ""));
      e.errno = err;
      e.code = name;
      e.syscall = syscall;
      return e;
    },
    _exceptionWithHostPort(err, syscall, address, port, additional) {
      const name = getSystemErrorName(err);
      let details = "";
      if (port && port > 0) {
        details = address + ":" + port;
      } else if (address) {
        details = address;
      }
      if (additional) {
        details += " - Local (" + additional + ")";
      }
      const e = new Error(syscall + " " + name + " " + details);
      e.errno = err;
      e.code = name;
      e.syscall = syscall;
      e.address = address;
      if (port) e.port = port;
      return e;
    },
    inspectSymbol: customInspectSymbol,
  };
});

__nanoNodeRegister("util/types", function (module, exports, require) {
  module.exports = require("util").types;
});

__nanoNodeRegister("sys", function (module, exports, require) {
  module.exports = require("util");
});
