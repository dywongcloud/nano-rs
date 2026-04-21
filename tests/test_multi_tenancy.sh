#!/bin/bash
#
# Sliver Multi-Tenancy Test Script
#
# Demonstrates the two operational modes:
# 1. Strict mode (default) - 404 for wrong hostname
# 2. Permissive mode (--static) - serve to any host
#

set -e
set -u

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
NANO_BIN="${1:-$SCRIPT_DIR/../target/release/nano-rs}"
TEST_DIR=$(mktemp -d)

section() {
    echo ""
    echo -e "${BLUE}========================================${NC}"
    echo -e "${BLUE}$1${NC}"
    echo -e "${BLUE}========================================${NC}"
}

cleanup() {
    rm -rf "$TEST_DIR"
}
trap cleanup EXIT

section "Creating Test Sliver"

# Create minimal test app
mkdir -p "$TEST_DIR/app"
echo '<!DOCTYPE html><html><body><h1>Hello from NANO</h1></body></html>' > "$TEST_DIR/app/index.html"
echo 'console.log("App loaded");' > "$TEST_DIR/app/app.js"

cd "$TEST_DIR/app"
$NANO_BIN sliver create test.example.com --name test-app --tag v1.0 2>&1 | tail -5

section "Testing STRICT Mode (Default)"

echo -e "${YELLOW}Command:${NC}"
echo "  nano-rs run --sliver ./test-app-v1.0.sliver"
echo ""
echo -e "${YELLOW}Behavior:${NC}"
echo "  - Exact hostname match: test.example.com"
echo "  - Wrong hostname: HTTP 404"
echo ""
echo "  # This returns 404 (wrong Host header):"
echo "  curl http://localhost:8080/"
echo ""
echo "  # This works (correct Host header):"
echo "  curl -H 'Host: test.example.com' http://localhost:8080/"
echo ""

section "Testing PERMISSIVE Mode (--static)"

echo -e "${YELLOW}Command:${NC}"
echo "  nano-rs run --sliver ./test-app-v1.0.sliver --static"
echo ""
echo -e "${YELLOW}Behavior:${NC}"
echo "  - ANY hostname gets VFS files"
echo "  - Good for local development"
echo ""
echo "  # These all work:"
echo "  curl http://localhost:8080/"
echo "  curl http://127.0.0.1:8080/"
echo "  curl http://0.0.0.0:8080/"
echo ""

section "Key Differences"

cat << 'EOF'
┌─────────────────┬──────────────────────┬──────────────────────┐
│ Feature         │ Strict (default)     │ Permissive (--static)│
├─────────────────┼──────────────────────┼──────────────────────┤
│ Host check      │ Exact match required │ Any host accepted    │
│ Wrong host      │ HTTP 404             │ VFS served           │
│ Edge functions  │ ✅ Recommended       │ Not recommended      │
│ Local dev       │ Need /etc/hosts      ✅ Works out of box     │
│ Multi-tenant    │ ✅ Safe isolation    │ Shared VFS           │
│ Production      │ ✅ Secure default    │ Use with care        │
└─────────────────┴──────────────────────┴──────────────────────┘
EOF

section "Testing Commands"

echo "Testing with created sliver: $TEST_DIR/app/test-app-v1.0.sliver"
echo ""

# Show what a strict mode response looks like (without running server)
echo -e "${GREEN}✓ Sliver created at:${NC}"
ls -lh "$TEST_DIR/app/test-app-v1.0.sliver"
echo ""

echo -e "${YELLOW}To test strict mode:${NC}"
echo "  1. Open terminal 1:"
echo "     cd $TEST_DIR/app"
echo "     $NANO_BIN run --sliver ./test-app-v1.0.sliver"
echo ""
echo "  2. Open terminal 2:"
echo "     curl -H 'Host: test.example.com' http://localhost:8080/  # ✅ Works"
echo "     curl http://localhost:8080/                            # ❌ 404"
echo ""

echo -e "${YELLOW}To test permissive mode:${NC}"
echo "  1. Open terminal 1:"
echo "     cd $TEST_DIR/app"
echo "     $NANO_BIN run --sliver ./test-app-v1.0.sliver --static"
echo ""
echo "  2. Open terminal 2:"
echo "     curl http://localhost:8080/  # ✅ Works with any host"
echo ""

echo -e "${GREEN}Test complete!${NC}"
