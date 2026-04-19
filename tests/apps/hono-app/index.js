// Hono.js style app - Simple REST API
// This demonstrates a lightweight API handler pattern

export default {
  async fetch(request) {
    const url = new URL(request.url);
    
    if (url.pathname === '/api/hello') {
      return new Response(JSON.stringify({
        message: 'Hello from Hono-style API!',
        framework: 'hono-like',
        timestamp: Date.now()
      }), {
        headers: { 'Content-Type': 'application/json' }
      });
    }
    
    if (url.pathname === '/api/users') {
      return new Response(JSON.stringify({
        users: [
          { id: 1, name: 'Alice' },
          { id: 2, name: 'Bob' }
        ],
        app: 'hono-api'
      }), {
        headers: { 'Content-Type': 'application/json' }
      });
    }
    
    return new Response('Not Found', { status: 404 });
  }
};

// Also export fetch function for NANO compatibility
export async function fetch(request) {
  const url = new URL(request.url);
  
  return new Response(JSON.stringify({
    hono: true,
    path: url.pathname,
    method: request.method
  }), {
    headers: { 'Content-Type': 'application/json' }
  });
}
