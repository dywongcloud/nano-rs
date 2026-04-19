// Astro Islands Architecture - Islands pattern
// Demonstrates Astro's partial hydration with interactive islands

export async function fetch(request) {
  const url = new URL(request.url);
  
  if (url.pathname === '/') {
    return new Response(`
<!DOCTYPE html>
<html>
<head>
  <title>Astro Islands Demo</title>
  <style>
    .island { border: 2px solid #4a90d9; padding: 1rem; margin: 1rem 0; }
    .static { background: #f0f0f0; }
    .interactive { background: #e3f2fd; }
  </style>
</head>
<body>
  <h1>Astro Islands Architecture</h1>
  
  <div class="island static">
    <h2>Static Island</h2>
    <p>This content is server-rendered and never hydrated.</p>
  </div>
  
  <div class="island interactive">
    <h2>Interactive Island</h2>
    <p>Would be hydrated with client-side JS in real Astro.</p>
    <button>Click me (simulated)</button>
  </div>
  
  <div class="island">
    <h2>Props Island</h2>
    <p>Server-rendered with props: {message: "Hello from Astro!"}</p>
  </div>
</body>
</html>
    `.trim(), {
      headers: { 'Content-Type': 'text/html' }
    });
  }
  
  if (url.pathname === '/api/islands') {
    return new Response(JSON.stringify({
      framework: 'astro',
      islands: [
        { name: 'Counter', hydration: 'client:load' },
        { name: 'Search', hydration: 'client:idle' },
        { name: 'Comments', hydration: 'client:visible' }
      ],
      partiallyHydrated: true
    }), {
      headers: { 'Content-Type': 'application/json' }
    });
  }
  
  return new Response('Not Found', { status: 404 });
}
