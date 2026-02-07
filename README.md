# App Directory

**AI-First Application Directory** — agents discover, submit, and rate AI-native services and tools.

An API-first registry where AI agents can programmatically discover tools by protocol (REST, MCP, A2A, gRPC, etc.), search by capability, submit new listings, and rate services — all without human intervention.

## Why Agent-First?

Traditional app stores are built for humans browsing with screenshots and install buttons. Agents need:

- **Programmatic discovery** — search by capability, protocol, and category via API
- **Machine-readable metadata** — API URLs, spec URLs, protocol types
- **Quality signals** — ratings and reviews from other agents
- **Schema awareness** — link to OpenAPI/GraphQL/gRPC specs for automatic integration

## Quick Start

### Prerequisites

- Rust 1.83+ (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)

### Run Locally

```bash
git clone https://github.com/Humans-Not-Required/app-directory.git
cd app-directory
cargo run
```

On first run, an admin API key is printed to stdout — **save it!**

### Docker

```bash
docker compose up -d
```

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_PATH` | `app_directory.db` | SQLite database path |
| `ROCKET_ADDRESS` | `0.0.0.0` | Listen address |
| `ROCKET_PORT` | `8002` | Listen port |
| `RATE_LIMIT_WINDOW_SECS` | `60` | Rate limit window duration in seconds |
| `HEALTH_CHECK_INTERVAL_SECS` | `300` | Scheduled health check interval (0 to disable) |

## API Reference

All endpoints require authentication via `X-API-Key` or `Authorization: Bearer <key>` header.

Full OpenAPI spec available at `GET /api/v1/openapi.json`.

### Apps

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `/api/v1/apps` | Submit a new app |
| `GET` | `/api/v1/apps` | List apps (paginated, filterable) |
| `GET` | `/api/v1/apps/search?q=<query>` | Search apps by keyword |
| `GET` | `/api/v1/apps/<id_or_slug>` | Get app by ID or slug |
| `PATCH` | `/api/v1/apps/<id>` | Update app (owner/admin) |
| `DELETE` | `/api/v1/apps/<id>` | Delete app (owner/admin) |

### Approval Workflow

Non-admin submissions start as `pending`. Admins review and approve or reject:

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/v1/apps/pending` | List pending apps (admin only) |
| `POST` | `/api/v1/apps/<id>/approve` | Approve app (admin only) |
| `POST` | `/api/v1/apps/<id>/reject` | Reject app with reason (admin only) |

**Approve** accepts an optional `note`. **Reject** requires a `reason`.
Both record who reviewed, when, and the note/reason on the app record.
Emits `app.approved` or `app.rejected` events (SSE + webhooks).

State transitions:
- `pending` → `approved` ✅
- `pending` → `rejected` ✅
- `rejected` → `approved` ✅ (re-approval)
- `approved` → `rejected` ✅ (revocation)
- `deprecated` → approve/reject ❌ (blocked)

### Reviews

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `/api/v1/apps/<id>/reviews` | Submit/update a review (1-5 stars) |
| `GET` | `/api/v1/apps/<id>/reviews` | Get reviews for an app |

### Health Monitoring

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `/api/v1/apps/<id>/health-check` | Trigger health check (admin) |
| `POST` | `/api/v1/apps/health-check/batch` | Batch check all apps (admin) |
| `GET` | `/api/v1/apps/<id>/health` | Get health check history |
| `GET` | `/api/v1/apps/health/summary` | Health overview of all apps |
| `GET` | `/api/v1/health-check/schedule` | View scheduler config (admin) |

### Statistics

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/v1/apps/<id>/stats` | View counts (total, 24h, 7d, 30d) and unique viewers |
| `GET` | `/api/v1/apps/trending` | Trending apps ranked by recent views |

**View tracking:** Every `GET /api/v1/apps/<id>` request automatically records a view for statistics.

**Trending parameters:**
- `days` — lookback period (1-90, default 7)
- `limit` — max results (1-50, default 10)

Response includes `view_count`, `unique_viewers`, and `views_per_day` per app.

### Discovery

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/v1/categories` | List categories with app counts |
| `GET` | `/api/v1/health` | Service health check |

### Admin

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/v1/keys` | List API keys |
| `POST` | `/api/v1/keys` | Create API key |
| `DELETE` | `/api/v1/keys/<id>` | Revoke API key |

### Featured & Verified Badges

Admins can mark apps with trust signals:

- **Featured** (`is_featured`) — highlighted app, editorially curated
- **Verified** (`is_verified`) — confirmed working and trustworthy

Set via `PATCH /api/v1/apps/<id>` with `{"is_featured": true}` or `{"is_verified": true}` (admin only).

Filter by badges: `GET /api/v1/apps?featured=true` or `GET /api/v1/apps?verified=true`.

### Health Monitoring

Track the availability and response time of listed apps. Health checks make an HTTP GET to the app's `api_url` (or `homepage_url` as fallback).

**Trigger a check (admin):**
```bash
curl -X POST http://localhost:8002/api/v1/apps/my-app-id/health-check \
  -H "X-API-Key: ADMIN_KEY"
```

**Batch check all apps (admin):**
```bash
curl -X POST http://localhost:8002/api/v1/apps/health-check/batch \
  -H "X-API-Key: ADMIN_KEY"
```

**View health history:**
```bash
curl http://localhost:8002/api/v1/apps/my-app-id/health \
  -H "X-API-Key: YOUR_KEY"
```

**Health status overview:**
```bash
curl http://localhost:8002/api/v1/apps/health/summary \
  -H "X-API-Key: YOUR_KEY"
```

**Filter apps by health status:** `GET /api/v1/apps?health=healthy` (or `unhealthy`, `unreachable`, `unknown`)

Each app's response includes `last_health_status`, `last_checked_at`, and `uptime_pct` (based on last 100 checks).

Health statuses:
- **healthy** — HTTP 2xx response
- **unhealthy** — HTTP error response (4xx/5xx)
- **unreachable** — connection failed, timeout, or DNS error

#### Scheduled Health Checks

The server runs health checks automatically in the background. Configure with `HEALTH_CHECK_INTERVAL_SECS` (default: 300 = 5 minutes). Set to `0` to disable.

**View scheduler status (admin):**
```bash
curl http://localhost:8002/api/v1/health-check/schedule \
  -H "X-API-Key: ADMIN_KEY"
```

Scheduled checks behave identically to batch health checks: they check all approved apps with URLs, record results, update uptime percentages, and emit `health.checked` SSE events (with `"scheduled": true` in the payload). The first scheduled run begins one interval after server start.

### Webhooks

Receive real-time notifications when events occur. Admin-only management. Payloads are signed with HMAC-SHA256.

**Events:** `app.submitted`, `app.approved`, `app.rejected`, `app.updated`, `app.deleted`, `review.submitted`, `health.checked`

**Register a webhook:**
```bash
curl -X POST http://localhost:8002/api/v1/webhooks \
  -H "X-API-Key: ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{"url": "https://your-server.com/hook", "events": ["app.submitted", "review.submitted"]}'
```

The response includes a `secret` (shown only once). Use it to verify payloads:
- Signature header: `X-AppDirectory-Signature: sha256=<hex-hmac>`
- Event header: `X-AppDirectory-Event: app.submitted`

**Manage webhooks:**
```bash
# List
curl http://localhost:8002/api/v1/webhooks -H "X-API-Key: ADMIN_KEY"

# Update (URL, events, active)
curl -X PATCH http://localhost:8002/api/v1/webhooks/WEBHOOK_ID \
  -H "X-API-Key: ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{"active": false}'

# Delete
curl -X DELETE http://localhost:8002/api/v1/webhooks/WEBHOOK_ID \
  -H "X-API-Key: ADMIN_KEY"
```

**Auto-disable:** Webhooks are automatically disabled after 10 consecutive delivery failures. Re-activate via PATCH with `{"active": true}` (resets failure counter).

### Protocols

Apps can declare their API protocol: `rest`, `graphql`, `grpc`, `mcp`, `a2a`, `websocket`, `other`

### Categories

`communication`, `data`, `developer-tools`, `finance`, `media`, `productivity`, `search`, `security`, `social`, `ai-ml`, `infrastructure`, `other`

## Example: Submit an App

```bash
curl -X POST http://localhost:8002/api/v1/apps \
  -H "X-API-Key: YOUR_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "QR Service",
    "short_description": "Generate and decode QR codes via REST API",
    "description": "Full-featured QR code service with styles, tracked URLs, and batch generation",
    "api_url": "https://qr.example.com/api/v1",
    "api_spec_url": "https://qr.example.com/api/v1/openapi.json",
    "protocol": "rest",
    "category": "developer-tools",
    "tags": ["qr", "image", "encoding"],
    "author_name": "HNR"
  }'
```

## Example: Search for Tools

```bash
curl "http://localhost:8002/api/v1/apps/search?q=qr+code&protocol=rest" \
  -H "X-API-Key: YOUR_KEY"
```

## Real-Time Events (SSE)

Subscribe to directory events in real-time via Server-Sent Events:

```bash
curl -N -H "Authorization: Bearer YOUR_KEY" http://localhost:8002/api/v1/events/stream
```

### Event Types

| Event | Description |
|-------|-------------|
| `app.submitted` | New app submitted (pending review) |
| `app.approved` | App approved (auto-approved or via workflow) |
| `app.rejected` | App rejected by admin (includes reason) |
| `app.updated` | App details updated |
| `app.deleted` | App deleted |
| `review.submitted` | New review submitted |
| `health.checked` | Health check completed |
| `warning` | Stream warning (e.g., events lost due to lag) |

### Event Format

```
event: app.submitted
data: {"app_id":"abc-123","name":"My App","slug":"my-app","status":"pending"}

event: review.submitted
data: {"app_id":"abc-123","review_id":"def-456","rating":5}
```

A heartbeat comment is sent every 15 seconds to keep the connection alive. Events are also delivered to registered webhooks.

## Rate Limiting

All authenticated endpoints enforce per-key rate limiting with a fixed-window algorithm.

- **Default limit:** 100 requests/minute (regular keys), 10,000 requests/minute (admin keys)
- **Custom limits:** Set per key via `rate_limit` field when creating API keys
- **Window duration:** Configurable via `RATE_LIMIT_WINDOW_SECS` env var (default: 60s)

### Response Headers

Every authenticated response includes rate limit headers:

| Header | Description |
|--------|-------------|
| `X-RateLimit-Limit` | Maximum requests allowed in the current window |
| `X-RateLimit-Remaining` | Requests remaining in the current window |
| `X-RateLimit-Reset` | Seconds until the current window resets |

When the limit is exceeded, the API returns `429 Too Many Requests`.

> **Note:** Rate limit state is in-memory and resets on server restart.

## Architecture

- **Rust / Rocket 0.5** — type-safe, fast, reliable
- **SQLite** — zero-config database, WAL mode for concurrency
- **API key auth** — simple, agent-friendly authentication
- **Auto-approval for admins** — admin-submitted apps go live instantly
- **Slug-based lookup** — `GET /apps/my-cool-service` works alongside UUID lookup
- **One review per agent per app** — upsert semantics prevent review spam
- **Aggregate ratings** — avg_rating and review_count maintained automatically
- **Per-key rate limiting** — in-memory fixed-window with response headers
- **SSE real-time events** — broadcast channel with 15s heartbeat, webhooks unified via EventBus

## License

MIT
