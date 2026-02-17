use rocket::http::{ContentType, Status};
use rocket::serde::json::Json;
use serde_json::{json, Value};

use rocket::response::stream::{Event, EventStream};
use rocket::tokio::select;
use rocket::tokio::time::Duration;
use rocket::Shutdown;

use crate::events::EventBus;

// === LLMs.txt ===

#[get("/llms.txt")]
pub fn llms_txt() -> (ContentType, &'static str) {
    (ContentType::Text, include_str!("../../llms.txt"))
}

/// Root-level /llms.txt for standard discovery (outside /api/v1)
#[get("/llms.txt", rank = 2)]
pub fn root_llms_txt() -> (ContentType, &'static str) {
    (ContentType::Text, include_str!("../../llms.txt"))
}

// === Health ===

#[get("/health")]
pub fn health() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "service": "app-directory",
        "version": "0.1.0"
    }))
}

// === CORS Preflight ===

#[options("/<_path..>")]
pub fn cors_preflight(_path: std::path::PathBuf) -> Status {
    Status::NoContent
}

// === SSE Event Stream ===

#[get("/events/stream")]
pub fn event_stream(
    bus: &rocket::State<EventBus>,
    mut shutdown: Shutdown,
) -> EventStream![] {
    let mut rx = bus.subscribe();

    EventStream! {
        loop {
            select! {
                msg = rx.recv() => match msg {
                    Ok(event) => {
                        yield Event::json(&event.data).event(event.event);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        yield Event::data("events_lost").event("warning".to_string());
                    }
                },
                _ = &mut shutdown => break,
            }
        }
    }
    .heartbeat(Duration::from_secs(15))
}

// === OpenAPI Spec ===

#[get("/openapi.json")]
pub fn openapi() -> (Status, (rocket::http::ContentType, String)) {
    let spec = include_str!("../../openapi.json");
    (
        Status::Ok,
        (rocket::http::ContentType::JSON, spec.to_string()),
    )
}

// === Well-Known Skills Discovery (Cloudflare RFC) ===

#[get("/.well-known/skills/index.json")]
pub fn skills_index() -> (ContentType, &'static str) {
    (ContentType::JSON, SKILLS_INDEX_JSON)
}

#[get("/.well-known/skills/app-directory/SKILL.md")]
pub fn skills_skill_md() -> (ContentType, &'static str) {
    (ContentType::Markdown, SKILL_MD_CONTENT)
}

const SKILLS_INDEX_JSON: &str = r#"{
  "skills": [
    {
      "name": "app-directory",
      "description": "Discover, submit, and review agent-native applications. A curated registry for AI agent tools and services with categories, search, deprecation tracking, and admin workflows.",
      "files": [
        "SKILL.md"
      ]
    }
  ]
}"#;

const SKILL_MD_CONTENT: &str = r##"---
name: app-directory
description: Discover, submit, and review agent-native applications. A curated registry for AI agent tools and services with categories, search, deprecation tracking, and admin workflows.
---

# App Directory Integration

A curated registry of agent-native applications. Submit apps for review, browse by category, search by name/tag, read reviews, and track deprecations. Designed for AI agents to discover tools and services.

## Quick Start

1. **Browse apps:**
   ```
   GET /api/v1/apps
   ```

2. **Search:**
   ```
   GET /api/v1/apps?search=monitoring&category=infrastructure
   ```

3. **Submit an app:**
   ```
   POST /api/v1/apps/submit
   {"name": "My Service", "description": "What it does", "url": "https://...", "category": "tools", "protocol": "rest"}
   ```
   Returns `edit_token` — save it for future edits.

4. **Get app details:**
   ```
   GET /api/v1/apps/{slug}
   ```

## Auth Model

- **No auth** to browse, search, or read reviews
- **Admin key** required for approvals, rejections, deprecation management
- **Edit token** returned on submission — required to update your own app
- Pass admin key via: `Authorization: Bearer <key>`, `X-API-Key: <key>`, or `?key=<key>`

## Core Patterns

### App Discovery
```
GET /api/v1/apps                           — List approved apps
GET /api/v1/apps?search=keyword            — Search by name/description
GET /api/v1/apps?category=infrastructure   — Filter by category
GET /api/v1/apps?status=all                — Include pending/rejected
GET /api/v1/apps?sort=name|oldest          — Sort order
GET /api/v1/apps?page=2&per_page=20        — Pagination
GET /api/v1/apps/{slug}                    — App details by slug
```

### App Submission
```
POST /api/v1/apps/submit
{
  "name": "My App",
  "description": "What it does",
  "url": "https://my-app.com",
  "category": "tools|infrastructure|communication|data|ai|other",
  "protocol": "rest|grpc|websocket|graphql|mcp|other",
  "tags": "monitoring,alerting"
}
```
Returns `edit_token` for future updates. Status starts as "pending" until admin approval.

### Reviews
```
POST /api/v1/apps/{id}/reviews
{"reviewer": "agent-name", "rating": 4, "comment": "Works great"}

GET /api/v1/apps/{id}/reviews?page=1&per_page=10
```
One review per reviewer per app (upsert behavior). Rating: 1-5.

### Your Apps
```
GET /api/v1/apps/mine?edit_token=<token>
```
List apps you submitted using your edit token.

### Deprecation
```
POST   /api/v1/admin/apps/{id}/deprecate    — Mark as deprecated (with optional replacement_id)
DELETE /api/v1/admin/apps/{id}/deprecate     — Remove deprecation
```
Self-deprecation (replacing with yourself) is rejected.

### Admin Workflows
```
GET  /api/v1/admin/pending                  — List pending submissions
POST /api/v1/admin/apps/{id}/approve        — Approve
POST /api/v1/admin/apps/{id}/reject         — Reject (with reason)
```

### SSE Real-Time Events
```
GET /api/v1/events/stream
```
Events for app submissions, approvals, reviews, and status changes.

### Webhooks
```
POST   /api/v1/webhooks     — Register webhook URL (admin key)
GET    /api/v1/webhooks     — List webhooks
PATCH  /api/v1/webhooks/{id} — Update
DELETE /api/v1/webhooks/{id} — Delete
```

## Categories

`tools`, `infrastructure`, `communication`, `data`, `ai`, `other`

## Gotchas

- Slugs are auto-generated from app name (lowercased, special chars → dashes)
- Apps start as "pending" — not visible in default listing until approved
- Edit token is shown only on submission — save it immediately
- Reviews use upsert: same reviewer submitting again updates their existing review
- Deprecation requires an existing approved app as replacement (unless clearing)
- `?status=all` needed to see pending/rejected apps
- Tags are comma-separated strings, searchable

## Full API Reference

See `/llms.txt` for complete endpoint documentation and `/api/v1/openapi.json` for the OpenAPI specification.
"##;
