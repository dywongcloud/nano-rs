// Framework-bundle validation driver: runs a real bundled framework app
// inside the NANO-faithful harness (eval banned, strict builtins — any
// module the node_compat layer doesn't provide throws MODULE_NOT_FOUND),
// resolves the handler exactly as the Rust runtime does, and dispatches
// requests through it.
//
// Setup (bundles are not committed — build them from the real npm packages):
//   mkdir fw && cd fw && npm i express hono esbuild
//   # write an app per framework (routes: GET /, GET /users/:id, POST /echo;
//   # express also needs a trailing 404 handler; see the assertions below),
//   # then bundle:
//   npx esbuild app-express.js --bundle --platform=node --format=cjs --outfile=express.bundle.js
//   npx esbuild app-hono.js --bundle --platform=neutral --format=esm --outfile=hono.bundle.js
//
// Usage: node framework_bundle.test.mjs <bundle.js> <server|esm-fetch> <express|hono|...>
//   server    — app calls http.createServer(...).listen(); resolved via the
//               tier-3 http-bridge adapter (CONTRACT.md §7)
//   esm-fetch — app does `export default app` (Hono style); the driver
//               replicates NANO's transform_module_code, including esbuild's
//               `export { X as default };` form
//
// Known-negative: fastify does NOT run under NANO — its router (find-my-way)
// compiles handlers with `new Function` at startup, which the eval ban
// rejects (same restriction as Cloudflare Workers). Expect BUNDLE EVALUATION
// FAILED with EvalError for a fastify bundle; that is the correct outcome.
import { createEnv, allLayerFiles } from "./harness.mjs";
import { readFileSync } from "node:fs";
import path from "node:path";
import vm from "node:vm";

const [, , bundlePath, mode, fw] = process.argv;

const { context, unprovided } = createEnv(allLayerFiles(), { strictBuiltins: true });
console.log("[strict] builtins NOT provided by node_compat (require would throw):", unprovided.join(", ") || "(none)");

let code = readFileSync(bundlePath, "utf8");

if (mode === "esm-fetch") {
  // Replicate NANO's transform_module_code (ADR-007), including handling of
  // esbuild's `export { X as default };` form.
  code = code.replace(/export\s*\{\s*([A-Za-z0-9_$]+)\s+as\s+default\s*,?\s*\}\s*;?/g, "var __nano_handler = $1;");
  code = code.replace("export default", "var __nano_handler =");
  code += "\n\nvar __nano_user_fetch = undefined;\nif (typeof __nano_handler === 'object' && __nano_handler.fetch) {\n    __nano_user_fetch = __nano_handler.fetch;\n}";
}

try {
  vm.runInContext(code, context, { filename: path.basename(bundlePath) });
} catch (e) {
  console.error(`BUNDLE EVALUATION FAILED (${fw}):`, e.stack || e);
  process.exit(1);
}

let failed = false;
function check(cond, msg) {
  if (!cond) { console.error("FAIL:", msg); failed = true; }
  else console.log("ok:", msg);
}

function nanoRequest({ method = "GET", url = "/", headers = {}, bodyText = null } = {}) {
  return {
    method, url,
    headers: { forEach(cb) { for (const [k, v] of Object.entries(headers)) cb(v, k); } },
    body: bodyText === null ? null : Buffer.from(bodyText).toString("base64"),
  };
}

if (mode === "server") {
  const handler = context.__nano_resolve_handler();
  check(typeof handler === "function", `${fw}: __nano_resolve_handler finds the http.createServer bridge`);
  if (typeof handler !== "function") process.exit(1);

  const r1 = await handler(nanoRequest({ method: "GET", url: "/" }));
  check(r1.status === 200, `${fw}: GET / → 200 (got ${r1.status})`);
  check(r1.body === `hello from ${fw}`, `${fw}: GET / body (got ${JSON.stringify(r1.body)})`);
  check(r1.headers["x-framework"] === fw, `${fw}: custom response header (got ${JSON.stringify(r1.headers["x-framework"])})`);

  const r2 = await handler(nanoRequest({ method: "GET", url: "/users/42?verbose=1" }));
  check(r2.status === 200, `${fw}: GET /users/42 → 200 (got ${r2.status})`);
  const j2 = JSON.parse(r2.body);
  check(j2.id === "42" && j2.q === true, `${fw}: route params + query parsed (got ${r2.body})`);
  check((r2.headers["content-type"] || "").includes("application/json"), `${fw}: JSON content-type (got ${r2.headers["content-type"]})`);

  const r3 = await handler(nanoRequest({
    method: "POST", url: "/echo",
    headers: { "content-type": "application/json", "content-length": "16" },
    bodyText: '{"name":"nano"}\n',
  }));
  check(r3.status === 201, `${fw}: POST /echo → 201 (got ${r3.status}, body ${JSON.stringify(r3.body)})`);
  const j3 = JSON.parse(r3.body);
  check(j3.received && j3.received.name === "nano", `${fw}: JSON request body parsed by framework middleware (got ${r3.body})`);

  if (fw === "express") {
    const r4 = await handler(nanoRequest({ method: "GET", url: "/definitely-not-a-route" }));
    check(r4.status === 404, `${fw}: unmatched route → 404 handler (got ${r4.status})`);
  }
} else {
  // esm-fetch: WinterTC-style — handler receives a standard Request,
  // returns a standard Response.
  const handler = context.__nano_user_fetch;
  check(typeof handler === "function", `${fw}: transform produced __nano_user_fetch`);
  if (typeof handler !== "function") process.exit(1);

  const r1 = await handler(new Request("https://tenant.example.com/"));
  check(r1.status === 200, `${fw}: GET / → 200 (got ${r1.status})`);
  check((await r1.text()) === `hello from ${fw}`, `${fw}: GET / body`);
  check(r1.headers.get("x-framework") === fw, `${fw}: custom response header`);

  const r2 = await handler(new Request("https://tenant.example.com/users/42"));
  const j2 = await r2.json();
  check(j2.id === "42", `${fw}: route params (got ${JSON.stringify(j2)})`);

  const r3 = await handler(new Request("https://tenant.example.com/echo", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ name: "nano" }),
  }));
  check(r3.status === 201, `${fw}: POST /echo → 201 (got ${r3.status})`);
  const j3 = await r3.json();
  check(j3.received && j3.received.name === "nano", `${fw}: JSON body round-trip (got ${JSON.stringify(j3)})`);
}

console.log(failed ? `\n${fw.toUpperCase()}: FAILED` : `\n${fw.toUpperCase()}: ALL PASSED`);
process.exit(failed ? 1 : 0);
