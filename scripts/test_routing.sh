#!/bin/bash
# Test virtual host routing for NANO HTTP server
# This script starts the server and tests different Host header routing

set -e  # Exit on error

echo "=========================================="
echo "NANO Virtual Host Routing Test"
echo "=========================================="

# Build release binary first
echo -e "\n[1/6] Building release binary..."
cargo build --release --quiet

# Start server in background
echo -e "\n[2/6] Starting NANO server..."
./target/release/nano-rs &
SERVER_PID=$!

# Wait for server to start
sleep 2

# Function to cleanup server on exit
cleanup() {
    echo -e "\n[6/6] Cleaning up..."
    if kill -0 $SERVER_PID 2>/dev/null; then
        kill $SERVER_PID 2>/dev/null
        wait $SERVER_PID 2>/dev/null || true
    fi
    echo "Server stopped"
}
trap cleanup EXIT

echo "Server started (PID: $SERVER_PID)"

# Test 1: Health endpoint
echo -e "\n[3/6] Testing health endpoint..."
echo "Request: GET /health"
HEALTH_RESPONSE=$(curl -s -o /dev/null -w "%{http_code}" http://localhost:8080/health)
if [ "$HEALTH_RESPONSE" = "200" ]; then
    echo "✓ Health endpoint returns 200 OK"
else
    echo "✗ Health endpoint failed with status: $HEALTH_RESPONSE"
    exit 1
fi

# Test 2: api.example.com routing
echo -e "\n[4/6] Testing virtual host routing..."
echo "Request: GET / with Host: api.example.com"
API_RESPONSE=$(curl -s -H "Host: api.example.com" http://localhost:8080/)
if echo "$API_RESPONSE" | grep -q "API Handler"; then
    echo "✓ api.example.com routes to API Handler"
    echo "  Response: $API_RESPONSE"
else
    echo "✗ api.example.com routing failed"
    echo "  Response: $API_RESPONSE"
    exit 1
fi

# Test 3: blog.example.com routing
echo "Request: GET / with Host: blog.example.com"
BLOG_RESPONSE=$(curl -s -H "Host: blog.example.com" http://localhost:8080/)
if echo "$BLOG_RESPONSE" | grep -q "Blog Handler"; then
    echo "✓ blog.example.com routes to Blog Handler"
    echo "  Response: $BLOG_RESPONSE"
else
    echo "✗ blog.example.com routing failed"
    echo "  Response: $BLOG_RESPONSE"
    exit 1
fi

# Test 4: Unknown host fallback
echo "Request: GET / with Host: unknown.example.com"
UNKNOWN_RESPONSE=$(curl -s -H "Host: unknown.example.com" http://localhost:8080/)
if echo "$UNKNOWN_RESPONSE" | grep -q "NANO Runtime"; then
    echo "✓ Unknown host falls back to default handler"
    echo "  Response: $UNKNOWN_RESPONSE"
else
    echo "✗ Fallback routing failed"
    echo "  Response: $UNKNOWN_RESPONSE"
    exit 1
fi

# Test 5: Case insensitive routing
echo -e "\n[5/6] Testing case-insensitive routing..."
echo "Request: GET / with Host: API.EXAMPLE.COM (uppercase)"
UPPER_RESPONSE=$(curl -s -H "Host: API.EXAMPLE.COM" http://localhost:8080/)
if echo "$UPPER_RESPONSE" | grep -q "API Handler"; then
    echo "✓ Case-insensitive routing works (API.EXAMPLE.COM -> API Handler)"
    echo "  Response: $UPPER_RESPONSE"
else
    echo "✗ Case-insensitive routing failed"
    echo "  Response: $UPPER_RESPONSE"
    exit 1
fi

echo -e "\n=========================================="
echo "All tests passed! ✓"
echo "=========================================="
echo ""
echo "Summary:"
echo "  - Health endpoint: OK"
echo "  - Virtual host routing: OK"
echo "  - Fallback handling: OK"
echo "  - Case-insensitive matching: OK"
echo ""
echo "The HTTP server correctly routes requests based on Host header."
