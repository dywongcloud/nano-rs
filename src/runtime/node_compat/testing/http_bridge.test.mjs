// Regression test for internal/http-bridge's `buildHandlerAdapter()`
// (CONTRACT.md §7): the returned adapter must accept the same NANO-shaped
// Request the Rust runtime passes to `fetch(request)` handlers, and return
// the same plain `{ status, headers, body }` Response-shape — not the
// internal base64 wire format used by `http._internal.createServerResponse`.
//
// Run: node src/runtime/node_compat/testing/http_bridge.test.mjs
import { createEnv } from "./harness.mjs";

const { require: nanoRequire } = createEnv([
  "10_events.js", "11_buffer.js", "12_path.js", "14_string_decoder.js",
  "16_util.js", "18_url.js", "20_process.js", "21_stream.js", "22_timers.js",
  "26_http.js", "27_net.js", "32_web.js", "33_console.js", "35_http_bridge.js",
]);

const http = nanoRequire("http");
const bridge = nanoRequire("internal/http-bridge");

const server = http.createServer((req, res) => {
  let chunks = [];
  req.on("data", (c) => chunks.push(c));
  req.on("end", () => {
    const bodyText = Buffer.concat(chunks).toString("utf8");
    res.writeHead(200, { "content-type": "text/plain", "x-echo-method": req.method });
    res.end(`hello ${req.method} ${req.url} body=${bodyText}`);
  });
});
server.listen(3000);

const adapter = bridge.buildHandlerAdapter();
if (typeof adapter !== "function") {
  console.error("FAIL: buildHandlerAdapter() did not return a function");
  process.exit(1);
}

// Mock a NANO-shaped Request: .method/.url plain, .headers with forEach
// (like the real Headers class — see apis.rs headers_foreach_callback),
// .body as base64 text (like request.rs's Request instance) or null.
function mockRequest({ method, url, headers, bodyBase64 }) {
  return {
    method,
    url,
    headers: {
      forEach(cb) {
        for (const [k, v] of Object.entries(headers || {})) cb(v, k);
      },
    },
    body: bodyBase64 ?? null,
  };
}

let failed = false;
function check(cond, msg) {
  if (!cond) { console.error("FAIL:", msg); failed = true; }
  else console.log("ok:", msg);
}

const req1 = mockRequest({ method: "GET", url: "/foo?x=1", headers: { "x-test": "abc" } });
const result = await adapter(req1);

check(typeof result === "object" && result !== null, "adapter returns an object");
check(result.status === 200, `status is 200 (got ${result.status})`);
check(typeof result.headers === "object" && !Array.isArray(result.headers), "headers is a plain object (not array of pairs)");
check(result.headers["content-type"] === "text/plain", `content-type header present (got ${JSON.stringify(result.headers)})`);
check(result.headers["x-echo-method"] === "GET", "custom header echoed");
check(typeof result.body === "string", `body is a string (got ${typeof result.body})`);
check(result.body === "hello GET /foo?x=1 body=", `body content correct (got ${JSON.stringify(result.body)})`);

// POST with a body (base64-encoded, matching request.rs's internal representation).
const postBody = Buffer.from("name=nano").toString("base64");
const req2 = mockRequest({ method: "POST", url: "/submit", headers: {}, bodyBase64: postBody });
const result2 = await adapter(req2);
check(result2.body === "hello POST /submit body=name=nano", `POST body round-trips correctly (got ${JSON.stringify(result2.body)})`);

if (failed) {
  console.error("\nHTTP BRIDGE TEST: FAILED");
  process.exit(1);
} else {
  console.log("\nHTTP BRIDGE TEST: ALL PASSED");
}
