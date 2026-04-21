// ESM handler with export default { fetch } pattern
export default {
    async fetch(request) {
        return new Response(`Hello from ESM! Method: ${request.method}`, {
            status: 200,
            headers: { "Content-Type": "text/plain" }
        });
    }
};
