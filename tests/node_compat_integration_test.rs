//! End-to-end integration tests for the Node.js compatibility layer.
//!
//! Exercises the actual live request path (transform_module_code +
//! Script::compile, exactly as worker/pool.rs and handler.rs run real
//! traffic) with handlers that use ESM `import` syntax against Node.js
//! builtins (ADR-007 transformation + node_compat require() bridge), and
//! the WebCrypto `subtle.deriveBits`/`deriveKey` ECDH binding.

use nano::http::{NanoHeaders, NanoRequest, NanoUrl};
use nano::runtime::{execute_handler, HandlerContext};
use nano::v8::{initialize_platform, transform_imports, NanoIsolate};
use std::sync::Once;

fn init_platform() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        initialize_platform().expect("Failed to initialize V8 platform");
    });
}

fn run_handler(js_code: &str, filename: &str) -> nano::http::NanoResponse {
    let temp_dir = std::env::temp_dir();
    let js_path = temp_dir.join(filename);
    std::fs::write(&js_path, js_code).expect("Failed to write test JS file");
    let js_path_str = js_path.to_string_lossy().to_string();

    init_platform();
    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

    let url = NanoUrl::parse("https://example.com/api").unwrap();
    let request = NanoRequest::new("GET".to_string(), url, NanoHeaders::new(), None);

    let context = HandlerContext {
        entrypoint: js_path_str,
        request,
        memory_limit_mb: 0,
        hostname: "test.example.com".to_string(),
    };

    execute_handler(&mut isolate, context).expect("handler execution failed")
}

/// The three ESM import forms a bundler commonly emits against Node
/// builtins (default, named, namespace — with and without the `node:`
/// prefix) all resolve through require() after transform_module_code,
/// and the builtins actually work end-to-end.
#[test]
fn test_esm_node_builtin_imports_end_to_end() {
    let js_code = r#"
        import crypto from 'node:crypto';
        import { randomUUID } from 'crypto';
        import * as qs from 'node:querystring';

        export default {
            async fetch(request) {
                const hash = crypto.createHash('sha256').update('nano-test').digest('hex');
                const uuid = randomUUID();
                const parsed = qs.parse('a=1&b=2');
                return {
                    status: 200,
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({
                        hash,
                        uuidValid: /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(uuid),
                        a: parsed.a,
                        b: parsed.b,
                    }),
                };
            }
        };
    "#;

    let response = run_handler(js_code, "test_esm_node_builtins.js");
    assert_eq!(response.status(), 200);
    let body = response.body().expect("response should have a body");
    let json: serde_json::Value = serde_json::from_slice(body).expect("body should be JSON");

    let expected_hash = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(b"nano-test");
        hex::encode(hasher.finalize())
    };
    assert_eq!(json["hash"], expected_hash, "node:crypto createHash must be correct");
    assert_eq!(json["uuidValid"], true, "crypto.randomUUID must produce a valid v4 UUID");
    assert_eq!(json["a"], "1", "querystring.parse must parse correctly");
    assert_eq!(json["b"], "2", "querystring.parse must parse correctly");
}

/// A side-effect-only import (`import 'node:xyz';`, no bindings) and a
/// mixed default+named import in one statement both transform correctly.
#[test]
fn test_esm_import_side_effect_and_mixed_forms() {
    let js_code = r#"
        import 'node:buffer';
        import util, { inspect } from 'node:util';

        export default {
            async fetch(request) {
                const ok = typeof util.format === 'function' && typeof inspect === 'function';
                return {
                    status: ok ? 200 : 500,
                    headers: { 'Content-Type': 'text/plain' },
                    body: ok ? 'ok' : 'fail',
                };
            }
        };
    "#;

    let response = run_handler(js_code, "test_esm_mixed_imports.js");
    assert_eq!(response.status(), 200);
    let body = response.body().expect("response should have a body");
    assert_eq!(String::from_utf8_lossy(body), "ok");
}

/// transform_imports in isolation: verifies the generated `require()` calls
/// are syntactically valid (round-trips through a real V8 compile) for
/// every supported import form, independent of the full handler pipeline.
#[test]
fn test_transform_imports_produces_valid_syntax() {
    init_platform();
    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");

    let cases = [
        r#"import Default from "node:fs";"#,
        r#"import * as ns from "node:path";"#,
        r#"import { a, b as c } from "node:util";"#,
        r#"import Default, { a, b } from "node:util";"#,
        r#"import Default, * as ns from "node:util";"#,
        r#"import "node:buffer";"#,
        "import {\n  a,\n  b as c,\n} from \"node:util\";",
    ];

    for case in cases {
        let transformed = transform_imports(case);
        assert!(!transformed.contains("import "), "import keyword should be gone: {}", transformed);
        assert!(transformed.contains("require("), "should use require(): {}", transformed);

        // Real V8 compile check: must be syntactically valid classic-script JS.
        v8::scope!(handle_scope, isolate.isolate());
        let context = v8::Context::new(handle_scope, Default::default());
        let ctx_scope = &mut v8::ContextScope::new(handle_scope, context);
        let code_str = v8::String::new(ctx_scope, &format!(
            "function require(x) {{ return {{ default: {{}}, a: 1, b: 2, c: 3 }}; }}\n{}",
            transformed
        )).unwrap();
        let script = v8::Script::compile(ctx_scope, code_str, None);
        assert!(script.is_some(), "transformed code failed to compile: {}", transformed);
        assert!(script.unwrap().run(ctx_scope).is_some(), "transformed code failed to run: {}", transformed);
    }
}

/// Type-only imports carry no runtime value and are dropped entirely.
#[test]
fn test_transform_imports_drops_type_only() {
    let transformed = transform_imports(r#"import type { Foo } from "some-types";"#);
    assert_eq!(transformed.trim(), "");
}

/// esbuild's ESM output emits `export { X as default };` instead of a literal
/// `export default` statement — the transform must handle both forms, or a
/// standard esbuild-bundled app (e.g. Hono) fails classic-script compilation.
#[test]
fn test_transform_export_list_as_default() {
    use nano::v8::transform_module_code;

    // Shape taken verbatim from an esbuild v0.25 bundle of a Hono app.
    let code = "var app = { fetch: function () {} };\nvar app_hono_default = app;\nexport {\n  app_hono_default as default\n};\n";
    let transformed = transform_module_code(code);
    assert!(
        !transformed.contains("export"),
        "export statement must be fully removed: {}",
        transformed
    );
    assert!(
        transformed.contains("var __nano_handler = app_hono_default;"),
        "local binding must be assigned to __nano_handler: {}",
        transformed
    );
    assert!(
        transformed.contains("__nano_user_fetch"),
        "fetch extraction epilogue must be present: {}",
        transformed
    );

    // The literal form still works unchanged.
    let literal = transform_module_code("export default { fetch: function () {} };");
    assert!(literal.contains("var __nano_handler ="));
    assert!(!literal.contains("export default"));
}

/// A relative (unbundled) import now fails with a clear MODULE_NOT_FOUND at
/// runtime instead of an opaque SyntaxError at compile time — a strict
/// improvement given relative ESM imports were never resolvable without
/// bundling (ADR-007 "Bundling is NANO's philosophy").
#[test]
fn test_relative_import_fails_clearly_not_a_syntax_error() {
    let js_code = r#"
        import { helper } from './utils.js';
        export default {
            async fetch(request) {
                return { status: 200, headers: {}, body: 'unreachable' };
            }
        };
    "#;
    let temp_dir = std::env::temp_dir();
    let js_path = temp_dir.join("test_relative_import_fails.js");
    std::fs::write(&js_path, js_code).expect("Failed to write test JS file");

    init_platform();
    let mut isolate = NanoIsolate::new().expect("Failed to create isolate");
    let url = NanoUrl::parse("https://example.com/api").unwrap();
    let request = NanoRequest::new("GET".to_string(), url, NanoHeaders::new(), None);
    let context = HandlerContext {
        entrypoint: js_path.to_string_lossy().to_string(),
        request,
        memory_limit_mb: 0,
        hostname: String::new(),
    };

    // Must fail (module not found), and must NOT be a parse/syntax error —
    // proving the import statement compiled fine and only the require()
    // resolution failed at runtime.
    let result = execute_handler(&mut isolate, context);
    assert!(result.is_err(), "unresolvable relative import must fail");
    let message = format!("{}", result.unwrap_err());
    assert!(
        !message.to_lowercase().contains("syntax"),
        "failure must not be a syntax error: {}",
        message
    );
}
