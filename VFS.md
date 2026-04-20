# NANO VFS — Virtual File System

The NANO Virtual File System (VFS) provides per-isolate filesystem storage with pluggable backends and Node.js compatibility.

## Overview

Each JavaScript isolate running in NANO has its own isolated filesystem. This filesystem:
- Is **ephemeral by default** — data persists only for the lifetime of the isolate
- Can use **pluggable backends** — memory, disk, or S3-compatible storage
- Is accessible via **two APIs** — `Nano.fs.*` (explicit) and `require('fs')` (Node.js compatible)
- Has **resource limits** — enforced per isolate to prevent abuse

## Quick Start

```javascript
// Using explicit NANO API
const data = await Nano.fs.readFile('/data/config.json', 'utf8');
await Nano.fs.writeFile('/data/output.txt', 'Hello, World!');

// Using Node.js compatible API
const fs = require('fs');
fs.writeFileSync('/data/output.txt', 'Hello, World!');
const data = fs.readFileSync('/data/config.json', 'utf8');
```

## API Reference

### Nano.fs.* (Explicit API)

Async methods returning Promises:

```javascript
// Read file
const content = await Nano.fs.readFile('/path/to/file', 'utf8');
const binary = await Nano.fs.readFile('/path/to/file'); // Returns Uint8Array

// Write file
await Nano.fs.writeFile('/path/to/file', 'content');
await Nano.fs.writeFile('/path/to/file', uint8Array); // Binary data

// Check existence
const exists = await Nano.fs.exists('/path/to/file'); // boolean

// Delete file
await Nano.fs.delete('/path/to/file');
```

### require('fs') (Node.js Polyfill)

Sync and async methods:

```javascript
const fs = require('fs');

// Synchronous (blocking)
const content = fs.readFileSync('/path/to/file', 'utf8');
fs.writeFileSync('/path/to/file', 'content');
const exists = fs.existsSync('/path/to/file');
fs.unlinkSync('/path/to/file');

// Asynchronous (callback-based)
fs.readFile('/path/to/file', 'utf8', (err, data) => {
  if (err) throw err;
  console.log(data);
});

fs.writeFile('/path/to/file', 'content', (err) => {
  if (err) throw err;
  console.log('Written');
});
```

## What is Ephemeral vs Persistent

### Ephemeral (Default)

By default, VFS uses the **memory backend**. Data exists only while the isolate is running:

```javascript
// App creates file
await Nano.fs.writeFile('/data/session.json', JSON.stringify({user: 'alice'}));

// File exists while isolate runs
const data = await Nano.fs.readFile('/data/session.json');

// When isolate terminates (request complete, timeout, etc.):
// - File is lost
// - Next request starts with empty VFS
```

**Use case:** Session data, temporary caches, intermediate computation results.

### Persistent (Disk Backend)

Configure disk backend for data that survives restarts:

```json
{
  "apps": [{
    "hostname": "api.example.com",
    "entrypoint": "./app.js",
    "vfs_backend": "disk",
    "vfs_config": {
      "path": "/var/lib/nano/api-data"
    }
  }]
}
```

```javascript
// File written to disk
await Nano.fs.writeFile('/data/users.json', '[]');

// Survives isolate termination
// Survives NANO restart
// Other isolates for same app see the same data
```

**Use case:** User databases, configuration files, persistent state.

### S3 Backend (Cloud Storage)

For distributed deployments:

```json
{
  "apps": [{
    "hostname": "api.example.com",
    "vfs_backend": "s3",
    "vfs_config": {
      "endpoint": "https://s3.amazonaws.com",
      "bucket": "my-nano-data",
      "region": "us-east-1",
      "access_key": "...",
      "secret_key": "..."
    }
  }]
}
```

**Use case:** Multi-instance deployments, data sharing across edge nodes.

## Pluggable Backends

### Available Backends

| Backend | Type | Persistence | Use Case |
|---------|------|-------------|----------|
| `memory` | In-memory | Ephemeral | Development, temporary data |
| `disk` | Filesystem | Persistent | Production single-node |
| `s3` | Object storage | Persistent | Production multi-node |

### Backend Selection

**Per-app in config:**
```json
{
  "apps": [{
    "hostname": "api.example.com",
    "vfs_backend": "disk",
    "vfs_config": {
      "path": "/var/lib/nano/data"
    }
  }]
}
```

**Default (memory):**
```json
{
  "apps": [{
    "hostname": "api.example.com"
    // No vfs_backend specified → uses memory
  }]
}
```

### Backend Configuration Options

**Memory backend:**
```json
{
  "vfs_backend": "memory"
  // No config needed
}
```

**Disk backend:**
```json
{
  "vfs_backend": "disk",
  "vfs_config": {
    "path": "/var/lib/nano/data",     // Base directory
    "create_if_missing": true,         // Create dirs if not exist
    "sync_on_write": false             // fsync after each write
  }
}
```

**S3 backend:**
```json
{
  "vfs_backend": "s3",
  "vfs_config": {
    "endpoint": "https://s3.amazonaws.com",
    "bucket": "my-bucket",
    "region": "us-east-1",
    "access_key": "AKIA...",
    "secret_key": "...",
    "prefix": "nano-apps/api/"         // Optional path prefix
  }
}
```

## Resource Limits

### Default Limits (Per Isolate)

| Limit | Value | Description |
|-------|-------|-------------|
| Max file size | 10 MB | Single file cannot exceed this |
| Max total storage | 100 MB | All files combined per isolate |
| Max file count | 1,000 | Number of files per isolate |
| Max path length | 4,096 bytes | Path string length limit |

### Configuring Limits

**Global defaults (server-wide):**
```json
{
  "server": {
    "vfs_limits": {
      "max_file_size_mb": 10,
      "max_total_storage_mb": 100,
      "max_file_count": 1000
    }
  }
}
```

**Per-app override:**
```json
{
  "apps": [{
    "hostname": "file-api.example.com",
    "vfs_limits": {
      "max_file_size_mb": 50,
      "max_total_storage_mb": 500,
      "max_file_count": 5000
    }
  }]
}
```

### Limit Exceeded Behavior

When limits are exceeded, operations throw errors:

```javascript
try {
  // File exceeds max_file_size
  await Nano.fs.writeFile('/data/huge.bin', hugeBuffer); // Throws EQUOTA
} catch (err) {
  console.log(err.code); // 'EQUOTA'
  console.log(err.message); // 'File size exceeds limit'
}

try {
  // Too many files
  for (let i = 0; i < 10000; i++) {
    await Nano.fs.writeFile(`/data/file${i}.txt`, 'x'); // Throws EQUOTA at 1001
  }
} catch (err) {
  console.log(err.code); // 'EQUOTA'
}
```

## Security

### Path Traversal Prevention

NANO prevents directory traversal attacks:

```javascript
// These all throw EACCES (permission denied)
await Nano.fs.readFile('../etc/passwd');
await Nano.fs.readFile('/etc/passwd');
await Nano.fs.readFile('data/../../../etc/passwd');

// Valid paths
await Nano.fs.readFile('/data/config.json');
await Nano.fs.readFile('data/config.json'); // Relative to VFS root
```

### Namespace Isolation

Each isolate has its own namespace:

```javascript
// Isolate A (hostname: api-a.example.com)
await Nano.fs.writeFile('/data/secret.txt', 'A');

// Isolate B (hostname: api-b.example.com)
// Cannot read A's files
const data = await Nano.fs.readFile('/data/secret.txt'); // Throws ENOENT
```

Even with the same backend (disk or S3), isolates cannot access each other's data.

### Error Codes

NANO uses Node.js-compatible error codes:

| Code | Meaning | HTTP Equivalent |
|------|---------|-----------------|
| ENOENT | File not found | 404 |
| EACCES | Permission denied | 403 |
| EEXIST | File already exists | 409 |
| EINVAL | Invalid argument | 400 |
| EQUOTA | Quota exceeded | 507 |
| EIO | I/O error | 500 |

## Code Examples

### Session Store

```javascript
// Simple session store using ephemeral VFS
export default {
  async fetch(request) {
    const url = new URL(request.url);
    const sessionId = url.searchParams.get('session');
    
    if (url.pathname === '/set') {
      const data = await request.text();
      await Nano.fs.writeFile(`/sessions/${sessionId}.json`, data);
      return new Response('Session saved');
    }
    
    if (url.pathname === '/get') {
      try {
        const data = await Nano.fs.readFile(`/sessions/${sessionId}.json`);
        return new Response(data);
      } catch (err) {
        if (err.code === 'ENOENT') {
          return new Response('Session not found', { status: 404 });
        }
        throw err;
      }
    }
  }
};
```

### File Upload Handler

```javascript
// Handle file uploads with size limits
export default {
  async fetch(request) {
    if (request.method !== 'POST') {
      return new Response('Method not allowed', { status: 405 });
    }
    
    const contentLength = request.headers.get('Content-Length');
    const maxSize = 10 * 1024 * 1024; // 10MB
    
    if (contentLength && parseInt(contentLength) > maxSize) {
      return new Response('File too large', { status: 413 });
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

### Persistent Database (JSON)

```javascript
// Simple JSON database using persistent disk backend
// Configure with: vfs_backend: 'disk'

const DB_PATH = '/data/users.json';

async function readDB() {
  try {
    const data = await Nano.fs.readFile(DB_PATH, 'utf8');
    return JSON.parse(data);
  } catch (err) {
    if (err.code === 'ENOENT') {
      return [];
    }
    throw err;
  }
}

async function writeDB(users) {
  await Nano.fs.writeFile(DB_PATH, JSON.stringify(users, null, 2));
}

export default {
  async fetch(request) {
    const url = new URL(request.url);
    const users = await readDB();
    
    if (url.pathname === '/users' && request.method === 'GET') {
      return new Response(JSON.stringify(users), {
        headers: { 'Content-Type': 'application/json' }
      });
    }
    
    if (url.pathname === '/users' && request.method === 'POST') {
      const newUser = await request.json();
      newUser.id = Date.now();
      users.push(newUser);
      await writeDB(users);
      return new Response(JSON.stringify(newUser), {
        status: 201,
        headers: { 'Content-Type': 'application/json' }
      });
    }
    
    return new Response('Not found', { status: 404 });
  }
};
```

## Architecture

```
JavaScript Code
    ↓
Nano.fs.readFile() or require('fs').readFileSync()
    ↓
VFS Bindings (src/runtime/vfs_bindings.rs)
    ↓
IsolateVfs (src/vfs/isolate.rs)
    ↓
Namespace Resolution
    ↓
VfsBackend trait
    ↓
    ┌─────────────┬─────────────┬─────────────┐
    ↓             ↓             ↓
MemoryBackend  DiskBackend    S3Backend
(DashMap)      (tokio::fs)    (rusoto_s3)
```

## VFS in Slivers

When you create a sliver, the entire VFS state is captured and restored.

### What's Captured

1. **File contents** — All files in the VFS are serialized
2. **Directory structure** — Hierarchy is preserved
3. **Metadata** — Timestamps and entry types

### What's Not Captured

- **Backend configuration** — S3 credentials, disk paths are not stored
- **Runtime state** — File handles, open streams
- **Per-request data** — Temp files created during request handling

### Capture Process

```
VFS (in-memory)
     ↓
[1] Walk directory tree recursively
[2] Read file contents
[3] Store in tar: vfs/file1, vfs/dir/file2, ...
```

### Restoration Process

```
Sliver tar
     ↓
[1] Extract vfs/* entries
[2] Write to fresh VFS
[3] Restore complete VFS state
```

### Cross-Instance Migration

VFS state in slivers is **backend-agnostic**. You can:
- Create sliver on Instance A (using memory backend)
- Move sliver file to Instance B
- Restore on Instance B (using disk backend)

The file contents are identical regardless of original backend.

## Best Practices

1. **Use ephemeral VFS for temporary data** — Sessions, caches, intermediates
2. **Use disk/S3 for persistent data** — User data, configs, databases
3. **Handle ENOENT gracefully** — File may not exist, check first or catch
4. **Respect limits** — Design apps within 10MB file / 100MB total limits
5. **Use streaming for large files** — Avoid loading multi-MB files into memory
6. **Namespace paths** — Use `/data/`, `/uploads/`, `/cache/` prefixes
7. **Pre-populate VFS in slivers** — Bundle configs and assets for faster starts

## Troubleshooting

### "ENOENT: File not found"
File doesn't exist. Check path or create it first.

### "EQUOTA: Quota exceeded"
Hit resource limits. Check file size or total usage. Consider S3 backend for larger storage.

### "EACCES: Permission denied"
Path traversal attempt or invalid path. Use relative paths from VFS root.

### Data lost after restart
Using memory backend (default). Switch to disk or S3 for persistence.

---

**See also:**
- [SLIVER.md](SLIVER.md) — Packaging VFS state as snapshots
- [ARCHITECTURE.md](ARCHITECTURE.md) — VFS internal design
