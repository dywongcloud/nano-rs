"use strict";
// node:assert and node:assert/strict (Node v22 semantics).
__nanoNodeRegister("assert", function (module, exports, require) {
  const { makeError } = require("internal/errors");
  const util = require("util");

  class AssertionError extends Error {
    constructor(options = {}) {
      const { message, actual, expected, operator, stackStartFn } = options;
      let msg = message;
      if (msg === undefined || msg === null) {
        if (operator === "fail") {
          msg = "Failed";
        } else {
          const a = util.inspect(actual, { depth: 2, maxArrayLength: 30 });
          const e = util.inspect(expected, { depth: 2, maxArrayLength: 30 });
          switch (operator) {
            case "strictEqual":
              msg = "Expected values to be strictly equal:\n\n" + a + " !== " + e + "\n";
              break;
            case "notStrictEqual":
              msg = 'Expected "actual" to be strictly unequal to: ' + e + "\n";
              break;
            case "deepStrictEqual":
              msg = "Expected values to be strictly deep-equal:\n\nactual: " + a + "\nexpected: " + e + "\n";
              break;
            case "notDeepStrictEqual":
              msg = 'Expected "actual" not to be strictly deep-equal to: ' + e + "\n";
              break;
            case "==":
              msg = a + " == " + e;
              break;
            case "!=":
              msg = a + " != " + e;
              break;
            case "deepEqual":
              msg = "Expected values to be loosely deep-equal:\n\n" + a + "\n\nshould loosely deep-equal\n\n" + e;
              break;
            case "notDeepEqual":
              msg = 'Expected "actual" not to be loosely deep-equal to: ' + e;
              break;
            default:
              msg = a + " " + (operator || "") + " " + e;
          }
        }
      }
      super(String(msg));
      this.name = "AssertionError";
      this.code = "ERR_ASSERTION";
      this.actual = actual;
      this.expected = expected;
      this.operator = operator;
      this.generatedMessage = message === undefined || message === null;
      if (Error.captureStackTrace && typeof stackStartFn === "function") {
        Error.captureStackTrace(this, stackStartFn);
      }
    }
  }

  function innerFail(obj) {
    const m = obj.message;
    if (m instanceof Error ||
        (m !== null && typeof m === "object" &&
         typeof m.message === "string" && typeof m.stack === "string")) {
      throw m;
    }
    throw new AssertionError(obj);
  }

  function ok(value, message) {
    if (arguments.length === 0) {
      innerFail({ message: "No value argument passed to `assert.ok()`", actual: undefined, expected: true, operator: "==", stackStartFn: ok });
    }
    if (!value) {
      innerFail({ message, actual: value, expected: true, operator: "==", stackStartFn: ok });
    }
  }

  const assert = ok.bind(null);
  // Rebind: assert must be callable AND carry all members
  function assertFn(value, message) {
    if (arguments.length === 0) {
      innerFail({ message: "No value argument passed to `assert.ok()`", actual: undefined, expected: true, operator: "==", stackStartFn: assertFn });
    }
    if (!value) {
      innerFail({ message, actual: value, expected: true, operator: "==", stackStartFn: assertFn });
    }
  }

  // Loose deep equality (assert.deepEqual): == for primitives, structure otherwise
  function looseDeepEqual(a, b, memo = new Map()) {
    // eslint-disable-next-line eqeqeq
    if (a == b) {
      return true;
    }
    if (a === null || b === null || typeof a !== "object" || typeof b !== "object") {
      // NaN == NaN is false but loose deepEqual treats NaN as equal? Node: NaN !== NaN in deepEqual (legacy: NaN not equal)
      return false;
    }
    const seen = memo.get(a);
    if (seen === b) {
      return true;
    }
    memo.set(a, b);
    try {
      if (util.types.isDate(a) && util.types.isDate(b)) {
        return a.getTime() === b.getTime();
      }
      if (util.types.isRegExp(a) && util.types.isRegExp(b)) {
        return a.source === b.source && a.flags === b.flags;
      }
      if (Array.isArray(a) !== Array.isArray(b)) {
        return false;
      }
      const aKeys = Object.keys(a);
      const bKeys = Object.keys(b);
      if (aKeys.length !== bKeys.length) {
        return false;
      }
      for (const k of aKeys) {
        if (!Object.prototype.hasOwnProperty.call(b, k)) return false;
        if (!looseDeepEqual(a[k], b[k], memo)) return false;
      }
      return true;
    } finally {
      memo.delete(a);
    }
  }

  assertFn.AssertionError = AssertionError;

  assertFn.ok = assertFn;

  assertFn.fail = function fail(message) {
    innerFail({ message: message === undefined ? "Failed" : message, operator: "fail", stackStartFn: fail });
  };

  assertFn.equal = function equal(actual, expected, message) {
    // eslint-disable-next-line eqeqeq
    if (actual != expected && !(Number.isNaN(actual) && Number.isNaN(expected))) {
      innerFail({ message, actual, expected, operator: "==", stackStartFn: equal });
    }
  };

  assertFn.notEqual = function notEqual(actual, expected, message) {
    // eslint-disable-next-line eqeqeq
    if (actual == expected || (Number.isNaN(actual) && Number.isNaN(expected))) {
      innerFail({ message, actual, expected, operator: "!=", stackStartFn: notEqual });
    }
  };

  assertFn.strictEqual = function strictEqual(actual, expected, message) {
    if (!Object.is(actual, expected)) {
      innerFail({ message, actual, expected, operator: "strictEqual", stackStartFn: strictEqual });
    }
  };

  assertFn.notStrictEqual = function notStrictEqual(actual, expected, message) {
    if (Object.is(actual, expected)) {
      innerFail({ message, actual, expected, operator: "notStrictEqual", stackStartFn: notStrictEqual });
    }
  };

  assertFn.deepEqual = function deepEqual(actual, expected, message) {
    if (!looseDeepEqual(actual, expected)) {
      innerFail({ message, actual, expected, operator: "deepEqual", stackStartFn: deepEqual });
    }
  };

  assertFn.notDeepEqual = function notDeepEqual(actual, expected, message) {
    if (looseDeepEqual(actual, expected)) {
      innerFail({ message, actual, expected, operator: "notDeepEqual", stackStartFn: notDeepEqual });
    }
  };

  assertFn.deepStrictEqual = function deepStrictEqual(actual, expected, message) {
    if (!util.isDeepStrictEqual(actual, expected)) {
      innerFail({ message, actual, expected, operator: "deepStrictEqual", stackStartFn: deepStrictEqual });
    }
  };

  assertFn.notDeepStrictEqual = function notDeepStrictEqual(actual, expected, message) {
    if (util.isDeepStrictEqual(actual, expected)) {
      innerFail({ message, actual, expected, operator: "notDeepStrictEqual", stackStartFn: notDeepStrictEqual });
    }
  };

  function checkExpected(actual, expected, message, fnName) {
    if (expected instanceof RegExp || util.types.isRegExp(expected)) {
      return expected.test(String(actual && actual.message !== undefined ? actual.message : actual));
    }
    if (typeof expected === "function") {
      if (expected.prototype !== undefined && actual instanceof expected) {
        return true;
      }
      if (Object.prototype.isPrototypeOf.call(Error, expected)) {
        return false;
      }
      return expected(actual) === true;
    }
    if (typeof expected === "object" && expected !== null) {
      for (const key of Object.keys(expected)) {
        const want = expected[key];
        const got = actual[key];
        if (want instanceof RegExp || util.types.isRegExp(want)) {
          if (!want.test(String(got))) return false;
        } else if (!util.isDeepStrictEqual(got, want)) {
          return false;
        }
      }
      return true;
    }
    if (typeof expected === "string") {
      // string expected is only valid as `message` — handled by callers
      return true;
    }
    return true;
  }

  function expectsError(fnName, shouldThrow, actualError, threw, expected, message) {
    if (typeof expected === "string" && message === undefined) {
      message = expected;
      expected = undefined;
    }
    if (shouldThrow) {
      if (!threw) {
        innerFail({
          message: (message ? message + " " : "") + "Missing expected " + (fnName === "rejects" ? "rejection" : "exception") + (message ? "" : "."),
          actual: undefined,
          expected,
          operator: fnName,
          stackStartFn: expectsError,
        });
      }
      if (expected !== undefined && !checkExpected(actualError, expected, message, fnName)) {
        if (actualError instanceof Error && expected !== undefined &&
            !(expected instanceof RegExp) && typeof expected !== "function" &&
            typeof expected !== "object") {
          throw actualError;
        }
        innerFail({
          message: message || "The error does not match the expected pattern",
          actual: actualError,
          expected,
          operator: fnName,
          stackStartFn: expectsError,
        });
      }
    } else {
      if (threw) {
        if (expected !== undefined && !checkExpected(actualError, expected)) {
          throw actualError;
        }
        const err = new AssertionError({
          message: "Got unwanted " + (fnName === "doesNotReject" ? "rejection" : "exception") + (message ? ": " + message : ".") +
            "\nActual message: \"" + (actualError && actualError.message) + "\"",
          actual: actualError,
          expected,
          operator: fnName,
          stackStartFn: expectsError,
        });
        throw err;
      }
    }
  }

  assertFn.throws = function throws(fn, expected, message) {
    if (typeof fn !== "function") {
      throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", 'The "fn" argument must be of type function');
    }
    let threw = false;
    let actualError;
    try {
      fn();
    } catch (e) {
      threw = true;
      actualError = e;
    }
    expectsError("throws", true, actualError, threw, expected, message);
  };

  assertFn.doesNotThrow = function doesNotThrow(fn, expected, message) {
    if (typeof fn !== "function") {
      throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", 'The "fn" argument must be of type function');
    }
    let threw = false;
    let actualError;
    try {
      fn();
    } catch (e) {
      threw = true;
      actualError = e;
    }
    expectsError("doesNotThrow", false, actualError, threw, expected, message);
  };

  assertFn.rejects = async function rejects(promiseOrFn, expected, message) {
    let threw = false;
    let actualError;
    try {
      const p = typeof promiseOrFn === "function" ? promiseOrFn() : promiseOrFn;
      await p;
    } catch (e) {
      threw = true;
      actualError = e;
    }
    expectsError("rejects", true, actualError, threw, expected, message);
  };

  assertFn.doesNotReject = async function doesNotReject(promiseOrFn, expected, message) {
    let threw = false;
    let actualError;
    try {
      const p = typeof promiseOrFn === "function" ? promiseOrFn() : promiseOrFn;
      await p;
    } catch (e) {
      threw = true;
      actualError = e;
    }
    expectsError("doesNotReject", false, actualError, threw, expected, message);
  };

  assertFn.match = function match(string, regexp, message) {
    if (!util.types.isRegExp(regexp)) {
      throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", 'The "regexp" argument must be an instance of RegExp');
    }
    if (typeof string !== "string" || !regexp.test(string)) {
      innerFail({
        message: message ||
          (typeof string !== "string"
            ? 'The "string" argument must be of type string. Received type ' + typeof string
            : "The input did not match the regular expression " + regexp + ". Input:\n\n" + util.inspect(string) + "\n"),
        actual: string,
        expected: regexp,
        operator: "match",
        stackStartFn: match,
      });
    }
  };

  assertFn.doesNotMatch = function doesNotMatch(string, regexp, message) {
    if (!util.types.isRegExp(regexp)) {
      throw makeError(TypeError, "ERR_INVALID_ARG_TYPE", 'The "regexp" argument must be an instance of RegExp');
    }
    if (typeof string !== "string" || regexp.test(string)) {
      innerFail({
        message: message ||
          (typeof string !== "string"
            ? 'The "string" argument must be of type string. Received type ' + typeof string
            : "The input was expected to not match the regular expression " + regexp + ". Input:\n\n" + util.inspect(string) + "\n"),
        actual: string,
        expected: regexp,
        operator: "doesNotMatch",
        stackStartFn: doesNotMatch,
      });
    }
  };

  assertFn.ifError = function ifError(value) {
    if (value !== null && value !== undefined) {
      let message = "ifError got unwanted exception: ";
      if (typeof value === "object" && typeof value.message === "string") {
        message += value.message.length === 0 && value.constructor ? value.constructor.name : value.message;
      } else {
        message += util.inspect(value);
      }
      const err = new AssertionError({
        message,
        actual: value,
        expected: null,
        operator: "ifError",
        stackStartFn: ifError,
      });
      throw err;
    }
  };

  // Partial structural comparison (Node v22.13+: assert.partialDeepStrictEqual)
  function partialCompare(actual, expected, memo = new Map()) {
    if (Object.is(actual, expected)) return true;
    if (expected === null || typeof expected !== "object") return false;
    if (actual === null || typeof actual !== "object") return false;
    const seen = memo.get(expected);
    if (seen === actual) return true;
    memo.set(expected, actual);
    try {
      if (util.types.isDate(expected)) {
        return util.types.isDate(actual) && actual.getTime() === expected.getTime();
      }
      if (util.types.isRegExp(expected)) {
        return util.types.isRegExp(actual) && actual.source === expected.source && actual.flags === expected.flags;
      }
      if (Array.isArray(expected)) {
        if (!Array.isArray(actual)) return false;
        // every expected element must appear in order as a subsequence
        let ai = 0;
        for (const ev of expected) {
          let found = false;
          while (ai < actual.length) {
            if (partialCompare(actual[ai], ev, memo)) {
              found = true;
              ai += 1;
              break;
            }
            ai += 1;
          }
          if (!found) return false;
        }
        return true;
      }
      if (util.types.isSet(expected)) {
        if (!util.types.isSet(actual)) return false;
        for (const ev of expected) {
          let found = false;
          for (const av of actual) {
            if (partialCompare(av, ev, memo)) {
              found = true;
              break;
            }
          }
          if (!found) return false;
        }
        return true;
      }
      if (util.types.isMap(expected)) {
        if (!util.types.isMap(actual)) return false;
        for (const [ek, ev] of expected) {
          let found = false;
          for (const [ak, av] of actual) {
            if (partialCompare(ak, ek, memo) && partialCompare(av, ev, memo)) {
              found = true;
              break;
            }
          }
          if (!found) return false;
        }
        return true;
      }
      for (const key of [...Object.keys(expected), ...Object.getOwnPropertySymbols(expected).filter((s) => Object.getOwnPropertyDescriptor(expected, s).enumerable)]) {
        if (!(key in actual)) return false;
        if (!partialCompare(actual[key], expected[key], memo)) return false;
      }
      return true;
    } finally {
      memo.delete(expected);
    }
  }

  assertFn.partialDeepStrictEqual = function partialDeepStrictEqual(actual, expected, message) {
    if (!partialCompare(actual, expected)) {
      innerFail({ message, actual, expected, operator: "partialDeepStrictEqual", stackStartFn: partialDeepStrictEqual });
    }
  };

  class CallTracker {
    constructor() {
      this._calls = [];
    }
    calls(fn, expected = 1) {
      if (typeof fn === "number") {
        expected = fn;
        fn = () => {};
      }
      if (fn === undefined) fn = () => {};
      const entry = { fn, expected, actual: 0, name: fn.name || "calls", stackTrace: new Error() };
      this._calls.push(entry);
      const tracked = (...args) => {
        entry.actual += 1;
        return fn(...args);
      };
      return tracked;
    }
    getCalls(tracked) {
      return [];
    }
    report() {
      return this._calls
        .filter((e) => e.actual !== e.expected)
        .map((e) => ({
          message: "Expected the " + e.name + " function to be executed " + e.expected +
            " time(s) but was executed " + e.actual + " time(s).",
          actual: e.actual,
          expected: e.expected,
          operator: e.name,
          stack: e.stackTrace,
        }));
    }
    reset(tracked) {
      if (tracked === undefined) {
        for (const e of this._calls) e.actual = 0;
      }
    }
    verify() {
      const report = this.report();
      if (report.length > 0) {
        const err = new AssertionError({ message: report[0].message, operator: "verify" });
        err.details = report;
        throw err;
      }
    }
  }
  assertFn.CallTracker = CallTracker;

  class Assert {
    constructor(options) {
      this._options = options || {};
      for (const key of ["ok", "fail", "equal", "notEqual", "strictEqual", "notStrictEqual",
        "deepEqual", "notDeepEqual", "deepStrictEqual", "notDeepStrictEqual",
        "throws", "doesNotThrow", "rejects", "doesNotReject", "match", "doesNotMatch",
        "ifError", "partialDeepStrictEqual"]) {
        this[key] = assertFn[key].bind(null);
      }
    }
  }
  assertFn.Assert = Assert;

  // strict namespace
  const strict = Object.assign(
    function strictAssert(value, message) {
      if (!value) {
        innerFail({ message, actual: value, expected: true, operator: "==", stackStartFn: strictAssert });
      }
    },
    assertFn,
    {
      equal: assertFn.strictEqual,
      notEqual: assertFn.notStrictEqual,
      deepEqual: assertFn.deepStrictEqual,
      notDeepEqual: assertFn.notDeepStrictEqual,
    }
  );
  strict.strict = strict;
  assertFn.strict = strict;

  module.exports = assertFn;
});

__nanoNodeRegister("assert/strict", function (module, exports, require) {
  module.exports = require("assert").strict;
});
