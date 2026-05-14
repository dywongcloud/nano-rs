// Hono.js-style app with middleware pattern simulation
// This mimics how Hono exports when built for edge runtimes

// Simple middleware chain simulation
function loggerMiddleware(handler) {
  return async (request) => {
    const start = performance.now();
    const response = await handler(request);
    const duration = performance.now() - start;
    console.log(`${request.method} ${request.url} - ${response.status} (${duration.toFixed(2)}ms)`);
    return response;
  };
}

function corsMiddleware(handler) {
  return async (request) => {
    const response = await handler(request);
    response.headers.set('Access-Control-Allow-Origin', '*');
    response.headers.set('Access-Control-Allow-Methods', 'GET, POST, PUT, DELETE');
    return response;
  };
}

// Route handlers
async function handleRoot(request) {
  return new Response(JSON.stringify({ 
    message: 'Hello from Hono-style app!',
    framework: 'hono-simulated',
    timestamp: Date.now()
  }), {
    status: 200,
    headers: { 
      'Content-Type': 'application/json',
      'X-Powered-By': 'NANO/Hono-Sim'
    }
  });
}

async function handleAbout(request) {
  return new Response(JSON.stringify({
    app: 'NANO Framework Test',
    version: '1.0.0',
    runtime: 'WinterTC-compatible'
  }), {
    status: 200,
    headers: { 'Content-Type': 'application/json' }
  });
}

async function handleNotFound(request) {
  return new Response(JSON.stringify({
    error: 'Not Found',
    path: new URL(request.url).pathname
  }), {
    status: 404,
    headers: { 'Content-Type': 'application/json' }
  });
}

// Main router simulating Hono's routing
async function router(request) {
  const url = new URL(request.url);
  
  switch (url.pathname) {
    case '/':
      return handleRoot(request);
    case '/about':
      return handleAbout(request);
    default:
      return handleNotFound(request);
  }
}

// Apply middleware chain
const withCors = corsMiddleware(router);
const withLogger = loggerMiddleware(withCors);

// Standard WinterTC export pattern
export default {
  async fetch(request) {
    return withLogger(request);
  }
};
