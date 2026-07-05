// Regression test for `globalThis.__nano_resolve_handler` (99_init.js,
// CONTRACT.md §7): exercises all four handler-resolution tiers against the
// exact calling convention the Rust runtime uses (call with a NANO-shaped
// Request, expect a plain `{ status, headers, body }` Response-shape back).
//
// Run: node src/runtime/node_compat/testing/resolve_handler.test.mjs
import { createEnv, allLayerFiles } from "./harness.mjs";

const allFiles = allLayerFiles();

let failed = false;
function check(cond, msg) {
  if (!cond) { console.error("FAIL:", msg); failed = true; }
  else console.log("ok:", msg);
}

function mockRequest({ method = "GET", url = "/", headers = {}, bodyBase64 = null } = {}) {
  return {
    method, url,
    headers: { forEach(cb) { for (const [k, v] of Object.entries(headers)) cb(v, k); } },
    body: bodyBase64,
  };
}

// --- Tier 1: __nano_user_fetch (ESM transform output) ---
{
  const { context } = createEnv(allFiles);
  context.__nano_user_fetch = async (req) => ({ status: 201, headers: { "x-tier": "1" }, body: "tier1:" + req.method });
  const handler = context.__nano_resolve_handler();
  check(typeof handler === "function", "tier1: resolves a function");
  const resp = await handler(mockRequest({ method: "GET" }));
  check(resp.status === 201 && resp.body === "tier1:GET", `tier1: __nano_user_fetch wins (got ${JSON.stringify(resp)})`);
}

// --- Tier 2: module.exports.fetch (CJS bundle) ---
{
  const { context } = createEnv(allFiles);
  context.module.exports = { fetch: async (req) => ({ status: 202, headers: {}, body: "tier2:" + req.url }) };
  const handler = context.__nano_resolve_handler();
  check(typeof handler === "function", "tier2: resolves a function");
  const resp = await handler(mockRequest({ url: "/t2" }));
  check(resp.status === 202 && resp.body === "tier2:/t2", `tier2: module.exports.fetch wins (got ${JSON.stringify(resp)})`);
}

// --- Tier 2b: module.exports.default.fetch ---
{
  const { context } = createEnv(allFiles);
  context.module.exports = { default: { fetch: async (req) => ({ status: 203, headers: {}, body: "tier2b" }) } };
  const handler = context.__nano_resolve_handler();
  const resp = await handler(mockRequest());
  check(resp.status === 203, `tier2b: module.exports.default.fetch wins (got ${JSON.stringify(resp)})`);
}

// --- Tier 3: registered http.Server, no fetch export ---
{
  const { context, require: nanoRequire } = createEnv(allFiles);
  const http = nanoRequire("http");
  http.createServer((req, res) => {
    res.writeHead(200, { "content-type": "text/plain" });
    res.end("tier3:" + req.method + req.url);
  }).listen(3000);
  const handler = context.__nano_resolve_handler();
  check(typeof handler === "function", "tier3: resolves a function (http-bridge adapter)");
  const resp = await handler(mockRequest({ method: "GET", url: "/t3" }));
  check(resp.status === 200 && resp.body === "tier3:GET/t3", `tier3: http-bridge adapter wins (got ${JSON.stringify(resp)})`);
}

// --- Tier 4: nothing registered at all ---
{
  const { context } = createEnv(allFiles);
  const handler = context.__nano_resolve_handler();
  check(handler === undefined, `tier4: undefined when nothing is registered (got ${handler})`);
}

if (failed) {
  console.error("\n__nano_resolve_handler TEST: FAILED");
  process.exit(1);
} else {
  console.log("\n__nano_resolve_handler TEST: ALL PASSED");
}
