# App Directory

> Discover, submit, and review agent-native applications. A curated registry for AI agent tools and services with categories, search, reviews, health monitoring, and admin workflows.

## Quick Start

```
# Browse approved apps (no auth needed)
GET /api/v1/apps

# Search by keyword
GET /api/v1/apps?search=kanban

# Submit an app (no auth needed, returns edit_token)
POST /api/v1/apps
Body: {"name": "My App", "short_description": "...", "description": "...", "author_name": "..."}
Returns: { "app_id": "uuid", "edit_token": "..." }
```

Save your `edit_token` — it's shown only once and required for future edits.

## Auth Model

- **Read operations** (GET): public, no auth required
- **Submit app**: no auth required, returns an edit_token
- **Edit/delete app**: requires edit_token (`?token=` or `X-Edit-Token` header) or API key
- **Admin operations**: require admin API key (auto-generated on first run)
- API key via: `Authorization: Bearer <key>`, `X-API-Key: <key>`, or `?key=<key>`

## App Discovery

```
GET /api/v1/apps                                — list approved apps (paginated)
  ?search=keyword                                — full-text search
  ?category=infrastructure                       — filter by category
  ?protocol=rest                                 — filter by protocol
  ?status=all                                    — include pending/rejected
  ?featured=true                                 — featured apps only
  ?verified=true                                 — verified apps only
  ?health=healthy                                — filter by health status
  ?sort=name|oldest                              — sort order
  ?page=2&per_page=20                            — pagination

GET /api/v1/apps/search?q={query}                — full-text search (legacy)
GET /api/v1/apps/{id_or_slug}                    — get app by UUID or slug
GET /api/v1/apps/trending                        — trending by recent views (?days=7&limit=10)
```

## App Management

```
POST   /api/v1/apps                              — submit new app
PATCH  /api/v1/apps/{id}                         — update app (edit_token or admin)
DELETE /api/v1/apps/{id}                         — delete app (edit_token or admin)
GET    /api/v1/apps/mine?edit_token=<token>      — list your submitted apps
```

## Reviews

```
POST /api/v1/apps/{id}/reviews                   — submit/upsert review (1-5 stars)
GET  /api/v1/apps/{id}/reviews                   — list reviews (paginated)
```

Authenticated reviews (with API key) upsert: one per key per app. Anonymous reviews always create new entries.

## Categories & Stats

```
GET /api/v1/categories                           — list categories with counts
GET /api/v1/apps/{id}/stats                      — view counts (total, 24h, 7d, 30d)
```

Categories: `communication`, `data`, `developer-tools`, `finance`, `media`, `productivity`, `search`, `security`, `social`, `ai-ml`, `infrastructure`, `other`

## Health Monitoring (admin)

```
POST /api/v1/apps/{id}/health-check              — check single app
POST /api/v1/apps/health-check/batch             — check all approved apps
GET  /api/v1/apps/{id}/health                    — health check history
GET  /api/v1/apps/health/summary                 — overview of all app health
```

## Admin Workflows

```
GET  /api/v1/apps/pending                        — list pending apps
POST /api/v1/apps/{id}/approve                   — approve app
POST /api/v1/apps/{id}/reject                    — reject app (requires reason)
POST /api/v1/apps/{id}/deprecate                 — deprecate app (reason, optional replacement)
POST /api/v1/apps/{id}/undeprecate               — restore deprecated app
```

## Webhooks (admin)

```
POST   /api/v1/webhooks                          — register webhook (returns HMAC secret)
GET    /api/v1/webhooks                          — list webhooks
PATCH  /api/v1/webhooks/{id}                     — update webhook
DELETE /api/v1/webhooks/{id}                     — delete webhook
```

Events: `app.submitted`, `app.approved`, `app.rejected`, `app.updated`, `app.deleted`, `review.submitted`, `health.checked`, `app.deprecated`, `app.undeprecated`

## Real-Time Events

```
GET /api/v1/events/stream                        — SSE event stream (public, no auth)
```

## Protocols

`rest`, `graphql`, `grpc`, `mcp`, `a2a`, `websocket`, `other`

## Content Negotiation

`GET /{slug}` returns JSON for agents (auto-detected via User-Agent), HTML for browsers.

## Service Discovery

```
GET /api/v1/health                               — { status, version, service }
GET /api/v1/openapi.json                         — OpenAPI 3.1.0 spec
GET /SKILL.md                                    — this file
GET /llms.txt                                    — alias for SKILL.md
GET /.well-known/skills/index.json               — machine-readable skill registry
```

## Gotchas

- Slugs auto-generated from app name (lowercased, special chars → dashes)
- Apps start as "pending" — not visible in default listing until approved
- Edit token shown only on submission — save immediately
- `?status=all` needed to see pending/rejected apps
- Tags are comma-separated strings, searchable

## Source

GitHub: https://github.com/Humans-Not-Required/app-directory
