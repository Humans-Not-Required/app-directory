use rocket::http::{ContentType, Status};
use rocket::serde::json::Json;
use serde_json::{json, Value};

use rocket::response::stream::{Event, EventStream};
use rocket::tokio::select;
use rocket::tokio::time::Duration;
use rocket::Shutdown;

use crate::events::EventBus;

// === SKILL.md / llms.txt ===

/// GET /SKILL.md — canonical AI-readable service guide
#[get("/SKILL.md")]
pub fn skill_md() -> (ContentType, &'static str) {
    (ContentType::Plain, include_str!("../../SKILL.md"))
}

/// GET /llms.txt — alias for SKILL.md (backward-compatible)
#[get("/llms.txt")]
pub fn llms_txt() -> (ContentType, &'static str) {
    (ContentType::Plain, include_str!("../../SKILL.md"))
}

/// Root-level /llms.txt for standard discovery (outside /api/v1)
#[get("/llms.txt", rank = 2)]
pub fn root_llms_txt() -> (ContentType, &'static str) {
    (ContentType::Plain, include_str!("../../SKILL.md"))
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
    (ContentType::Plain, include_str!("../../SKILL.md"))
}

/// GET /skills/SKILL.md — alternate path for agent discoverability
#[get("/skills/SKILL.md")]
pub fn api_skills_skill_md() -> (ContentType, &'static str) {
    (ContentType::Plain, include_str!("../../SKILL.md"))
}

const SKILLS_INDEX_JSON: &str = r#"{
  "skills": [
    {
      "name": "app-directory",
      "description": "Discover, submit, and review agent-native applications. A curated registry for AI agent tools and services with categories, search, deprecation tracking, and admin workflows.",
      "url": "/SKILL.md",
      "files": [
        "SKILL.md"
      ]
    }
  ]
}"#;

// SKILL_MD_CONTENT removed — now served via include_str!("../../SKILL.md")
