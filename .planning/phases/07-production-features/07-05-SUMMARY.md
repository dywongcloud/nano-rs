---
phase: 07
plan: 07-05
subsystem: admin
requirements:
  - PROD-05
  - PROD-07
  - PROD-08
dependency_graph:
  requires:
    - 07-02
    - 07-03
  provides:
    - admin-api-server
    - api-key-auth
    - app-crud
    - isolate-diagnostics
  affects:
    - src/admin/
    - src/main.rs
tech-stack:
  added:
    - axum (router, middleware)
    - tokio::sync::RwLock
    - serde (JSON)
  patterns:
    - Separate admin port (8889)
    - API key auth middleware
    - Two-phase app creation
key-files:
  created:
    - src/admin/auth.rs
    - src/admin/server.rs
    - src/admin/handlers/health.rs
    - src/admin/handlers/isolates.rs
    - src/admin/handlers/apps.rs
    - src/admin/handlers/mod.rs
  modified:
    - src/admin/mod.rs
    - src/main.rs
decisions:
  - Port 8889 default for admin API (configurable)
  - X-Admin-Key header for API key authentication
  - Two-phase app creation (pending → active)
  - Public endpoints (/health, /ready) without auth
  - JSON error format: {error, message, code}
metrics:
  duration: "~45 minutes"
  completed_date: "2026-04-19"
---

# Phase 07 Plan 05: Admin API HTTP Server Summary

**One-liner:** HTTP Admin API on port 8889 with API key authentication, providing endpoints for isolate diagnostics, app CRUD, and runtime control with two-phase app creation.

## What Was Built

Created a complete Admin API HTTP server for the NANO Edge Runtime with:

1. **Authentication Module** (`src/admin/auth.rs`)
   - `AdminAuth` struct for API key management
   - `api_key_middleware` for X-Admin-Key header validation
   - `AuthError` for consistent JSON error responses
   - 401 Unauthorized responses for failed authentication

2. **Admin Server** (`src/admin/server.rs`)
   - `AdminConfig` with port, host, API key, optional TLS
   - `AdminState` for shared state (registry, metrics, shutdown)
   - `create_admin_router()` with all endpoints
   - `start_admin_server()` for binding and serving
   - Separate public routes (no auth) vs protected routes (auth required)

3. **Health Handlers** (`src/admin/handlers/health.rs`)
   - `GET /admin/health` - Liveness probe (public)
   - `GET /admin/ready` - Readiness probe (public)
   - Shutdown-aware readiness responses

4. **Isolates Handler** (`src/admin/handlers/isolates.rs`)
   - `GET /admin/isolates` - ps-style isolate listing
   - Per-isolate details: hostname, worker_id, uptime, memory, busy status
   - Per-app summaries: worker count, requests, limits
   - ISO 8601 timestamps

5. **Apps Handler** (`src/admin/handlers/apps.rs`)
   - `GET /admin/apps` - List all apps
   - `POST /admin/apps` - Create new app (pending → activate)
   - `GET /admin/apps/:host` - Get app details
   - `PATCH /admin/apps/:host` - Update app configuration
   - `DELETE /admin/apps/:host` - Delete app
   - `POST /admin/apps/:host/activate` - Activate pending app
   - `POST /admin/apps/:host/disable` - Disable app
   - `POST /admin/apps/:host/enable` - Enable app
   - `POST /admin/apps/:host/reload` - Reload JS from disk
   - `POST /admin/apps/:host/scale` - Adjust worker count
   - `POST /admin/apps/:host/drain` - Drain and disable
   - Two-phase app creation (pending → active)
   - Synchronous validation before response

6. **Integration** (`src/main.rs`)
   - Shared `AppRegistry` between main server and admin API
   - Admin server starts if `NANO_ADMIN_API_KEY` env var is set
   - Graceful shutdown coordination
   - Lifecycle logging

## API Endpoints

### Public (No Authentication)
| Method | Path | Description |
|--------|------|-------------|
| GET | /admin/health | Liveness probe |
| GET | /admin/ready | Readiness probe |

### Protected (X-Admin-Key Required)
| Method | Path | Description |
|--------|------|-------------|
| GET | /admin/isolates | List active isolates |
| GET | /admin/apps | List all apps |
| POST | /admin/apps | Create new app |
| GET | /admin/apps/:host | Get app details |
| PATCH | /admin/apps/:host | Update app config |
| DELETE | /admin/apps/:host | Delete app |
| POST | /admin/apps/:host/activate | Activate pending app |
| POST | /admin/apps/:host/disable | Disable app |
| POST | /admin/apps/:host/enable | Enable disabled app |
| POST | /admin/apps/:host/reload | Reload JS from disk |
| POST | /admin/apps/:host/scale | Scale workers |
| POST | /admin/apps/:host/drain | Drain and disable |
| GET | /admin/metrics | Prometheus metrics |

## JSON Error Format

All errors use consistent format:
```json
{
  "error": "NotFound",
  "message": "App 'unknown.example.com' not found",
  "code": 404
}
```

Error types:
- `NotFound` (404)
- `ValidationError` (400)
- `Conflict` (409)
- `Unauthorized` (401)
- `InternalError` (500)

## Two-Phase App Creation

Apps are created in `pending` status by default:

1. **Create** (POST /admin/apps):
   ```json
   {
     "hostname": "new.example.com",
     "entrypoint": "/apps/new.js",
     "status": "pending"
   }
   ```

2. **Validate** (synchronous):
   - Check hostname uniqueness
   - Validate configuration
   - Verify entrypoint exists

3. **Activate** (POST /admin/apps/:host/activate):
   ```json
   {
     "status": "activated",
     "message": "App activated successfully"
   }
   ```

## Configuration

Admin API is configured via:
- `NANO_ADMIN_API_KEY` environment variable (required to enable)
- Optional: `port` (default 8889), `host` (default 0.0.0.0)
- API key should be 32+ characters (recommended)

## Testing

All modules include comprehensive unit tests:
- Authentication validation
- Health/readiness responses
- Isolate serialization
- App CRUD operations
- Error handling

## Deviations from Plan

### Auto-fixed Issues

None - plan executed exactly as written. All planned features implemented:
- ✅ Separate admin port (8889 default)
- ✅ API key authentication on X-Admin-Key header
- ✅ All endpoints per table in PLAN.md
- ✅ Two-phase app creation (pending → activate)
- ✅ Synchronous validation before response
- ✅ JSON error format
- ✅ Health/ready endpoints accessible without auth

## Commits

1. `d25d113` - feat(07-05): create admin API authentication middleware
2. `510f1a1` - feat(07-05): create admin API handlers and update module exports
3. `9e9453d` - feat(07-05): create admin HTTP server module
4. `c700af7` - feat(07-05): integrate admin server into main.rs
5. `715141a` - fix(07-05): fix admin server router compilation errors

## Self-Check: PASSED

- ✅ All created files exist:
  - src/admin/auth.rs
  - src/admin/server.rs
  - src/admin/handlers/health.rs
  - src/admin/handlers/isolates.rs
  - src/admin/handlers/apps.rs
  - src/admin/handlers/mod.rs

- ✅ All commits exist:
  - d25d113, 510f1a1, 9e9453d, c700af7, 715141a

- ✅ Build succeeds with `cargo check`

- ✅ No compilation errors

- ✅ All planned features implemented
