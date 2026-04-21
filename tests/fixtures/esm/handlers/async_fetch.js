// ESM handler with async operations
export default {
    async fetch(request) {
        // Simulate async work
        const data = await Promise.resolve({ 
            message: "Hello from async ESM",
            method: request.method,
            url: request.url
        });
        
        return new Response(JSON.stringify(data), {
            status: 200,
            headers: { "Content-Type": "application/json" }
        });
    }
};
