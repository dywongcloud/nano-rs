"use strict";
// node:worker_threads, node:child_process, node:cluster — CONTRACT.md §6:
// spawning subprocesses/threads is sandbox-restricted. MessageChannel and
// BroadcastChannel (in-isolate messaging) are fully functional.
__nanoNodeRegister("worker_threads", function (module, exports, require) {
  const web = require("internal/web");
  const { notPermitted } = require("internal/errors");

  const untransferable = new WeakSet();
  let envData = new Map();

  class Worker extends require("events") {
    constructor() {
      super();
      throw notPermitted("worker_threads.Worker", "spawning worker threads");
    }
  }

  module.exports = {
    isMainThread: true,
    threadId: 0,
    workerData: null,
    parentPort: null,
    resourceLimits: {},
    SHARE_ENV: Symbol.for("nano.worker_threads.SHARE_ENV"),
    Worker,
    MessageChannel: web.MessageChannel,
    MessagePort: web.MessagePort,
    BroadcastChannel: web.BroadcastChannel,
    receiveMessageOnPort(port) {
      if (port._queue && port._queue.length > 0) {
        return { message: port._queue.shift() };
      }
      return undefined;
    },
    markAsUntransferable(obj) {
      untransferable.add(obj);
    },
    isMarkedAsUntransferable(obj) {
      return untransferable.has(obj);
    },
    moveMessagePortToContext() {
      const { unsupported } = require("internal/errors");
      throw unsupported("worker_threads.moveMessagePortToContext");
    },
    getEnvironmentData(key) {
      return envData.get(key);
    },
    setEnvironmentData(key, value) {
      if (value === undefined) {
        envData.delete(key);
      } else {
        envData.set(key, value);
      }
    },
  };
});

__nanoNodeRegister("child_process", function (module, exports, require) {
  const EventEmitter = require("events");
  const { Readable } = require("stream");
  const { notPermitted } = require("internal/errors");

  function nullStream() {
    const s = new Readable({ read() { this.push(null); } });
    return s;
  }

  class ChildProcess extends EventEmitter {
    constructor() {
      super();
      this.pid = undefined;
      this.stdin = null;
      this.stdout = null;
      this.stderr = null;
      this.stdio = [null, null, null, null, null];
      this.killed = false;
      this.connected = false;
      this.exitCode = null;
      this.signalCode = null;
      this.spawnfile = undefined;
      this.spawnargs = [];
    }
    kill() {
      return false;
    }
    send() {
      return false;
    }
    disconnect() {}
    ref() {}
    unref() {}
  }

  function spawnDenied(syscall, command) {
    const cp = new ChildProcess();
    cp.spawnfile = command;
    queueMicrotask(() => {
      const err = notPermitted(syscall, "child_process." + syscall + "('" + command + "')");
      err.errno = -1;
      err.syscall = syscall;
      err.path = command;
      err.spawnargs = [];
      cp.emit("error", err);
    });
    return cp;
  }

  function spawn(command, args, options) {
    return spawnDenied("spawn", command);
  }
  function exec(command, options, callback) {
    if (typeof options === "function") {
      callback = options;
      options = {};
    }
    const cp = spawnDenied("exec", command);
    if (callback) {
      cp.on("error", (err) => callback(err, "", ""));
    }
    return cp;
  }
  function execFile(file, args, options, callback) {
    if (typeof args === "function") { callback = args; args = []; options = {}; }
    else if (typeof options === "function") { callback = options; options = {}; }
    const cp = spawnDenied("execFile", file);
    if (callback) {
      cp.on("error", (err) => callback(err, "", ""));
    }
    return cp;
  }
  function fork(modulePath, args, options) {
    const cp = spawnDenied("fork", modulePath);
    cp.connected = false;
    return cp;
  }

  // Real Node's sync EPERM shape: spawnSync returns an object carrying
  // `.error` rather than throwing (execSync/execFileSync do throw).
  function spawnSync(command) {
    const err = notPermitted("spawnSync", "child_process.spawnSync('" + command + "')");
    err.errno = -1;
    err.syscall = "spawnSync";
    err.path = command;
    return {
      pid: undefined,
      output: [null, null, null],
      stdout: null,
      stderr: null,
      status: null,
      signal: null,
      error: err,
    };
  }
  function execSync(command) {
    throw notPermitted("execSync", "child_process.execSync('" + command + "')");
  }
  function execFileSync(file) {
    throw notPermitted("execFileSync", "child_process.execFileSync('" + file + "')");
  }

  module.exports = {
    ChildProcess,
    spawn, exec, execFile, fork,
    spawnSync, execSync, execFileSync,
  };
});

__nanoNodeRegister("cluster", function (module, exports, require) {
  const EventEmitter = require("events");
  const { notPermitted } = require("internal/errors");

  class Worker extends EventEmitter {
    constructor(id) {
      super();
      this.id = id;
      this.process = null;
      this.state = "none";
      this.isDead = () => true;
      this.isConnected = () => false;
    }
    send() { return false; }
    kill() {}
    destroy() {}
    disconnect(cb) {
      if (cb) queueMicrotask(cb);
    }
  }

  class Cluster extends EventEmitter {
    constructor() {
      super();
      this.isPrimary = true;
      this.isMaster = true;
      this.isWorker = false;
      this.workers = {};
      this.settings = {};
      this.schedulingPolicy = 2; // SCHED_RR
      this.SCHED_NONE = 1;
      this.SCHED_RR = 2;
    }
    setupPrimary(settings) {
      this.settings = { ...this.settings, ...(settings || {}) };
    }
    setupMaster(settings) {
      this.setupPrimary(settings);
    }
    fork() {
      throw notPermitted("fork", "cluster.fork");
    }
    disconnect(cb) {
      if (cb) queueMicrotask(cb);
    }
  }

  module.exports = new Cluster();
  module.exports.Worker = Worker;
});
