# Phase 08: Framework Compatibility — Context

**Gathered:** 2026-04-19
**Status:** Ready for planning

<domain>
## Phase Boundary

Popular JavaScript frameworks run without modification on NANO. This phase verifies that Hono.js, Next.js static export, Astro static build, and generic WinterTC apps execute correctly when properly configured for edge deployment.

**Success Criteria (from ROADMAP.md):**
1. Hono.js hello-world app with middleware responds correctly
2. Next.js static export (HTML/CSS/JS assets) serves all files correctly
3. Astro static build (islands architecture) renders and hydrates correctly
4. Generic WinterTC-compliant app (not framework-specific) runs correctly

**In scope:**
- Framework compatibility verification testing
- Static asset handling strategy
- Entry point pattern validation
- Test applications for each target framework

**Out of scope:**
- npm package resolution (user must bundle apps)
- Framework-specific runtime code (frameworks adapt to WinterTC)
- Real filesystem access (VFS/bundle approach only)
- TypeScript/JSX transpilation (user must bundle beforehand)

</domain>

<decisions>
## Implementation Decisions

### Framework Detection Strategy (D-01)
- **No framework detection in runtime**
- NANO does not detect or special-case frameworks
- Follows Cloudflare Workers/Deno Deploy model: runtime executes JS, frameworks adapt to it
- Frameworks export WinterTC-compatible handlers when configured for edge deployment

### Static Asset Serving (D-02)
- **VFS bundle approach**
- All static assets (HTML, CSS, JS, images) must be bundled into the JS entrypoint
- NANO only serves what the JS fetch handler returns
- No filesystem access from runtime (consistent with PROJECT.md constraints)
- Frameworks must bundle static assets or the app must serve them via JS

### Entry Point Pattern (D-03)
- **Cloudflare Workers style only**
- Pattern: `export default { fetch(request) { ... } }`
- Single, standard export pattern
- Consistent with existing Phase 3 D-01 decision
- Frameworks must export this pattern when building for NANO

### Test Strategy (D-04)
- **Minimal test apps approach**
- Hand-written test apps that mimic each framework's export structure
- Fast CI execution, no external dependencies
- Clear failure diagnosis
- Test apps:
  - Hono: `export default { fetch }` with middleware pattern
  - Next.js: static export structure with HTML/CSS/JS bundle
  - Astro: islands architecture with client/server split
  - Generic: plain WinterTC handler without framework

### App Configuration (D-05)
- No special framework config fields needed
- Standard `AppConfig` from Phase 5 sufficient:
  ```json
  {
    "hostname": "app.example.com",
    "entrypoint": "/apps/myapp/bundle.js",
    "limits": { "memory_mb": 128, "timeout_secs": 30, "workers": 4 }
  }
  ```

### the agent's Discretion
- Exact test app code structure
- How to simulate Next.js static export structure
- How to simulate Astro islands hydration
- Test assertion details (status codes, headers, body content)

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Prior Phase Context
- `.planning/phases/03-runtime-apis/03-CONTEXT.md` — Handler interface patterns (D-01)
- `.planning/phases/05-multi-app-hosting/` — App configuration and registry
- `.planning/phases/06-outbound-io/` — fetch() API for framework use
- `.planning/phases/07-production-features/` — Logging for framework debugging

### Requirements
- `.planning/REQUIREMENTS.md` §FRAME-01 through FRAME-04 — Framework compatibility requirements
- `.planning/REQUIREMENTS.md` §Design Principles — WinterTC compliance, no filesystem

### Project Context
- `.planning/PROJECT.md` — Core value, constraints (no npm, no filesystem access)
- `.planning/ROADMAP.md` §Phase 8 — Goal and success criteria
- `.planning/research/STACK.md` — Technology stack

### Framework Documentation
- Hono.js: https://hono.dev/docs/getting-started/basic
- Next.js static export: https://nextjs.org/docs/app/api-reference/cli/next#build
- Astro static build: https://docs.astro.build/en/guides/deploy/
- WinterTC spec: https://wintertc.org/working-group/

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `src/runtime/handler.rs` — Handler execution with `export default { fetch }` pattern
- `src/runtime/apis.rs` — RuntimeAPIs::bind_all() provides WinterTC APIs
- `src/config/app.rs` — AppConfig for app registration (no changes needed)
- `src/http/v8_bridge.rs` — Request/Response serialization for JS handlers

### Established Patterns
- Handler exports: `fetch` function on `export default` object
- Request handling: JSON serialization → V8 parse → handler call
- Response extraction: V8 object → status, headers, body extraction
- Test pattern: V8 isolate with bound APIs, script execution, assertion

### Integration Points
- Tests will create isolates, bind RuntimeAPIs, execute framework-like scripts
- No changes to production code expected (verification phase)
- Test apps placed in `tests/fixtures/frameworks/` or similar

</code_context>

<specifics>
## Specific Ideas

### Hono.js Test App Pattern
```javascript
// Hono-style app export
export default {
  fetch(request) {
    const url = new URL(request.url);
    if (url.pathname === '/') {
      return new Response('Hello from Hono!', { 
        status: 200,
        headers: { 'Content-Type': 'text/plain' }
      });
    }
    return new Response('Not Found', { status: 404 });
  }
};
```

### Next.js Static Export Pattern
```javascript
// Simulating Next.js static export bundle
const pages = {
  '/': { html: '<html>...</html>', css: '...', js: '...' },
  '/about': { html: '...', css: '...', js: '...' }
};

export default {
  fetch(request) {
    const url = new URL(request.url);
    const page = pages[url.pathname];
    if (page) {
      return new Response(page.html, { 
        status: 200,
        headers: { 'Content-Type': 'text/html' }
      });
    }
    return new Response('Not Found', { status: 404 });
  }
};
```

### Astro Islands Pattern
```javascript
// Simulating Astro islands (server + hydrated client components)
export default {
  fetch(request) {
    // Return HTML with island markers
    const html = `
      <html>
        <body>
          <div data-island="counter">Server rendered</div>
          <script>/* hydration code */</script>
        </body>
      </html>
    `;
    return new Response(html, { 
      status: 200,
      headers: { 'Content-Type': 'text/html' }
    });
  }
};
```

</specifics>

<deferred>
## Deferred Ideas

- Real framework npm installs in CI (slow, can add later as extended tests)
- Framework-specific optimization hints (future phase)
- VFS implementation for file serving (Phase 999.2 backlog)
- Framework adapter SDKs (if demand emerges)

</deferred>

---

*Phase: 08-framework-compatibility*
*Context gathered: 2026-04-19*
