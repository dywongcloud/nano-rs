# Sliver Format Specification v1.0

## Overview

Sliver is a tar-based archive format for JavaScript isolate snapshots in NANO. It captures the complete state of a V8 isolate including the heap snapshot and virtual filesystem contents, enabling fast cold starts and migration between NANO instances.

## Design Goals

- **Simplicity**: Standard tar format, inspectable with standard tools
- **Opacity**: V8 heap is an opaque blob, never parsed by NANO
- **Portability**: No host-specific paths or identifiers
- **Evolvability**: Format supports future extensions (deltas, compression)

## Archive Structure

```
app-v1.sliver (tar archive)
├── meta.json          # Required: JSON metadata
├── heap.bin           # Required: V8 heap snapshot (opaque)
├── vfs/               # Optional: Virtual filesystem
│   ├── data/
│   │   └── config.json
│   └── assets/
│       └── logo.png
└── manifest.txt       # Generated: Human-readable listing
```

## File Specifications

### meta.json (Required)

JSON metadata describing the snapshot:

```json
{
  "hostname": "app.example.com",
  "created_at": "2026-04-20T12:34:56.789Z",
  "format_version": "1.0",
  "nano_version": "1.1.0",
  "description": "Production deployment v2.3.1",
  "custom": {
    "deployment": "production",
    "git_sha": "abc123"
  }
}
```

**Fields:**
- `hostname` (string, required): Virtual hostname for the app
- `created_at` (string, required): ISO 8601 timestamp
- `format_version` (string, required): Sliver format version (currently "1.0")
- `nano_version` (string, required): NANO runtime version that created the snapshot
- `description` (string, optional): Human-readable description
- `custom` (object, optional): Application-specific key-value pairs

### heap.bin (Required)

Opaque binary blob containing the V8 heap snapshot. Created by V8's SnapshotCreator API.

**Important:** The contents of heap.bin are version-specific to the V8 version used. NANO treats this as an opaque blob and does not parse or interpret its contents.

**Size:** Variable (typically 100KB - 10MB depending on isolate state)

**Format:** V8-specific binary format, not documented here.

### vfs/ (Optional)

Virtual filesystem contents stored under the `vfs/` prefix. Each file becomes a tar entry with the full path preserved.

**Path Format:**
- All paths use forward slashes (`/`) regardless of platform
- Paths are relative to the VFS root
- Directory structure is preserved

**Example Entry:**
```
Name: vfs/data/config.json
Size: 1234 bytes
Mode: 0644 (rw-r--r--)
MTime: [file modification time or snapshot time]
```

**Content:** Files are stored as-is (binary-safe). No encoding or compression is applied at the file level.

### manifest.txt (Generated)

Human-readable listing of all archive entries. Generated automatically during packing.

```
# Sliver Archive Manifest
# =========================

meta.json
heap.bin
vfs/data/config.json
vfs/assets/logo.png
```

This file is informational only and not used during loading.

## Tar Format Details

The archive uses standard tar format (ustar or GNU tar):

- **Format**: POSIX.1-2001 (pax) or GNU tar format
- **Compression**: None (uncompressed tar)
- **Encoding**: UTF-8 for pathnames
- **Checksum**: Standard tar checksum in header

### Entry Headers

Each entry has a tar header with:
- Name: Entry path (e.g., "meta.json", "vfs/data/config.json")
- Size: File size in bytes
- Mode: File permissions (0644 for files, 0755 for directories)
- MTime: Modification time (Unix timestamp)
- Checksum: Header checksum

### Directory Entries

Directories can be represented as:
1. Explicit directory entries (typeflag = '5')
2. Implicit via file paths (parent directories inferred)

NANO supports both approaches during unpacking.

## Format Versioning

### Current Version: 1.0

Version 1.0 defines the basic structure:
- Required: meta.json, heap.bin
- Optional: vfs/* entries
- No compression
- No delta support

### Version Compatibility

- **Reading**: NANO only supports reading version 1.0 archives
- **Writing**: NANO always writes version 1.0 archives
- **Forward compatibility**: Unknown entries are skipped during reading

### Future Versions

Potential future extensions (not yet implemented):
- **1.1**: Optional compression layer (gzip/brotli)
- **1.2**: Delta/differential snapshots (additive format)
- **2.0**: New heap format or metadata schema changes

## Extension Points

### Compression

While v1.0 does not use compression at the archive level, the format can be extended:

```
app-v1.sliver.gz (gzip compressed tar)
```

The loader would detect compression and decompress before unpacking.

### Deltas

Differential snapshots can be supported by adding special entry types:

```
delta-v1.sliver
├── delta.json        # Delta metadata (base reference, changes)
├── heap.patch        # Binary diff of heap
└── vfs/              # Changed/new files only
```

### Checksums

Per-file checksums can be added via the tar extended header (pax format):

```
30 SCHILY.xattr.checksum=sha256:abc123...
```

## Tooling

### Viewing Contents

```bash
# List archive contents
tar -tf app-v1.sliver

# Extract specific file
tar -xf app-v1.sliver meta.json

# View metadata
tar -xf app-v1.sliver -O meta.json | jq .
```

### Creating Archives

While NANO provides `nano-rs snapshot create`, manual creation is possible:

```bash
# Create archive manually
tar -cf app-v1.sliver meta.json heap.bin vfs/
```

### Validating Archives

```bash
# Verify tar structure
tar -tf app-v1.sliver > /dev/null && echo "Valid tar"

# Check required files exist
tar -tf app-v1.sliver | grep -q "^meta.json$" && echo "Has metadata"
tar -tf app-v1.sliver | grep -q "^heap.bin$" && echo "Has heap"
```

## Security Considerations

### Path Traversal

- All paths in the archive are treated as relative to VFS root
- Path traversal attempts ("../") in archive entries are rejected
- Absolute paths (starting with "/") are normalized to relative

### File Permissions

- Permissions from the archive are informational only
- Access control is enforced by NANO's VFS layer, not tar mode bits
- Executable bits are preserved but not directly used

### Size Limits

- Maximum file size: Enforced by VFS ResourceLimits
- Maximum archive size: Enforced during streaming read
- Memory limits: Enforced during unpacking

### Validation

Before loading, NANO validates:
1. Tar structure is valid
2. Required files (meta.json, heap.bin) exist
3. Format version is supported
4. Metadata JSON is valid
5. Heap blob is non-empty

## Examples

### Minimal Sliver

```
test.sliver
├── meta.json (96 bytes)
└── heap.bin (1024 bytes)
```

### Full Sliver with VFS

```
production-v1.sliver (245 KB)
├── meta.json (156 bytes)
├── heap.bin (241,234 bytes)
├── manifest.txt (89 bytes)
└── vfs/
    ├── data/
    │   └── session-store.json (1,234 bytes)
    └── cache/
        └── precomputed.html (2,456 bytes)
```

### Metadata Example

```json
{
  "hostname": "api.example.com",
  "created_at": "2026-04-20T14:30:00.000Z",
  "format_version": "1.0",
  "nano_version": "1.1.0",
  "description": "API server deployment with session state",
  "custom": {
    "deployment_id": "deploy-2026-04-20-001",
    "git_sha": "a1b2c3d4",
    "environment": "production"
  }
}
```

## References

- [Tar Format (POSIX)](https://pubs.opengroup.org/onlinepubs/9699919799/utilities/pax.html)
- [V8 SnapshotCreator API](https://v8.github.io/api/head/classv8_1_1SnapshotCreator.html)
- [NANO VFS Documentation](../vfs/)

---

*Specification Version: 1.0*  
*Last Updated: 2026-04-20*
