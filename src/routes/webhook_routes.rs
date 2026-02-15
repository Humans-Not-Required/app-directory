use rocket::http::Status;
use rocket::serde::json::Json;
use serde_json::{json, Value};

use crate::auth::AuthenticatedKey;
use crate::DbState;

#[derive(Debug, serde::Deserialize)]
pub struct CreateWebhookRequest {
    pub url: String,
    pub events: Option<Vec<String>>,
}

#[derive(Debug, serde::Serialize)]
pub struct WebhookResponse {
    pub id: String,
    pub url: String,
    pub events: Vec<String>,
    pub active: bool,
    pub failure_count: i64,
    pub last_triggered_at: Option<String>,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub struct UpdateWebhookRequest {
    pub url: Option<String>,
    pub events: Option<Vec<String>>,
    pub active: Option<bool>,
}

static VALID_WEBHOOK_EVENTS: &[&str] = &[
    "app.submitted",
    "app.approved",
    "app.rejected",
    "app.deprecated",
    "app.undeprecated",
    "app.updated",
    "app.deleted",
    "review.submitted",
    "health.checked",
];

/// Register a webhook. Admin only.
#[post("/webhooks", format = "json", data = "<body>")]
pub fn create_webhook(
    key: AuthenticatedKey,
    body: Json<CreateWebhookRequest>,
    db: &rocket::State<DbState>,
) -> (Status, Json<Value>) {
    if !key.is_admin {
        return (
            Status::Forbidden,
            Json(json!({ "error": "ADMIN_REQUIRED" })),
        );
    }

    let url = body.url.trim();
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return (
            Status::BadRequest,
            Json(
                json!({ "error": "INVALID_URL", "message": "URL must start with http:// or https://" }),
            ),
        );
    }

    let events = body.events.clone().unwrap_or_default();
    for evt in &events {
        if !VALID_WEBHOOK_EVENTS.contains(&evt.as_str()) {
            return (
                Status::BadRequest,
                Json(json!({
                    "error": "INVALID_EVENT",
                    "message": format!("Invalid event '{}'. Valid: {}", evt, VALID_WEBHOOK_EVENTS.join(", "))
                })),
            );
        }
    }

    let conn = db.0.lock().unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let secret = format!(
        "whsec_{}",
        uuid::Uuid::new_v4().to_string().replace('-', "")
    );
    let events_json = serde_json::to_string(&events).unwrap();

    match conn.execute(
        "INSERT INTO webhooks (id, url, secret, events, created_by) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![id, url, secret, events_json, key.id],
    ) {
        Ok(_) => (
            Status::Created,
            Json(json!(WebhookResponse {
                id,
                url: url.to_string(),
                events,
                active: true,
                failure_count: 0,
                last_triggered_at: None,
                created_at: chrono::Utc::now().to_rfc3339(),
                secret: Some(secret),
            })),
        ),
        Err(e) => (
            Status::InternalServerError,
            Json(json!({ "error": "DB_ERROR", "message": e.to_string() })),
        ),
    }
}

/// List all webhooks. Admin only.
#[get("/webhooks")]
pub fn list_webhooks(key: AuthenticatedKey, db: &rocket::State<DbState>) -> (Status, Json<Value>) {
    if !key.is_admin {
        return (
            Status::Forbidden,
            Json(json!({ "error": "ADMIN_REQUIRED" })),
        );
    }

    let conn = db.0.lock().unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT id, url, events, active, failure_count, last_triggered_at, created_at
             FROM webhooks ORDER BY created_at DESC",
        )
        .unwrap();

    let webhooks: Vec<WebhookResponse> = stmt
        .query_map([], |row| {
            let events_str: String = row.get(2)?;
            let events: Vec<String> = serde_json::from_str(&events_str).unwrap_or_default();
            Ok(WebhookResponse {
                id: row.get(0)?,
                url: row.get(1)?,
                events,
                active: row.get::<_, i32>(3)? != 0,
                failure_count: row.get(4)?,
                last_triggered_at: row.get(5)?,
                created_at: row.get(6)?,
                secret: None,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    (Status::Ok, Json(json!({ "webhooks": webhooks })))
}

/// Update a webhook (URL, events, active). Admin only.
#[patch("/webhooks/<webhook_id>", format = "json", data = "<body>")]
pub fn update_webhook(
    key: AuthenticatedKey,
    webhook_id: &str,
    body: Json<UpdateWebhookRequest>,
    db: &rocket::State<DbState>,
) -> (Status, Json<Value>) {
    if !key.is_admin {
        return (
            Status::Forbidden,
            Json(json!({ "error": "ADMIN_REQUIRED" })),
        );
    }

    let conn = db.0.lock().unwrap();

    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM webhooks WHERE id = ?1",
            rusqlite::params![webhook_id],
            |r| r.get(0),
        )
        .unwrap_or(false);

    if !exists {
        return (
            Status::NotFound,
            Json(json!({ "error": "NOT_FOUND", "message": "Webhook not found" })),
        );
    }

    if let Some(ref url) = body.url {
        let url = url.trim();
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return (
                Status::BadRequest,
                Json(
                    json!({ "error": "INVALID_URL", "message": "URL must start with http:// or https://" }),
                ),
            );
        }
        let _ = conn.execute(
            "UPDATE webhooks SET url = ?1 WHERE id = ?2",
            rusqlite::params![url, webhook_id],
        );
    }

    if let Some(ref events) = body.events {
        for evt in events {
            if !VALID_WEBHOOK_EVENTS.contains(&evt.as_str()) {
                return (
                    Status::BadRequest,
                    Json(json!({
                        "error": "INVALID_EVENT",
                        "message": format!("Invalid event '{}'. Valid: {}", evt, VALID_WEBHOOK_EVENTS.join(", "))
                    })),
                );
            }
        }
        let events_json = serde_json::to_string(events).unwrap();
        let _ = conn.execute(
            "UPDATE webhooks SET events = ?1 WHERE id = ?2",
            rusqlite::params![events_json, webhook_id],
        );
    }

    if let Some(active) = body.active {
        if active {
            let _ = conn.execute(
                "UPDATE webhooks SET active = 1, failure_count = 0 WHERE id = ?1",
                rusqlite::params![webhook_id],
            );
        } else {
            let _ = conn.execute(
                "UPDATE webhooks SET active = 0 WHERE id = ?1",
                rusqlite::params![webhook_id],
            );
        }
    }

    let result = conn.query_row(
        "SELECT id, url, events, active, failure_count, last_triggered_at, created_at FROM webhooks WHERE id = ?1",
        rusqlite::params![webhook_id],
        |row| {
            let events_str: String = row.get(2)?;
            let events: Vec<String> = serde_json::from_str(&events_str).unwrap_or_default();
            Ok(WebhookResponse {
                id: row.get(0)?,
                url: row.get(1)?,
                events,
                active: row.get::<_, i32>(3)? != 0,
                failure_count: row.get(4)?,
                last_triggered_at: row.get(5)?,
                created_at: row.get(6)?,
                secret: None,
            })
        },
    );

    match result {
        Ok(wh) => (Status::Ok, Json(json!(wh))),
        Err(e) => (
            Status::InternalServerError,
            Json(json!({ "error": "DB_ERROR", "message": e.to_string() })),
        ),
    }
}

/// Delete a webhook. Admin only.
#[delete("/webhooks/<webhook_id>")]
pub fn delete_webhook(
    key: AuthenticatedKey,
    webhook_id: &str,
    db: &rocket::State<DbState>,
) -> (Status, Json<Value>) {
    if !key.is_admin {
        return (
            Status::Forbidden,
            Json(json!({ "error": "ADMIN_REQUIRED" })),
        );
    }

    let conn = db.0.lock().unwrap();
    match conn.execute(
        "DELETE FROM webhooks WHERE id = ?1",
        rusqlite::params![webhook_id],
    ) {
        Ok(1) => (Status::Ok, Json(json!({ "message": "Webhook deleted" }))),
        Ok(_) => (
            Status::NotFound,
            Json(json!({ "error": "NOT_FOUND", "message": "Webhook not found" })),
        ),
        Err(e) => (
            Status::InternalServerError,
            Json(json!({ "error": "DB_ERROR", "message": e.to_string() })),
        ),
    }
}
