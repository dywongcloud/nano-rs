# Config Mode: Entrypoint App Support

## Current Status

Config mode **fully supports sliver-based apps** with dedicated `SliverWorkerPool`.

Entrypoint-based apps (non-sliver) use the **existing WorkQueue dispatch** system:
- `WorkQueue` routes requests to the standard `WorkerPool`
- This provides basic functionality for Phase 19.1

## Phase 19.2 Enhancement (Optional)

A dedicated `EntrypointWorkerPool` can be added later if needed for:
- Per-app memory limits for entrypoint apps
- Per-app worker count isolation
- Better resource accounting

## Configuration Example

```json
{
  "apps": [
    {
      "hostname": "api.example.com",
      "sliver": "/apps/api.sliver",
      "limits": {"memory_mb": 256, "timeout_secs": 60, "workers": 8}
    },
    {
      "hostname": "blog.example.com",
      "entrypoint": "/apps/blog/index.js",
      "limits": {"memory_mb": 128, "timeout_secs": 30, "workers": 4}
    }
  ]
}
```

Both app types work in config mode. Sliver apps get snapshot restoration;
entrypoint apps get fresh isolate creation per request.
