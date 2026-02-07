# App Directory - Status

## Current State: Core Backend ✅ + 12 Tests Passing ✅

Rust/Rocket + SQLite backend with full app CRUD, search, reviews with aggregate ratings, category listing, API key management, and OpenAPI spec. Compiles cleanly (clippy -D warnings), all tests pass (run with `--test-threads=1`).

### What's Done

- **Core API** (all routes implemented):
  - `POST /api/v1/apps` — Submit app with name, description, protocol, category, tags, URLs
  - `GET /api/v1/apps` — List apps (paginated, filterable by category/protocol/status, sortable)
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
  - `GET /api/v1/openapi.json` — OpenAPI 3.0 spec
- **Agent-First Features:**
  - 7 protocol types: rest, graphql, grpc, mcp, a2a, websocket, other
  - 12 categories for structured discovery
  - API spec URL field (link to OpenAPI/GraphQL/gRPC definitions)
  - Slug-based lookup (human-readable URLs)
  - Auto-approval for admin submissions
  - One review per agent per app (upsert, prevents spam)
  - Automatic aggregate rating computation
- **Auth:** API key authentication via `Authorization: Bearer` or `X-API-Key`
- **Database:** SQLite with WAL mode, auto-creates admin key on first run
- **Docker:** Dockerfile (multi-stage build) + docker-compose.yml
- **Config:** Environment variables via `.env` / `dotenvy`
- **Tests:** 12 integration tests passing
- **Code Quality:** Zero clippy warnings, cargo fmt clean
- **README:** Complete with setup instructions, API reference, examples

### Tech Stack

- Rust 1.83+ / Rocket 0.5 / SQLite (rusqlite)
- CORS: wide open (all origins) — tighten for production

### Key Product Decisions

- **Protocol-aware discovery** — agents filter by `rest`, `mcp`, `a2a`, etc.
- **API spec URL** — link directly to machine-readable specs for auto-integration
- **Slug + UUID lookup** — `GET /apps/my-cool-service` AND `GET /apps/<uuid>` both work
- **Auto-approve admin submissions** — remove friction for trusted keys
- **Upsert reviews** — one review per key per app, update by resubmitting
- **SQLite** — same proven stack as qr-service and kanban
- **Port 8002** — avoids conflicts with qr-service (8000) and kanban (8001)

### What's Next (Priority Order)

1. **Rate limiting** — per-key rate limiting with response headers (same pattern as qr-service/kanban)
2. **Health check integration** — periodic endpoint testing for listed apps
3. **Featured/verified badges** — trust signals for high-quality listings
4. **Webhook notifications** — notify app owners when reviews are posted

**Consider deployable?** Core API works end-to-end: submit, discover, search, review. README has setup instructions. Tests pass. Docker support included. This is deployable — remaining items are enhancements.

### ⚠️ Gotchas

- `cargo` not on PATH by default — use `export PATH="$HOME/.cargo/bin:$PATH"` before building
- CORS wide open (all origins) — tighten for production
- Admin key printed to stdout on first run — save it!
- **Tests must run with `--test-threads=1`** — tests use `std::env::set_var("DATABASE_PATH", ...)` which races under parallel execution
- Search is LIKE-based (not full-text search) — adequate for moderate scale
- No slug uniqueness guarantee across deletions (slug collision appends UUID prefix)

### Architecture Notes

- Lib + binary split for testability (`lib.rs` exposes `rocket()` builder)
- Single-threaded SQLite via `Mutex<Connection>`
- Dynamic SQL construction for update/filter operations
- Aggregate ratings recomputed on every review write (no stale caches)
- CORS wide open (all origins)

---

*Last updated: 2026-02-07 10:30 UTC — Session: Initial build (core backend + tests + Docker + README)*
