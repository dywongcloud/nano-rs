// Next.js Static Export Simulation
// Mimics output of `next build` with `output: 'export'`

// Static pages generated at build time
const pages = {
  '/': {
    html: `<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Next.js App - Home</title>
  <style>
    body { font-family: system-ui, sans-serif; margin: 2rem; }
    h1 { color: #0070f3; }
    .container { max-width: 800px; margin: 0 auto; }
  </style>
</head>
<body>
  <div class="container">
    <h1>Welcome to Next.js on NANO</h1>
    <p>This page was statically generated at build time.</p>
    <nav>
      <a href="/">Home</a> |
      <a href="/about">About</a> |
      <a href="/blog/hello-world">Blog Post</a>
    </nav>
    <div id="__next" data-reactroot="">
      <main>
        <h2>Static Site Generation (SSG)</h2>
        <p>Content: ${new Date().toISOString()}</p>
      </main>
    </div>
  </div>
  <script>
    console.log('Next.js app hydrated on NANO');
  </script>
</body>
</html>`,
    css: '/* Next.js global styles */ body { background: #fafafa; }',
    js: 'console.log("Next.js runtime initialized");'
  },
  '/about': {
    html: `<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>Next.js App - About</title>
  <style>
    body { font-family: system-ui, sans-serif; margin: 2rem; }
    .about-section { background: #f0f0f0; padding: 2rem; border-radius: 8px; }
  </style>
</head>
<body>
  <div class="container">
    <h1>About Next.js on NANO</h1>
    <div class="about-section">
      <h2>Static Export</h2>
      <p>This demonstrates Next.js static export working on the NANO edge runtime.</p>
      <ul>
        <li>Pre-rendered HTML at build time</li>
        <li>Zero server-side JavaScript execution per request</li>
        <li>Fast cold starts</li>
        <li>Compatible with WinterCG</li>
      </ul>
    </div>
    <nav>
      <a href="/">← Back to Home</a>
    </nav>
  </div>
</body>
</html>`,
    css: '',
    js: ''
  },
  '/blog/hello-world': {
    html: `<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>Blog: Hello World - Next.js on NANO</title>
  <style>
    article { max-width: 65ch; margin: 0 auto; line-height: 1.6; }
    .meta { color: #666; font-size: 0.9rem; }
  </style>
</head>
<body>
  <article>
    <h1>Hello World</h1>
    <p class="meta">Published on ${new Date().toLocaleDateString()}</p>
    <p>This is a sample blog post demonstrating dynamic route handling in a static export.</p>
    <p>The route <code>/blog/hello-world</code> was pre-rendered at build time.</p>
    <a href="/">← All posts</a>
  </article>
</body>
</html>`,
    css: 'article { font-family: Georgia, serif; }',
    js: ''
  }
};

// Helper to get content type based on file extension
function getContentType(path) {
  if (path.endsWith('.css')) return 'text/css';
  if (path.endsWith('.js')) return 'application/javascript';
  if (path.endsWith('.json')) return 'application/json';
  if (path.endsWith('.png')) return 'image/png';
  if (path.endsWith('.jpg') || path.endsWith('.jpeg')) return 'image/jpeg';
  if (path.endsWith('.svg')) return 'image/svg+xml';
  return 'text/html';
}

export default {
  async fetch(request) {
    const url = new URL(request.url);
    const pathname = url.pathname;
    
    console.log(`[Next.js Static] ${request.method} ${pathname}`);
    
    // Serve static assets by path pattern
    if (pathname.endsWith('.css')) {
      // Extract page path from CSS reference
      const pagePath = pathname.replace('/_next/static/css/', '').replace('.css', '');
      const page = pages[pagePath] || pages['/'];
      if (page.css) {
        return new Response(page.css, {
          status: 200,
          headers: { 
            'Content-Type': 'text/css',
            'Cache-Control': 'public, max-age=31536000, immutable'
          }
        });
      }
    }
    
    if (pathname.endsWith('.js')) {
      const pagePath = pathname.replace('/_next/static/js/', '').replace('.js', '');
      const page = pages[pagePath] || pages['/'];
      if (page.js) {
        return new Response(page.js, {
          status: 200,
          headers: { 
            'Content-Type': 'application/javascript',
            'Cache-Control': 'public, max-age=31536000, immutable'
          }
        });
      }
    }
    
    // Serve HTML pages
    const page = pages[pathname];
    if (page) {
      return new Response(page.html, {
        status: 200,
        headers: { 
          'Content-Type': 'text/html; charset=utf-8',
          'X-Nextjs-Static': 'true',
          'Cache-Control': 'public, max-age=60, stale-while-revalidate=300'
        }
      });
    }
    
    // 404 for unknown paths
    return new Response(`<!DOCTYPE html>
<html>
<head><title>404 - Page Not Found</title></head>
<body>
  <h1>404 - Page Not Found</h1>
  <p>The page <code>${pathname}</code> was not found.</p>
  <p>Available pages: ${Object.keys(pages).join(', ')}</p>
  <a href="/">Go Home</a>
</body>
</html>`, {
      status: 404,
      headers: { 
        'Content-Type': 'text/html; charset=utf-8',
        'X-Nextjs-Static': 'true'
      }
    });
  }
};
