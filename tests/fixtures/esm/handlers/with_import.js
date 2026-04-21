// ESM handler with relative import
import { greet } from '../utils/helper.js';

export default {
    fetch(request) {
        const message = greet("ESM");
        return new Response(message, {
            status: 200,
            headers: { "Content-Type": "text/plain" }
        });
    }
};
