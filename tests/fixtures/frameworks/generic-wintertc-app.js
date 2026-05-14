// Generic WinterTC-compatible application
// No framework — just raw WinterTC APIs

export default {
  async fetch(request) {
    const url = new URL(request.url);
    const method = request.method;
    
    // Log the request
    console.log(`[GenericApp] ${method} ${url.pathname}`);
    
    // Simple routing
    if (url.pathname === '/') {
      // Test various WinterTC APIs
      const data = {
        message: 'Generic WinterTC app',
        timestamp: Date.now(),
        url: request.url,
        method: method,
        headers: {}
      };
      
      // Copy request headers
      request.headers.forEach((value, key) => {
        data.headers[key] = value;
      });
      
      return new Response(JSON.stringify(data, null, 2), {
        status: 200,
        headers: { 
          'Content-Type': 'application/json',
          'X-Generic-App': 'true'
        }
      });
    }
    
    if (url.pathname === '/api/data') {
      // Simulate data fetching/processing
      const randomBytes = new Uint8Array(16);
      crypto.getRandomValues(randomBytes);
      
      const hexString = Array.from(randomBytes)
        .map(b => b.toString(16).padStart(2, '0'))
        .join('');
      
      return new Response(JSON.stringify({
        randomId: hexString,
        generatedAt: performance.now()
      }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' }
      });
    }
    
    if (url.pathname === '/health') {
      return new Response(JSON.stringify({
        status: 'healthy',
        uptime: performance.now()
      }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' }
      });
    }
    
    // 404 for unknown paths
    return new Response(JSON.stringify({
      error: 'Not found',
      path: url.pathname,
      availableRoutes: ['/', '/api/data', '/health']
    }), {
      status: 404,
      headers: { 'Content-Type': 'application/json' }
    });
  }
};
