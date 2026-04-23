# NANO Examples

Complete examples for common NANO use cases.

## Table of Contents

1. [Basic JavaScript App](#basic-javascript-app)
2. [Using the VFS](#using-the-vfs)
3. [Creating and Running Slivers](#creating-and-running-slivers)
4. [Multi-App Configuration](#multi-app-configuration)
5. [Production Setup](#production-setup)
6. [Admin API Usage](#admin-api-usage)

---

## Basic JavaScript App

### Minimal HTTP Handler

`apps/hello.js`:

```javascript
export default {
  async fetch(request) {
    const url = new URL(request.url);
    
    if (url.pathname === '/') {
      return new Response('Hello from NANO!', {
        headers: { 'Content-Type': 'text/plain' }
      });
    }
    
    if (url.pathname === '/json') {
      return new Response(
        JSON.stringify({ message: 'Hello', time: Date.now() }),
        { headers: { 'Content-Type': 'application/json' } }
      );
    }
    
    return new Response('Not Found', { status: 404 });
  }
};
```

### Configuration

`config.json`:

```json
{
  "server": {
    "host": "0.0.0.0",
    "port": 8080
  },
  "apps": [
    {
      "hostname": "hello.local",
      "entrypoint": "./apps/hello.js",
      "workers": 2,
      "memory_limit_mb": 64,
      "timeout_ms": 30000
    }
  ]
}
```

### Run

```bash
nano-rs run --config config.json

# Test
curl http://hello.local:8080/
curl http://hello.local:8080/json
```

---

## Serving Static Files from Sliver Mode

In sliver mode (`--sliver`), **ALL requests route through your JavaScript handler**. Static files in the VFS must be served by your JS code.

### Static File Handler Example

`apps/static-server.js`:

```javascript
// MIME type mapping
const MIME_TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.js': 'application/javascript; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.svg': 'image/svg+xml; charset=utf-8',
  '.ico': 'image/x-icon',
};

function getMimeType(path) {
  const ext = path.substring(path.lastIndexOf('.')).toLowerCase();
  return MIME_TYPES[ext] || 'application/octet-stream';
}

export default {
  async fetch(request) {
    const url = new URL(request.url);
    let path = url.pathname;
    
    // Default to index.html for directories
    if (path.endsWith('/')) {
      path += 'index.html';
    }
    
    // Try to serve from VFS
    try {
      const content = await Nano.vfs.readFile(path);
      return new Response(content, {
        headers: { 
          'Content-Type': getMimeType(path),
          'Cache-Control': 'public, max-age=3600'
        }
      });
    } catch (err) {
      // File not found in VFS - could be dynamic route
      if (path === '/api/data') {
        return new Response(
          JSON.stringify({ message: 'Dynamic API response' }),
          { headers: { 'Content-Type': 'application/json' } }
        );
      }
      
      // 404 for everything else
      return new Response('Not Found', { status: 404 });
    }
  }
};
```

### Sliver Creation with Static Files

```bash
# Your app directory structure:
# my-app/
#   ├── index.js          (handler above)
#   ├── index.html        (static HTML)
#   ├── style.css         (static CSS)
#   └── assets/
#       └── logo.png

# Create sliver from directory
cd my-app
nano-rs sliver create app.example.com --output app.sliver

# Run with JS execution
nano-rs run --sliver app.sliver --workers 4

# All requests go through JS:
curl http://app.example.com:8080/          # → JS serves index.html from VFS
curl http://app.example.com:8080/style.css  # → JS serves CSS from VFS
curl http://app.example.com:8080/api/data  # → JS returns dynamic JSON
```

### Important Notes

- **Pure WinterCG model**: Unlike traditional web servers, NANO doesn't have a separate "static file server"
- **Your JS is the router**: Every request hits your `fetch()` handler first
- **VFS access via `Nano.vfs`**: Use the WinterCG-compatible VFS API to read files
- **Performance**: Each static file request creates a JS context and executes your handler (~5ms overhead). For high-traffic static assets, see BACKLOG.md for planned "hybrid mode" optimization.

---

## Using the VFS

### Example 1: Session Store (Ephemeral)

`apps/session.js`:

```javascript
export default {
  async fetch(request) {
    const url = new URL(request.url);
    const sessionId = url.searchParams.get('id') || 'default';
    
    // Store session data in ephemeral VFS
    if (url.pathname === '/set') {
      const data = await request.text();
      await Nano.fs.writeFile(`/sessions/${sessionId}.json`, data);
      return new Response(`Session ${sessionId} saved`);
    }
    
    // Retrieve session
    if (url.pathname === '/get') {
      try {
        const data = await Nano.fs.readFile(`/sessions/${sessionId}.json`, 'utf8');
        return new Response(data, { 
          headers: { 'Content-Type': 'application/json' } 
        });
      } catch (err) {
        if (err.code === 'ENOENT') {
          return new Response('Session not found', { status: 404 });
        }
        throw err;
      }
    }
    
    // Delete session
    if (url.pathname === '/delete') {
      try {
        await Nano.fs.delete(`/sessions/${sessionId}.json`);
        return new Response(`Session ${sessionId} deleted`);
      } catch (err) {
        if (err.code === 'ENOENT') {
          return new Response('Session not found', { status: 404 });
        }
        throw err;
      }
    }
    
    return new Response('Usage: /set, /get, /delete?id=<session>');
  }
};
```

### Example 2: File Upload Handler

`apps/upload.js`:

```javascript
export default {
  async fetch(request) {
    if (request.method !== 'POST') {
      return new Response('POST required', { status: 405 });
    }
    
    const contentLength = parseInt(request.headers.get('Content-Length') || '0');
    const maxSize = 10 * 1024 * 1024; // 10MB limit
    
    if (contentLength > maxSize) {
      return new Response('File too large (max 10MB)', { status: 413 });
    }
    
    const formData = await request.formData();
    const file = formData.get('file');
    
    if (!file) {
      return new Response('No file provided', { status: 400 });
    }
    
    const arrayBuffer = await file.arrayBuffer();
    const uint8Array = new Uint8Array(arrayBuffer);
    
    try {
      await Nano.fs.writeFile(`/uploads/${file.name}`, uint8Array);
      return new Response(`Uploaded ${file.name} (${uint8Array.length} bytes)`);
    } catch (err) {
      if (err.code === 'EQUOTA') {
        return new Response('Storage quota exceeded', { status: 507 });
      }
      throw err;
    }
  }
};
```

### Example 3: Node.js Compatible Code

`apps/legacy.js`:

```javascript
// Using Node.js fs polyfill (routes to VFS)
const fs = require('fs');

export default {
  async fetch(request) {
    const url = new URL(request.url);
    
    if (url.pathname === '/save') {
      const data = await request.text();
      // This writes to VFS, not real filesystem
      fs.writeFileSync('/data/note.txt', data);
      return new Response('Saved to VFS');
    }
    
    if (url.pathname === '/load') {
      try {
        // Reading from VFS
        const data = fs.readFileSync('/data/note.txt', 'utf8');
        return new Response(data);
      } catch (err) {
        if (err.code === 'ENOENT') {
          return new Response('File not found', { status: 404 });
        }
        throw err;
      }
    }
    
    return new Response('Usage: POST /save, GET /load');
  }
};
```

### Persistent VFS Configuration

```json
{
  "apps": [{
    "hostname": "storage.local",
    "entrypoint": "./apps/storage.js",
    "vfs_backend": "disk",
    "vfs_config": {
      "path": "/var/lib/nano/storage-app",
      "create_if_missing": true
    },
    "vfs_limits": {
      "max_file_size_mb": 50,
      "max_total_storage_mb": 500
    }
  }]
}
```

---

## Creating and Running Slivers

### Development Workflow

```bash
# 1. Start with config-based app
nano-rs run --config dev.json

# 2. Test the app
curl http://api.local:8080/test

# 3. Create sliver from running app
nano-rs sliver create api.local --output api-v1.sliver --name api-prod

# 4. List slivers
nano-rs sliver list
# NAME       HOSTNAME      CREATED              SIZE
# api-prod   api.local     2026-04-19T20:00:00  2.4MB

# 5. Run from sliver (JavaScript handler executed for all requests)
nano-rs run --sliver api-v1.sliver --workers 4
# Output: "ALL requests route through JavaScript (WinterCG fetch handler)"
# The sliver's JS code handles every HTTP request via its fetch() export

# 6. Or use in production config
cat > prod.json << 'EOF'
{
  "apps": [{
    "hostname": "api.example.com",
    "sliver": "./api-v1.sliver",
    "workers": 8,
    "memory_limit_mb": 128
  }]
}
EOF

nano-rs run --config prod.json
```

### Sliver from Directory

```bash
# Package a directory as sliver (no running app needed)
nano-rs sliver pack ./my-app --output my-app-v1.sliver

# Inspect contents
nano-rs sliver inspect my-app-v1.sliver
# Metadata:
#   Hostname: auto-generated
#   Created: 2026-04-19T20:00:00
#   Version: 1.0
# Contents:
#   - index.js
#   - package.json
#   - assets/logo.png
```

### Sliver Migration

```bash
# On server A
nano-rs sliver create api.local --output api.sliver
gzip api.sliver
scp api.sliver.gz server-b:/tmp/

# On server B
gunzip /tmp/api.sliver.gz
nano-rs run --sliver /tmp/api.sliver --workers 4
```

---

## Multi-App Configuration

### Multiple Apps with Different Backends

```json
{
  "server": {
    "host": "0.0.0.0",
    "port": 8080,
    "admin_port": 8889,
    "admin_key": "secret-key-here"
  },
  "apps": [
    {
      "hostname": "api.example.com",
      "sliver": "./api-v1.sliver",
      "workers": 8,
      "memory_limit_mb": 128,
      "timeout_ms": 30000,
      "env": {
        "API_KEY": "prod-key",
        "LOG_LEVEL": "info"
      }
    },
    {
      "hostname": "upload.example.com",
      "entrypoint": "./apps/upload.js",
      "workers": 4,
      "memory_limit_mb": 256,
      "vfs_backend": "disk",
      "vfs_config": {
        "path": "/var/lib/nano/uploads"
      },
      "vfs_limits": {
        "max_file_size_mb": 100,
        "max_total_storage_mb": 1000
      }
    },
    {
      "hostname": "static.example.com",
      "entrypoint": "./apps/static.js",
      "workers": 2,
      "memory_limit_mb": 64
    }
  ]
}
```

### Virtual Host Routing

NANO routes by `Host` header:

```bash
# Each hostname routes to different app
curl -H "Host: api.example.com" http://localhost:8080/users
curl -H "Host: upload.example.com" http://localhost:8080/upload
curl -H "Host: static.example.com" http://localhost:8080/index.html
```

---

## Production Setup

### Systemd Service

`/etc/systemd/system/nano.service`:

```ini
[Unit]
Description=NANO Edge Runtime
After=network.target

[Service]
Type=simple
User=nano
Group=nano
WorkingDirectory=/opt/nano
ExecStart=/usr/local/bin/nano-rs run --config /etc/nano/config.json
Restart=always
RestartSec=5

# Resource limits
LimitNOFILE=65536
LimitNPROC=4096

[Install]
WantedBy=multi-user.target
```

### Directory Structure

```
/opt/nano/
├── apps/              # JavaScript apps
│   ├── api.js
│   └── upload.js
├── slivers/           # Sliver archives
│   ├── api-v1.sliver
│   └── api-v2.sliver
├── data/              # Persistent VFS data (disk backend)
│   └── api-data/
├── config.json        # Production config
└── logs/              # Log files

/etc/nano/
└── config.json        # Alternative config location

/var/lib/nano/         # VFS disk backend storage
├── app1-data/
└── app2-data/
```

### Log Rotation

`/etc/logrotate.d/nano`:

```
/opt/nano/logs/*.log {
  daily
  rotate 30
  compress
  delaycompress
  missingok
  notifempty
  create 0644 nano nano
  sharedscripts
  postrotate
    systemctl reload nano
  endscript
}
```

---

## Admin API Usage

### Health Check

```bash
curl -H "X-Admin-Key: secret-key" http://localhost:8889/admin/health
```

### List Isolates

```bash
curl -H "X-Admin-Key: secret-key" http://localhost:8889/admin/isolates
```

### Metrics

```bash
# Prometheus format
curl -H "X-Admin-Key: secret-key" http://localhost:8889/admin/metrics

# Or for specific app
curl -H "X-Admin-Key: secret-key" http://localhost:8889/admin/metrics?app=api.example.com
```

### Reload Config

```bash
# Trigger hot-reload (graceful, no downtime)
curl -X POST -H "X-Admin-Key: secret-key" http://localhost:8889/admin/reload
```

### Unix Socket (Local Only)

```bash
# Access via Unix socket (no auth required for local)
curl --unix-socket /var/run/nano/control.sock http://localhost/admin/health
```

---

## Testing

### Unit Test Example

```javascript
// Test your app with fetch
async function test() {
  const request = new Request('http://test.local/', {
    method: 'POST',
    body: 'test data'
  });
  
  const response = await app.fetch(request);
  console.assert(response.status === 200);
  
  const body = await response.text();
  console.assert(body.includes('success'));
}

test();
```

### Load Test

```bash
# Using wrk
wrk -t4 -c100 -d30s http://api.local:8080/test

# Using ab
ab -n 10000 -c 100 http://api.local:8080/test
```

---

## Troubleshooting

### Check App Logs

```bash
# View structured logs
journalctl -u nano -f

# Or log file
tail -f /opt/nano/logs/nano.log | jq '.event, .hostname, .message'
```

### Debug VFS Issues

```javascript
// Add to your app for debugging
console.log('VFS debug:', {
  path: '/data/config.json',
  exists: await Nano.fs.exists('/data/config.json'),
  // Read and log content
  content: await Nano.fs.readFile('/data/config.json').catch(e => e.code)
});
```

### Sliver Issues

```bash
# Verify sliver integrity
nano-rs sliver inspect my-app.sliver

# Check if it's a valid tar
tar -tf my-app.sliver

# Extract and inspect manually
tar -xf my-app.sliver -C /tmp/inspect/
cat /tmp/inspect/meta.json
```

---

## More Resources

- [ARCHITECTURE.md](ARCHITECTURE.md) — Internal design
- [VFS.md](VFS.md) — Virtual filesystem detailed guide
- [SLIVER.md](SLIVER.md) — Sliver documentation
- [README.md](README.md) — Quick start

---

*Generated for NANO v1.1*
