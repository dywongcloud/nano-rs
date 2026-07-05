"use strict";
// node:os — virtualized system information for the NANO isolate.
//
// Divergence (documented in docs/NODEJS_COMPAT.md): memory totals are
// isolate-scale synthetic values, cpus() reports virtual cores, and
// network interfaces expose only loopback — the sandbox does not reveal
// host topology to tenants.
__nanoNodeRegister("os", function (module, exports, require) {
  const host = globalThis.__nano_node_host;
  const { UV_ERRNOS } = require("internal/errors");

  const TOTAL_MEM = 512 * 1024 * 1024;
  const FREE_MEM = 256 * 1024 * 1024;

  function cpuEntry() {
    return {
      model: "NANO virtual CPU",
      speed: 2400,
      times: { user: 0, nice: 0, sys: 0, idle: 0, irq: 0 },
    };
  }

  const signals = {
    SIGHUP: 1, SIGINT: 2, SIGQUIT: 3, SIGILL: 4, SIGTRAP: 5, SIGABRT: 6,
    SIGIOT: 6, SIGBUS: 7, SIGFPE: 8, SIGKILL: 9, SIGUSR1: 10, SIGSEGV: 11,
    SIGUSR2: 12, SIGPIPE: 13, SIGALRM: 14, SIGTERM: 15, SIGCHLD: 17,
    SIGSTKFLT: 16, SIGCONT: 18, SIGSTOP: 19, SIGTSTP: 20, SIGTTIN: 21,
    SIGTTOU: 22, SIGURG: 23, SIGXCPU: 24, SIGXFSZ: 25, SIGVTALRM: 26,
    SIGPROF: 27, SIGWINCH: 28, SIGIO: 29, SIGPOLL: 29, SIGPWR: 30, SIGSYS: 31,
  };

  const errno = {};
  for (const [name, num] of Object.entries(UV_ERRNOS)) {
    if (num > -3000) {
      errno[name] = -num;
    }
  }

  const constants = Object.freeze({
    UV_UDP_REUSEADDR: 4,
    dlopen: Object.freeze({ RTLD_LAZY: 1, RTLD_NOW: 2, RTLD_GLOBAL: 256, RTLD_LOCAL: 0, RTLD_DEEPBIND: 8 }),
    errno: Object.freeze(errno),
    signals: Object.freeze(signals),
    priority: Object.freeze({
      PRIORITY_LOW: 19, PRIORITY_BELOW_NORMAL: 10, PRIORITY_NORMAL: 0,
      PRIORITY_ABOVE_NORMAL: -7, PRIORITY_HIGH: -14, PRIORITY_HIGHEST: -20,
    }),
  });

  module.exports = {
    EOL: "\n",
    devNull: "/dev/null",
    constants,
    availableParallelism: () => host.availableParallelism(),
    arch: () => "x64",
    machine: () => "x86_64",
    cpus: () => Array.from({ length: host.availableParallelism() }, cpuEntry),
    endianness: () => "LE",
    freemem: () => FREE_MEM,
    totalmem: () => TOTAL_MEM,
    getPriority: () => 0,
    setPriority: () => undefined,
    homedir: () => "/",
    hostname: () => host.hostname(),
    loadavg: () => [0, 0, 0],
    networkInterfaces: () => ({
      lo: [
        {
          address: "127.0.0.1", netmask: "255.0.0.0", family: "IPv4",
          mac: "00:00:00:00:00:00", internal: true, cidr: "127.0.0.1/8",
        },
        {
          address: "::1", netmask: "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
          family: "IPv6", mac: "00:00:00:00:00:00", internal: true,
          cidr: "::1/128", scopeid: 0,
        },
      ],
    }),
    platform: () => "linux",
    release: () => "6.0.0-nano",
    tmpdir: () => "/tmp",
    type: () => "Linux",
    uptime: () => Math.floor(performance.now() / 1000),
    userInfo: (options) => ({
      uid: 0,
      gid: 0,
      username: "nano",
      homedir: "/",
      shell: options && options.encoding === "buffer" ? null : null,
    }),
    version: "#1 SMP NANO 6.0.0",
  };
});
