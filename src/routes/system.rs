use rocket::http::{ContentType, Status};
use rocket::serde::json::Json;
use serde_json::{json, Value};

use rocket::response::stream::{Event, EventStream};
use rocket::tokio::select;
use rocket::tokio::time::Duration;
use rocket::Shutdown;

use crate::auth::AuthenticatedKey;
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
    _key: AuthenticatedKey,
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
