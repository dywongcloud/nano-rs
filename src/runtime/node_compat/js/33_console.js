"use strict";
// node:console and internal/console — full Console class over the bound
// native transports (log/warn/error stay wired to the Rust tracing sinks).
__nanoNodeRegister("internal/console", function (module, exports, require) {
  const util = require("util");

  const kCounts = Symbol("counts");
  const kTimes = Symbol("times");
  const kGroupIndent = Symbol("groupIndent");
  const kWriteToStdout = Symbol("stdout");
  const kWriteToStderr = Symbol("stderr");

  function indent(str, level) {
    if (level === 0) return str;
    const prefix = "  ".repeat(level);
    return str
      .split("\n")
      .map((line) => prefix + line)
      .join("\n");
  }

  function writeLine(console, sink, args) {
    const text = indent(util.formatWithOptions({ colors: false }, ...args), console[kGroupIndent]);
    sink(text);
  }

  class Console {
    constructor(options, stderrArg, ignoreErrors) {
      let stdout;
      let stderr;
      let opts = options;
      if (options instanceof (require("stream").Writable) || (options && typeof options.write === "function" && stderrArg === undefined)) {
        stdout = options;
        stderr = stderrArg || options;
        opts = {};
      } else {
        opts = options || {};
        stdout = opts.stdout;
        stderr = opts.stderr || stdout;
      }
      if (typeof stdout?.write !== "function") {
        throw new TypeError('The "stdout" argument must have a write() method');
      }
      this._stdout = stdout;
      this._stderr = stderr || stdout;
      this[kCounts] = new Map();
      this[kTimes] = new Map();
      this[kGroupIndent] = 0;
      this[kWriteToStdout] = (text) => this._stdout.write(text + "\n");
      this[kWriteToStderr] = (text) => this._stderr.write(text + "\n");
    }

    log(...args) {
      writeLine(this, this[kWriteToStdout], args);
    }
    info(...args) {
      this.log(...args);
    }
    debug(...args) {
      this.log(...args);
    }
    warn(...args) {
      writeLine(this, this[kWriteToStderr], args);
    }
    error(...args) {
      this.warn(...args);
    }
    trace(...args) {
      const err = new Error();
      const stack = (err.stack || "").split("\n").slice(1).join("\n");
      writeLine(this, this[kWriteToStderr], ["Trace: " + util.formatWithOptions({}, ...args) + (stack ? "\n" + stack : "")]);
    }
    dir(obj, options) {
      const text = util.inspect(obj, { customInspect: false, ...options });
      writeLine(this, this[kWriteToStdout], [text]);
    }
    dirxml(...args) {
      this.log(...args);
    }
    assert(expression, ...args) {
      if (!expression) {
        const rest = args.length > 0 ? [": " + util.formatWithOptions({}, ...args)] : [""];
        writeLine(this, this[kWriteToStderr], ["Assertion failed" + rest[0]]);
      }
    }
    count(label = "default") {
      const key = String(label);
      const n = (this[kCounts].get(key) || 0) + 1;
      this[kCounts].set(key, n);
      writeLine(this, this[kWriteToStdout], [key + ": " + n]);
    }
    countReset(label = "default") {
      this[kCounts].set(String(label), 0);
    }
    time(label = "default") {
      this[kTimes].set(String(label), performance.now());
    }
    timeEnd(label = "default") {
      const key = String(label);
      const start = this[kTimes].get(key);
      if (start === undefined) {
        writeLine(this, this[kWriteToStderr], ["Warning: No such label '" + key + "' for console.timeEnd()"]);
        return;
      }
      this[kTimes].delete(key);
      const ms = performance.now() - start;
      writeLine(this, this[kWriteToStdout], [key + ": " + ms.toFixed(3) + "ms"]);
    }
    timeLog(label = "default", ...args) {
      const key = String(label);
      const start = this[kTimes].get(key);
      if (start === undefined) {
        writeLine(this, this[kWriteToStderr], ["Warning: No such label '" + key + "' for console.timeLog()"]);
        return;
      }
      const ms = performance.now() - start;
      writeLine(this, this[kWriteToStdout], [key + ": " + ms.toFixed(3) + "ms", ...args]);
    }
    group(...args) {
      if (args.length > 0) this.log(...args);
      this[kGroupIndent] += 1;
    }
    groupCollapsed(...args) {
      this.group(...args);
    }
    groupEnd() {
      this[kGroupIndent] = Math.max(0, this[kGroupIndent] - 1);
    }
    table(data, columns) {
      writeLine(this, this[kWriteToStdout], [renderTable(data, columns, util)]);
    }
    clear() {}
    profile() {}
    profileEnd() {}
    timeStamp() {}
  }

  function cellText(value, util_) {
    return util_.inspect(value, { breakLength: Infinity, compact: true, depth: 0 });
  }

  function renderTable(data, columns, util_) {
    if (data === null || typeof data !== "object") {
      return util_.inspect(data);
    }
    const isArrayLike = Array.isArray(data);
    const indexKeys = isArrayLike
      ? data.map((_, i) => String(i))
      : Object.keys(data);
    const rows = isArrayLike ? data : Object.values(data);

    const colSet = new Set();
    let hasValuesColumn = false;
    const rowCells = rows.map((row) => {
      if (row !== null && typeof row === "object" && !Array.isArray(row)) {
        const cells = {};
        for (const k of Object.keys(row)) {
          if (columns && !columns.includes(k)) continue;
          colSet.add(k);
          cells[k] = cellText(row[k], util_);
        }
        return cells;
      }
      if (Array.isArray(row)) {
        const cells = {};
        row.forEach((v, i) => {
          const k = String(i);
          if (columns && !columns.includes(k)) return;
          colSet.add(k);
          cells[k] = cellText(v, util_);
        });
        return cells;
      }
      hasValuesColumn = true;
      return { Values: cellText(row, util_) };
    });
    const cols = ["(index)", ...colSet, ...(hasValuesColumn ? ["Values"] : [])];

    const table = [cols];
    indexKeys.forEach((idx, i) => {
      const row = [idx];
      for (const c of cols.slice(1)) {
        row.push(rowCells[i][c] !== undefined ? rowCells[i][c] : "");
      }
      table.push(row);
    });

    const widths = cols.map((_, ci) => Math.max(...table.map((r) => String(r[ci]).length)));
    const rule = "┌" + widths.map((w) => "─".repeat(w + 2)).join("┬") + "┐";
    const mid = "├" + widths.map((w) => "─".repeat(w + 2)).join("┼") + "┤";
    const bottom = "└" + widths.map((w) => "─".repeat(w + 2)).join("┴") + "┘";
    const rowLine = (row) =>
      "│ " + row.map((cell, ci) => String(cell).padEnd(widths[ci])).join(" │ ") + " │";

    const lines = [rule, rowLine(table[0]), mid];
    for (let i = 1; i < table.length; i += 1) {
      lines.push(rowLine(table[i]));
    }
    lines.push(bottom);
    return lines.join("\n");
  }

  function upgradeGlobalConsole(g) {
    const existing = g.console || {};
    const nativeLog = typeof existing.log === "function" ? existing.log.bind(existing) : (...a) => {};
    const nativeWarn = typeof existing.warn === "function" ? existing.warn.bind(existing) : nativeLog;
    const nativeError = typeof existing.error === "function" ? existing.error.bind(existing) : nativeWarn;

    const upgraded = new Console({
      stdout: { write: (text) => nativeLog(text.replace(/\n$/, "")) },
      stderr: { write: (text) => nativeError(text.replace(/\n$/, "")) },
    });
    g.console = upgraded;
    return upgraded;
  }

  module.exports = { Console, upgradeGlobalConsole };
});

__nanoNodeRegister("console", function (module, exports, require) {
  const { Console } = require("internal/console");
  module.exports = globalThis.console;
  module.exports.Console = Console;
});
