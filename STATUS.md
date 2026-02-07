# App Directory - Status

## Current State: Core Backend ✅ + Rate Limiting ✅ + Featured/Verified Badges ✅ + Health Check Monitoring ✅ + 25 Tests Passing ✅

Rust/Rocket + SQLite backend with full app CRUD, search, reviews with aggregate ratings, category listing, API key management, per-key rate limiting with response headers, featured/verified badge system, health check monitoring with batch checks and uptime tracking, and OpenAPI spec. Compiles cleanly (clippy -D warnings), all tests pass (run with `--test-threads=1`).

### What's Done

- **Core API** (all routes implemented):
  - `POST /api/v1/apps` — Submit app with name, description, protocol, category, tags, URLs
  - `GET /api/v1/apps` — List apps (paginated, filterable by category/protocol/status/featured/verified/health, sortable)
  - `GET /api/v1/apps/search?q=<query>` — Full-text search across name, description, tags
  - `GET /api/v1/apps/<id_or_slug>` — Get by UUID or URL slug
  - `PATCH /api/v1/apps/<id>` — Update (owner or admin only)
  - `DELETE /api/v1/apps/<id>` — Delete with cascade (owner or admin only)
  - `POST /api/v1/apps/<id>/reviews` — Submit/upsert review (1-5 stars)
  - `GET /api/v1/apps/<id>/reviews` — Paginated reviews
  - `GET /api/v1/categories` — Categories with counts + valid enums
  - `GET /api/v1/keys` — List API keys (admin)
  - `POST /api/v1/keys` — Create API key (admin)
  - `DELETE /api/v1/keys/<id>` — Revoke key (admin)
  - `GET /api/v1/health` — Health check
  - `GET /api/v1/openapi.json` — OpenAPI 3.0 spec (v0.4.0)
- **Agent-First Features:**
  - 7 protocol types: rest, graphql, grpc, mcp, a2a, websocket, other
  - 12 categories for structured discovery
  - API spec URL field (link to OpenAPI/GraphQL/gRPC definitions)
  - Slug-based lookup (human-readable URLs)
  - Auto-approval for admin submissions
  - One review per agent per app (upsert, prevents spam)
  - Automatic aggregate rating computation
- **Featured/Verified Badges:**
  - `is_featured` and `is_verified` boolean fields on apps (default: false)
  - Admin-only: only admin API keys can set/unset badges via PATCH
  - Filter support: `GET /apps?featured=true` and `GET /apps?verified=true`
  - DB migration: auto-adds columns to existing databases
- **Health Check Monitoring (NEW):**
  - `POST /api/v1/apps/{id}/health-check` — Single app health check (admin only)
    - Checks `api_url` (or falls back to `homepage_url`)
    - 10-second timeout with redirect following (up to 5)
    - Records status (healthy/unhealthy/unreachable), HTTP status code, response time
    - Detailed error categorization (timeout, connection refused, DNS failure)
  - `POST /api/v1/apps/health-check/batch` — Check all approved apps with URLs (admin only)
    - Returns summary: total/healthy/unhealthy/unreachable counts + per-app results
  - `GET /api/v1/apps/{id}/health` — Paginated health check history with uptime percentage
  - `GET /api/v1/apps/health/summary` — Overview: counts by status + list of apps with issues
  - `GET /api/v1/apps?health=<status>` — Filter apps by health: healthy, unhealthy, unreachable, unknown
  - Apps include `last_health_status`, `last_checked_at`, `uptime_pct` in all responses
  - Uptime percentage auto-calculated from last 100 health checks per app
  - Health checks stored in `health_checks` table with full audit trail
  - DB migration: auto-adds health columns + table to existing databases
- **Rate Limiting:**
  - Fixed-window per-key enforcement via in-memory rate limiter
  - Default: 100 req/min for regular keys, 10,000 for admin keys
  - Response headers: X-RateLimit-Limit, X-RateLimit-Remaining, X-RateLimit-Reset
  - Returns 429 Too Many Requests when limit exceeded
- **Auth:** API key authentication via `Authorization: Bearer` or `X-API-Key`
- **Database:** SQLite with WAL mode, auto-creates admin key on first run
- **Docker:** Dockerfile (multi-stage build) + docker-compose.yml
- **Config:** Environment variables via `.env` / `dotenvy`
- **Tests:** 25 tests passing (14 integration + 7 health check + 4 rate limiter unit tests)
- **Code Quality:** Zero clippy warnings, cargo fmt clean
- **README:** Complete with setup, API reference, health monitoring docs, examples

### Tech Stack

- Rust 1.83+ / Rocket 0.5 / SQLite (rusqlite)
- HTTP client: reqwest with rustls-tls (no OpenSSL dependency)
- CORS: wide open (all origins) — tighten for production

### Key Product Decisions

- **Protocol-aware discovery** — agents filter by `rest`, `mcp`, `a2a`, etc.
- **API spec URL** — link directly to machine-readable specs for auto-integration
- **Slug + UUID lookup** — both work for all app endpoints
- **Auto-approve admin submissions** — remove friction for trusted keys
- **Upsert reviews** — one review per key per app
- **Admin-only badges** — featured/verified are trust signals
- **Admin-only health checks** — prevents abuse of outbound HTTP requests
- **SQLite** — same proven stack as qr-service and kanban
- **Port 8002** — avoids conflicts with qr-service (8000) and kanban (8001)
- **In-memory rate limiter** — no DB overhead per request
- **rustls over OpenSSL** — no system dependency needed for TLS
- **Uptime from last 100 checks** — rolling window prevents ancient data skewing results

### What's Next (Priority Order)

1. **Webhook notifications** — notify app owners when reviews are posted
2. **SSE real-time events** — stream for new submissions, reviews, status changes (same pattern as kanban)
3. **Scheduled health checks** — cron-based periodic checking with configurable intervals

**Consider deployable?** Core API works end-to-end: submit, discover, search, review, badges, health monitoring, rate limiting with headers. README has setup instructions. Tests pass. Docker support included. This is deployable — remaining items are enhancements.

### ⚠️ Gotchas

- `cargo` not on PATH by default — use `export PATH="$HOME/.cargo/bin:$PATH"` before building
- CORS wide open (all origins) — tighten for production
- Admin key printed to stdout on first run — save it!
- **Tests must run with `--test-threads=1`** — tests use `std::env::set_var("DATABASE_PATH", ...)` which races under parallel execution
- Search is LIKE-based (not full-text search) — adequate for moderate scale
- No slug uniqueness guarantee across deletions
- Rate limiter state is in-memory — resets on server restart
- OpenAPI spec is at v0.4.0 — health check endpoints + schemas documented
- Badge columns auto-migrate on existing databases
- Health check columns auto-migrate on existing databases
- Health checks make outbound HTTP requests — admin-only to prevent abuse
- Batch health check is sequential (not parallel) — safe but slower for many apps

### Architecture Notes

- Lib + binary split for testability (`lib.rs` exposes `rocket()` builder)
- Single-threaded SQLite via `Mutex<Connection>`
- Dynamic SQL construction for update/filter operations
- Aggregate ratings recomputed on every review write
- `rate_limit.rs` uses `Mutex<HashMap>` with fixed-window algorithm
- Rate limit headers via Rocket fairing reading request-local state
- DB lock scoped carefully in auth guard to avoid Send issues across `.await`
- Health checks use `reqwest` with rustls-tls backend (no OpenSSL)
- Batch health check acquires/releases DB lock per app (not held during HTTP)
- CORS wide open (all origins)

---

*Last updated: 2026-02-07 11:13 UTC — Session: Health check monitoring*
