// Differential regression test for node:string_decoder against real Node,
// including the ES5 `StringDecoder.call(this)` inheritance pattern that
// iconv-lite ≤0.4 (Koa body-parsing stack) depends on.
// Run: node src/runtime/node_compat/testing/string_decoder.test.mjs
import { createEnv } from "./harness.mjs";
import { StringDecoder as RealSD } from "node:string_decoder";

const { require: nanoRequire } = createEnv(["11_buffer.js", "14_string_decoder.js"]);
const { StringDecoder } = nanoRequire("string_decoder");

let failed = false;
const check = (c, m) => { if (!c) { console.error("FAIL:", m); failed = true; } else console.log("ok:", m); };

// Differential: chunked writes across every tricky boundary, vs real Node.
const cases = [
  ["utf8", [[0xe2], [0x82], [0xac]]],                        // € split 3 ways
  ["utf8", [[0xf0, 0x9f], [0x92, 0xa9]]],                    // 💩 split mid-4-byte
  ["utf8", [[0x61, 0xc3], [0xa9, 0x62]]],                    // aéb split mid-2-byte
  ["utf8", [[0xe2, 0x82]]],                                  // incomplete at end()
  ["utf16le", [[0x3d, 0xd8], [0xa9, 0xdc]]],                 // 💩 split mid-surrogate-pair
  ["utf16le", [[0x61, 0x00, 0x62]]],                         // odd trailing byte
  ["base64", [[1, 2], [3, 4], [5]]],
  ["hex", [[0xde, 0xad], [0xbe, 0xef]]],
  ["latin1", [[0xe9, 0xe8]]],
];
for (const [enc, chunks] of cases) {
  const mine = new StringDecoder(enc), real = new RealSD(enc);
  let m = "", r = "";
  for (const c of chunks) { m += mine.write(Buffer.from(c)); r += real.write(Buffer.from(c)); }
  m += mine.end(); r += real.end();
  check(m === r, `${enc} ${JSON.stringify(chunks)} → ${JSON.stringify(m)} (real: ${JSON.stringify(r)})`);
}

// The regression that broke Koa: ES5 .call() inheritance (iconv-lite ≤0.4 pattern).
function InternalDecoder(enc) { StringDecoder.call(this, enc); }
InternalDecoder.prototype = StringDecoder.prototype;
const d = new InternalDecoder("utf8");
check(d.write(Buffer.from([0xe2])) === "" && d.write(Buffer.from([0x82, 0xac])) === "€",
  "ES5 StringDecoder.call(this) inheritance works (iconv-lite pattern)");

console.log(failed ? "\nSTRING_DECODER: FAILED" : "\nSTRING_DECODER: ALL PASSED");
process.exit(failed ? 1 : 0);
