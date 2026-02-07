# App Directory - Status

## Current State: Core Backend ✅ + Rate Limiting ✅ + Featured/Verified Badges ✅ + Health Check Monitoring ✅ + Webhooks ✅ + SSE Events ✅ + Scheduled Health Checks ✅ + Approval Workflow ✅ + App Statistics ✅ + 35 Tests Passing ✅

Rust/Rocket + SQLite backend with full app CRUD, search, reviews with aggregate ratings, category listing, API key management, per-key rate limiting with response headers, featured/verified badge system, health check monitoring with batch checks and uptime tracking, scheduled background health checks, webhook notifications with HMAC-SHA256 signing, SSE real-time event stream, app approval workflow with dedicated approve/reject endpoints, **app statistics with view tracking and trending**, and OpenAPI spec. Compiles cleanly (clippy -D warnings), all tests pass (run with `--test-threads=1`).

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
  - `GET /api/v1/openapi.json` — OpenAPI 3.0 spec (v0.8.0)
- **Approval Workflow (NEW):**
  - `GET /api/v1/apps/pending` — List pending apps (admin only, paginated, oldest first)
  - `POST /api/v1/apps/{id}/approve` — Approve a pending/rejected app (admin only)
    - Optional `note` field recorded on the app
    - Emits `app.approved` event with previous_status, reviewer, and note
    - Blocks on already-approved or deprecated apps (409)
  - `POST /api/v1/apps/{id}/reject` — Reject a pending/approved app (admin only)
    - Required `reason` field (empty string rejected with 400)
    - Emits `app.rejected` event with previous_status, reviewer, and reason
    - Blocks on already-rejected or deprecated apps (409)
  - Review metadata on all app responses: `review_note`, `reviewed_by`, `reviewed_at`
  - State transitions: pending↔approved, pending→rejected, rejected→approved, approved→rejected
  - Deprecated apps blocked from approve/reject
  - DB migration: auto-adds `review_note`, `reviewed_by`, `reviewed_at` columns
  - `app.rejected` added to valid webhook event types (7 total)
- **App Statistics (NEW):**
  - `GET /api/v1/apps/{id}/stats` — View counts (total, 24h, 7d, 30d) and unique viewers
    - Accepts app ID or slug (resolved to canonical ID)
    - Returns 404 for non-existent apps
  - `GET /api/v1/apps/trending` — Trending apps ranked by recent views
    - Configurable `days` period (1-90, default 7)
    - Configurable `limit` (1-50, default 10)
    - Returns view_count, unique_viewers, views_per_day per app
    - Only includes approved apps with at least 1 view
  - Automatic view tracking: every `GET /apps/{id}` records a view event
  - `app_views` table with app_id, viewer_key_id, viewed_at
  - Indexes on app_id, viewed_at, and composite (app_id, viewed_at) for efficient queries
  - 2 integration tests (test_app_stats, test_trending_apps)
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
- **Health Check Monitoring:**
  - `POST /api/v1/apps/{id}/health-check` — Single app health check (admin only)
  - `POST /api/v1/apps/health-check/batch` — Check all approved apps with URLs (admin only)
  - `GET /api/v1/apps/{id}/health` — Paginated health check history with uptime percentage
  - `GET /api/v1/apps/health/summary` — Overview: counts by status + list of apps with issues
  - `GET /api/v1/apps?health=<status>` — Filter apps by health status
  - Apps include `last_health_status`, `last_checked_at`, `uptime_pct` in all responses
- **Scheduled Health Checks:**
  - Background tokio task checks all approved apps on a configurable interval
  - Default interval: 300 seconds (5 minutes)
  - Configurable via `HEALTH_CHECK_INTERVAL_SECS` env var (0 to disable)
  - Separate DB connection for scheduler (no lock contention with request handlers)
  - First check runs after one full interval (server warmup period)
  - Emits `health.checked` SSE events with `"scheduled": true` flag
  - Graceful shutdown via `Rocket::Shutdown` handle
  - `GET /api/v1/health-check/schedule` — View scheduler config (admin only)
  - Records all the same data as manual checks: status, response time, uptime recalculation
- **SSE Real-Time Events:**
  - `GET /api/v1/events/stream` — Server-Sent Events stream (any authenticated key)
  - EventBus using `tokio::sync::broadcast` channel (lazy creation, Arc-wrapped for cloneability)
  - 7 event types: app.submitted, app.approved, app.rejected, app.updated, app.deleted, review.submitted, health.checked
  - 15-second heartbeat to keep connections alive
  - Graceful lagged-client handling (warning event if >256 events buffered)
  - Unified with webhook delivery — EventBus.emit() handles both SSE broadcast and webhook dispatch
  - No persistence — events are fire-and-forget to connected subscribers
- **Webhooks:**
  - `POST /api/v1/webhooks` — Register webhook (admin only)
    - Custom URL + optional event filter
    - Returns HMAC secret (shown only once)
  - `GET /api/v1/webhooks` — List all webhooks (admin only, secrets hidden)
  - `PATCH /api/v1/webhooks/{id}` — Update URL, events, or active status
    - Re-activating resets failure counter
  - `DELETE /api/v1/webhooks/{id}` — Delete webhook
  - 7 event types: app.submitted, app.approved, app.rejected, app.updated, app.deleted, review.submitted, health.checked
  - HMAC-SHA256 payload signatures via `X-AppDirectory-Signature` header
  - Event type in `X-AppDirectory-Event` header
  - Auto-disable after 10 consecutive delivery failures
  - Async delivery via `tokio::spawn` (non-blocking)
  - Separate DB connection for webhook delivery (no lock contention with main)
- **Rate Limiting:**
  - Fixed-window per-key enforcement via in-memory rate limiter
  - Default: 100 req/min for regular keys, 10,000 for admin keys
  - Response headers: X-RateLimit-Limit, X-RateLimit-Remaining, X-RateLimit-Reset
  - Returns 429 Too Many Requests when limit exceeded
- **Auth:** API key authentication via `Authorization: Bearer` or `X-API-Key`
- **Database:** SQLite with WAL mode, auto-creates admin key on first run
- **Docker:** Dockerfile (multi-stage build) + docker-compose.yml
- **Config:** Environment variables via `.env` / `dotenvy`
- **Tests:** 35 tests passing (19 integration + 1 scheduler + 7 health check + 4 webhook + 4 rate limiter unit tests)
- **Code Quality:** Zero clippy warnings, cargo fmt clean
- **README:** Complete with setup, API reference, approval workflow, webhooks, health monitoring, scheduled checks docs, examples

### Tech Stack

- Rust 1.83+ / Rocket 0.5 / SQLite (rusqlite)
- HTTP client: reqwest with rustls-tls (no OpenSSL dependency)
- HMAC: hmac + sha2 + hex crates
- CORS: wide open (all origins) — tighten for production

### Key Product Decisions

- **Protocol-aware discovery** — agents filter by `rest`, `mcp`, `a2a`, etc.
- **API spec URL** — link directly to machine-readable specs for auto-integration
- **Slug + UUID lookup** — both work for all app endpoints
- **Auto-approve admin submissions** — remove friction for trusted keys
- **Upsert reviews** — one review per key per app
- **Admin-only badges** — featured/verified are trust signals
- **Admin-only health checks** — prevents abuse of outbound HTTP requests
- **Admin-only webhooks** — global event notification system
- **Dedicated approve/reject endpoints** — clearer than PATCH status (audit trail, required reasons)
- **Rejection requires reason** — accountability and feedback to submitters
- **Review metadata on app responses** — transparency about approval decisions
- **Scheduled checks via background task** — no external cron needed
- **Separate scheduler DB connection** — avoids lock contention, same pattern as webhooks
- **SQLite** — same proven stack as qr-service and kanban
- **Port 8002** — avoids conflicts with qr-service (8000) and kanban (8001)
- **In-memory rate limiter** — no DB overhead per request
- **rustls over OpenSSL** — no system dependency needed for TLS
- **Uptime from last 100 checks** — rolling window prevents ancient data skewing results
- **EventBus internally Arc-wrapped** — cheaply cloneable for sharing with background tasks

### What's Next (Priority Order)

1. ~~**Webhook notifications**~~ ✅ Done
2. ~~**SSE real-time events**~~ ✅ Done
3. ~~**Scheduled health checks**~~ ✅ Done
4. ~~**App approval workflow**~~ ✅ Done
5. ~~**App statistics**~~ ✅ Done — view tracking, per-app stats, trending endpoint
6. **Frontend** — React or lightweight dashboard for human browsing
7. **App deprecation workflow** — mark apps as deprecated with migration notes

**Consider deployable?** Core API works end-to-end: submit, discover, search, review, badges, health monitoring (manual + scheduled), webhooks, SSE real-time events, approval workflow, app statistics with trending, rate limiting with headers. README has setup instructions. Tests pass. Docker support included. This is deployable — remaining items are enhancements.

### ⚠️ Gotchas

- `cargo` not on PATH by default — use `export PATH="$HOME/.cargo/bin:$PATH"` before building
- CORS wide open (all origins) — tighten for production
- Admin key printed to stdout on first run — save it!
- **Tests must run with `--test-threads=1`** — tests use `std::env::set_var("DATABASE_PATH", ...)` which races under parallel execution
- Search is LIKE-based (not full-text search) — adequate for moderate scale
- No slug uniqueness guarantee across deletions
- Rate limiter state is in-memory — resets on server restart
- OpenAPI spec is at v0.9.0 — 21 paths, 10+ schemas including approval workflow and statistics types
- Badge columns auto-migrate on existing databases
- Health check columns auto-migrate on existing databases
- Approval workflow columns auto-migrate on existing databases
- Webhook table auto-creates (in init_db schema)
- Health checks make outbound HTTP requests — admin-only to prevent abuse
- Batch health check is sequential (not parallel) — safe but slower for many apps
- Webhook delivery is fire-and-forget — check failure_count for monitoring
- Scheduler opens its own DB connection on liftoff — unaffected by main DB lock contention
- `HEALTH_CHECK_INTERVAL_SECS=0` disables scheduled checks entirely
- First scheduled check runs after one full interval (not immediately on start)
- `/apps/pending` route must be mounted before `/apps/<id_or_slug>` for Rocket to rank correctly
- View tracking records every GET /apps/{id} request — no deduplication per session (agent views count repeatedly)
- `app_views` table grows unbounded — consider periodic cleanup for long-running instances

### Architecture Notes

- Lib + binary split for testability (`lib.rs` exposes `rocket()` builder)
- Single-threaded SQLite via `Mutex<Connection>` (main)
- Separate `WebhookDb` connection for async delivery (avoids lock contention)
- Separate `SchedulerDb` connection for background health checks (same pattern)
- EventBus internally `Arc`-wrapped for cheap cloning across tasks
- Dynamic SQL construction for update/filter operations
- Aggregate ratings recomputed on every review write
- `rate_limit.rs` uses `Mutex<HashMap>` with fixed-window algorithm
- Rate limit headers via Rocket fairing reading request-local state
- DB lock scoped carefully in auth guard to avoid Send issues across `.await`
- Health checks use `reqwest` with rustls-tls backend (no OpenSSL)
- Batch health check acquires/releases DB lock per app (not held during HTTP)
- Webhook delivery spawned via `tokio::spawn` — non-blocking to request handler
- `events.rs` — EventBus with single `broadcast::Sender` (global, not per-entity)
- `scheduler.rs` — Rocket `Liftoff` fairing spawning tokio task with `Shutdown` handle
- SSE stream uses `rocket::response::stream::EventStream` with `tokio::select!` for graceful shutdown
- CORS wide open (all origins)

---

*Last updated: 2026-02-07 13:05 UTC — Session: App statistics shipped (view tracking + per-app stats + trending endpoint + 2 tests)*
