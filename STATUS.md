# App Directory - Status

## Current State: Core Backend ✅ + Rate Limiting ✅ + Featured/Verified Badges ✅ + Health Check Monitoring ✅ + Webhooks ✅ + SSE Events ✅ + Scheduled Health Checks ✅ + Approval Workflow ✅ + App Statistics ✅ + Deprecation Workflow ✅ + Frontend ✅ + Unified Serving ✅ + README Complete ✅ + 96 Tests Passing ✅

**Tests:** 117 Rust + 209 Python SDK = 326 total  
**Python SDK** shipped (209 integration tests). Rust/Rocket + SQLite backend with full app CRUD, search, reviews with aggregate ratings, category listing, API key management, per-key rate limiting with response headers, featured/verified badge system, health check monitoring with batch checks and uptime tracking, scheduled background health checks, webhook notifications with HMAC-SHA256 signing, SSE real-time event stream, app approval workflow with dedicated approve/reject endpoints, app statistics with view tracking and trending, app deprecation workflow with replacement tracking and sunset dates, **React frontend with browse/search/submit/admin dashboard served from Rocket via unified serving**, and OpenAPI spec. Compiles cleanly (clippy -D warnings), all tests pass (run with `--test-threads=1`).

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
- **Deprecation Workflow (NEW):**
  - `POST /api/v1/apps/{id}/deprecate` — Deprecate app (admin only)
    - Required `reason` field (empty string rejected with 400)
    - Optional `replacement_app_id` — validated to exist and not self-reference
    - Optional `sunset_at` — ISO-8601 date when app stops working
    - Emits `app.deprecated` event with full metadata
    - Blocks on already-deprecated apps (409)
  - `POST /api/v1/apps/{id}/undeprecate` — Restore to approved (admin only)
    - Clears all deprecation metadata (reason, by, at, replacement, sunset)
    - Emits `app.undeprecated` event
    - Blocks on non-deprecated apps (409)
  - Deprecation fields on all app responses: `deprecated_reason`, `deprecated_by`, `deprecated_at`, `replacement_app_id`, `sunset_at`
  - DB migration: auto-adds 5 deprecation columns to existing databases
  - `app.deprecated` and `app.undeprecated` added to valid webhook event types (9 total)
  - Integration test covering full lifecycle (deprecate, verify, double-deprecate, approve/reject blocked, undeprecate, verify cleared)
- **App Statistics:**
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
- **Frontend:**
  - React + Vite dashboard served from Rocket via FileServer
  - Browse tab: paginated app listing with category/protocol filters
  - Full-text search bar across names, descriptions, tags
  - Submit tab: form for submitting new apps with all fields
  - App detail view: reviews, stats, deprecation info, external links
  - Admin panel: pending app approval/rejection, health overview, batch checks
  - Trending panel: configurable time window (24h/7d/30d)
  - API key stored in localStorage, rate limit display in header
  - Protocol/health/badge color-coding throughout
  - Dark theme (slate/indigo palette matching qr-service and kanban)
  - SPA catch-all fallback route (rank 20) serves index.html for client-side routing
  - STATIC_DIR env var for configurable frontend path (default: frontend/dist)
- **Unified Serving:**
  - Backend serves frontend static files via Rocket's FileServer
  - Auto-detects frontend/dist directory; API-only mode if missing
  - Single port, single binary deployment
- **Auth:** API key authentication via `Authorization: Bearer` or `X-API-Key`
- **Database:** SQLite with WAL mode, auto-creates admin key on first run
- **Docker:** Dockerfile (3-stage: Node frontend → Rust backend → Debian slim runtime)
- **Config:** Environment variables via `.env` / `dotenvy` (DATABASE_PATH, ROCKET_ADDRESS, ROCKET_PORT, RATE_LIMIT_WINDOW_SECS, HEALTH_CHECK_INTERVAL_SECS, STATIC_DIR)
- **Tests:** 99 tests passing (96 integration + 3 unit)
- **Code Quality:** Zero clippy warnings, cargo fmt clean
- **README:** Complete with setup, API reference, approval workflow, webhooks, health monitoring, scheduled checks docs, examples
- **Deployment:** Single-port unified serving (API + frontend on same origin)

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
6. ~~**App deprecation workflow**~~ ✅ Done — deprecate/undeprecate with replacement tracking + sunset dates
7. ~~**Frontend**~~ ✅ Done — React dashboard with browse/search/submit/admin/trending + unified serving
8. ~~**Update README**~~ ✅ Done — Frontend dashboard, unified serving, STATIC_DIR, architecture, backend dev docs
9. ~~**Investigate empty production DB**~~ ✅ Done (2026-02-08 14:35 UTC) — DB was fresh after auth refactor deploy (old volume orphaned). Seeded 3 HNR apps (QR Service, Kanban Board, App Directory) via admin key. All approved and visible.

- [x] **Backend route decomposition** (commit fec920d) — Monolithic 1846-line `src/routes.rs` split into 6 focused modules under `src/routes/`: system (82), apps (758), reviews (169), keys (102), webhook_routes (293), admin (405), mod.rs (17). Extracted `app_row_to_json` helper. Zero clippy warnings. All 37 tests pass.
- [x] **Parallel-safe tests** (commit fec920d) — Added `rocket_with_path()` to bypass env var races. Tests no longer need `--test-threads=1`. Parallel execution: 1.5s vs 4s sequential.
- [x] **Edit token auth refactor** (commit 900402e) — `update_app` and `delete_app` now accept per-app edit tokens via `?token=` query param or `X-Edit-Token` header, in addition to API key (owner/admin). SSE event stream made public (no auth required). Added `EditTokenParam` request guard and `check_edit_access()` helper. Edit tokens cannot change admin-only fields (status, badges). Cross-app token validation prevents using one app's token on another. 11 new tests (48 total). Updated llms.txt and OpenAPI spec.

**Consider deployable?** ✅ **YES — fully deployable.** Core API feature-complete: submit, discover, search, review, badges, health monitoring (manual + scheduled), webhooks, SSE real-time events, approval workflow, deprecation workflow with replacement tracking, app statistics with trending, rate limiting with headers, **per-app edit tokens (no signup needed)**. React frontend with browse/search/submit/admin/trending. Single port unified serving. 3-stage Docker build. README has setup instructions. 88 tests pass.

### ⚠️ Gotchas

- `cargo` not on PATH by default — use `export PATH="$HOME/.cargo/bin:$PATH"` before building
- CORS wide open (all origins) — tighten for production
- Admin key printed to stdout on first run — save it!
- ~~**Tests must run with `--test-threads=1`**~~ ✅ Fixed — tests now use `rocket_with_path()` to pass DB path directly, avoiding env var races. Parallel execution works.
- Search is LIKE-based (not full-text search) — adequate for moderate scale
- No slug uniqueness guarantee across deletions
- Rate limiter state is in-memory — resets on server restart
- OpenAPI spec is at v0.10.0 — 25 paths, 10+ schemas including approval, deprecation, and statistics types
- Badge columns auto-migrate on existing databases
- Health check columns auto-migrate on existing databases
- Approval workflow columns auto-migrate on existing databases
- Deprecation workflow columns auto-migrate on existing databases
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

**⚡ APP-DIRECTORY IS DONE. All three HNR projects are complete and deployable.**

*Last updated: 2026-02-07 21:10 UTC — Session: Fixed all 36 tests for open-read auth model (response field renames, public endpoints, auto-approve). Clippy clean.*

*Last updated: 2026-02-08 14:45 UTC — Seeded 3 HNR apps (empty DB resolved). Added /llms.txt endpoint. Pinned time crate to 0.3.36 (0.3.47 needs Rust 1.88). Deploy in progress.*

### Completed (2026-02-09 14:10 UTC)

- **ADMIN_API_KEY env var** ✅ — Allows seeding an admin key from environment variable on startup. Idempotent (checks hash before inserting). Solves the lost-admin-key problem without DB access. Key set on staging: `ad_hnr_appdir_admin_2026`.
- **Staging URL overrides** ✅ — Updated app listings to use internal IP `api_url` values until Cloudflare tunnels exist for blog/QR/apps.

### Completed (2026-02-09 14:30 UTC)

- **Fix admin update/delete for anonymous submissions** ✅ — Apps submitted without auth have `submitted_by_key_id = NULL`. Update/delete routes now handle NULL correctly (admins can manage anonymous apps). Added integration test `test_update_anonymous_app_as_admin`. 37 tests passing.

### Completed (2026-02-09 15:50 UTC)

- **Fix health checker checking wrong URL** ✅ — Scheduler was hitting raw `api_url` (e.g. `/api/v1`) which returns 404/500 on services without a root API handler. Now appends `/health` to `api_url` when available, so Blog and Agent Docs report healthy correctly. Commit: b655b07. 37 tests passing.

*Last updated: 2026-02-09 15:50 UTC — health checker URL fix. 37 tests passing.*

### Completed (2026-02-13 22:22 UTC)

- **Enhanced admin panel frontend** ✅ — Admin panel now has 4 sub-tabs:
  - **Pending:** App approval/rejection with improved layout (category badges, URLs)
  - **All Apps:** List all apps with featured/verified badge toggles, deprecate/undeprecate, delete buttons
  - **Health:** Dedicated health overview with batch check and issues list
  - **Keys:** List/create/revoke API keys with one-time key display
  - Admin tab still hidden when not authenticated (commit 622695a)
  - Frontend builds cleanly, 37 backend tests pass. Commit 6c28080.

## Incoming Directions (Work Queue)

<!-- WORK_QUEUE_DIRECTIONS_START -->
- [x] App Directory: Hide admin link when not logged in — ✅ Done (622695a + enhanced admin panel 6c28080)
<!-- WORK_QUEUE_DIRECTIONS_END -->

### Completed (2026-02-16 Daytime, Session — 00:45 UTC)

- **Expanded integration test coverage** ✅ Done — 40 new tests covering: pagination (page/per_page, beyond-data pages), sorting (name, oldest), filtering (category, protocol, status including rejected/all), slug-based lookup, /apps/mine endpoint, review upsert behavior, review for nonexistent app, review pagination, search with category filter, search pagination, search by tags, search no results, submission validation (missing name/description), anonymous submit response shape, approve/reject edge cases (already approved/rejected, empty reason, nonexistent app), pending list (empty since auto-approved), partial update field preservation, llms.txt/openapi.json/root llms.txt endpoints, delete cascade (reviews), deprecation with replacement + self-reference + undeprecate non-deprecated, key management (delete nonexistent, create response), webhook update, CORS preflight, health status initially null, complete response field verification. Test count: 48 → 88. Zero clippy warnings. Commit: d0f86b0.

### Completed (2026-02-17 — 15:45 UTC)

- **Fixed anonymous review bug** ✅ — Reviews submitted without an API key returned HTTP 500 due to `FOREIGN KEY (reviewer_key_id) REFERENCES api_keys(id)` constraint violation (code used "anonymous" as reviewer_key_id, which doesn't exist in api_keys). Fix: migrated reviews table to make `reviewer_key_id` nullable, removed FK constraint, added `reviewer_name` field. Anonymous reviews now always create new entries (multiple allowed per app). Authenticated reviews still upsert (one per key per app). Added reviewer_name to list endpoint responses. Python SDK updated (removed skip workaround, added reviewer_name parameter). 5 new tests (91 → 96 integration). Zero clippy warnings.
