use rocket::http::Status;
use rocket::serde::json::Json;
use serde_json::{json, Value};

use crate::auth::AuthenticatedKey;
use crate::events::{AppEvent, EventBus};
use crate::DbState;

#[derive(Debug, serde::Deserialize)]
pub struct ApproveRequest {
    pub note: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub struct RejectRequest {
    pub reason: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct DeprecateRequest {
    pub reason: String,
    pub replacement_app_id: Option<String>,
    pub sunset_at: Option<String>,
}

/// Approve a pending app. Admin only.
#[post("/apps/<id>/approve", format = "json", data = "<body>")]
pub fn approve_app(
    key: AuthenticatedKey,
    id: &str,
    body: Json<ApproveRequest>,
    db: &rocket::State<DbState>,
    bus: &rocket::State<EventBus>,
) -> (Status, Json<Value>) {
    if !key.is_admin {
        return (
            Status::Forbidden,
            Json(json!({ "error": "ADMIN_REQUIRED", "message": "Only admins can approve apps" })),
        );
    }

    let conn = db.0.lock().unwrap();

    let current: Result<(String, String), _> = conn.query_row(
        "SELECT status, name FROM apps WHERE id = ?1",
        rusqlite::params![id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    );

    let (current_status, app_name) = match current {
        Ok(v) => v,
        Err(_) => {
            return (
                Status::NotFound,
                Json(json!({ "error": "NOT_FOUND", "message": "App not found" })),
            )
        }
    };

    if current_status == "approved" {
        return (
            Status::Conflict,
            Json(json!({ "error": "ALREADY_APPROVED", "message": "App is already approved" })),
        );
    }

    if current_status == "deprecated" {
        return (
            Status::Conflict,
            Json(
                json!({ "error": "INVALID_TRANSITION", "message": "Cannot approve a deprecated app. Unset deprecated status first." }),
            ),
        );
    }

    match conn.execute(
        "UPDATE apps SET status = 'approved', review_note = ?1, reviewed_by = ?2, reviewed_at = datetime('now'), updated_at = datetime('now') WHERE id = ?3",
        rusqlite::params![body.note, key.id, id],
    ) {
        Ok(1) => {
            bus.emit(AppEvent {
                event: "app.approved".to_string(),
                data: json!({
                    "app_id": id,
                    "name": app_name,
                    "previous_status": current_status,
                    "reviewed_by": key.id,
                    "note": body.note,
                }),
            });

            (
                Status::Ok,
                Json(json!({
                    "message": "App approved",
                    "app_id": id,
                    "previous_status": current_status,
                })),
            )
        }
        Ok(_) => (
            Status::NotFound,
            Json(json!({ "error": "NOT_FOUND", "message": "App not found" })),
        ),
        Err(e) => (
            Status::InternalServerError,
            Json(json!({ "error": "DB_ERROR", "message": e.to_string() })),
        ),
    }
}

/// Reject a pending app. Admin only. Requires a reason.
#[post("/apps/<id>/reject", format = "json", data = "<body>")]
pub fn reject_app(
    key: AuthenticatedKey,
    id: &str,
    body: Json<RejectRequest>,
    db: &rocket::State<DbState>,
    bus: &rocket::State<EventBus>,
) -> (Status, Json<Value>) {
    if !key.is_admin {
        return (
            Status::Forbidden,
            Json(json!({ "error": "ADMIN_REQUIRED", "message": "Only admins can reject apps" })),
        );
    }

    if body.reason.trim().is_empty() {
        return (
            Status::BadRequest,
            Json(
                json!({ "error": "REASON_REQUIRED", "message": "A reason is required when rejecting an app" }),
            ),
        );
    }

    let conn = db.0.lock().unwrap();

    let current: Result<(String, String), _> = conn.query_row(
        "SELECT status, name FROM apps WHERE id = ?1",
        rusqlite::params![id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    );

    let (current_status, app_name) = match current {
        Ok(v) => v,
        Err(_) => {
            return (
                Status::NotFound,
                Json(json!({ "error": "NOT_FOUND", "message": "App not found" })),
            )
        }
    };

    if current_status == "rejected" {
        return (
            Status::Conflict,
            Json(json!({ "error": "ALREADY_REJECTED", "message": "App is already rejected" })),
        );
    }

    if current_status == "deprecated" {
        return (
            Status::Conflict,
            Json(
                json!({ "error": "INVALID_TRANSITION", "message": "Cannot reject a deprecated app" }),
            ),
        );
    }

    match conn.execute(
        "UPDATE apps SET status = 'rejected', review_note = ?1, reviewed_by = ?2, reviewed_at = datetime('now'), updated_at = datetime('now') WHERE id = ?3",
        rusqlite::params![body.reason, key.id, id],
    ) {
        Ok(1) => {
            bus.emit(AppEvent {
                event: "app.rejected".to_string(),
                data: json!({
                    "app_id": id,
                    "name": app_name,
                    "previous_status": current_status,
                    "reviewed_by": key.id,
                    "reason": body.reason,
                }),
            });

            (
                Status::Ok,
                Json(json!({
                    "message": "App rejected",
                    "app_id": id,
                    "previous_status": current_status,
                    "reason": body.reason,
                })),
            )
        }
        Ok(_) => (
            Status::NotFound,
            Json(json!({ "error": "NOT_FOUND", "message": "App not found" })),
        ),
        Err(e) => (
            Status::InternalServerError,
            Json(json!({ "error": "DB_ERROR", "message": e.to_string() })),
        ),
    }
}

/// Deprecate an app. Admin only.
#[post("/apps/<id>/deprecate", format = "json", data = "<body>")]
pub fn deprecate_app(
    key: AuthenticatedKey,
    id: &str,
    body: Json<DeprecateRequest>,
    db: &rocket::State<DbState>,
    bus: &rocket::State<EventBus>,
) -> (Status, Json<Value>) {
    if !key.is_admin {
        return (
            Status::Forbidden,
            Json(json!({ "error": "ADMIN_REQUIRED", "message": "Only admins can deprecate apps" })),
        );
    }

    if body.reason.trim().is_empty() {
        return (
            Status::BadRequest,
            Json(
                json!({ "error": "REASON_REQUIRED", "message": "A reason is required when deprecating an app" }),
            ),
        );
    }

    let conn = db.0.lock().unwrap();

    if let Some(ref replacement_id) = body.replacement_app_id {
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM apps WHERE id = ?1",
                rusqlite::params![replacement_id],
                |r| r.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if !exists {
            return (
                Status::BadRequest,
                Json(json!({
                    "error": "INVALID_REPLACEMENT",
                    "message": "Replacement app not found"
                })),
            );
        }

        if replacement_id == id {
            return (
                Status::BadRequest,
                Json(json!({
                    "error": "INVALID_REPLACEMENT",
                    "message": "An app cannot replace itself"
                })),
            );
        }
    }

    let current: Result<(String, String), _> = conn.query_row(
        "SELECT status, name FROM apps WHERE id = ?1",
        rusqlite::params![id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    );

    let (current_status, app_name) = match current {
        Ok(v) => v,
        Err(_) => {
            return (
                Status::NotFound,
                Json(json!({ "error": "NOT_FOUND", "message": "App not found" })),
            )
        }
    };

    if current_status == "deprecated" {
        return (
            Status::Conflict,
            Json(json!({ "error": "ALREADY_DEPRECATED", "message": "App is already deprecated" })),
        );
    }

    match conn.execute(
        "UPDATE apps SET status = 'deprecated', deprecated_reason = ?1, deprecated_by = ?2, deprecated_at = datetime('now'), replacement_app_id = ?3, sunset_at = ?4, updated_at = datetime('now') WHERE id = ?5",
        rusqlite::params![body.reason, key.id, body.replacement_app_id, body.sunset_at, id],
    ) {
        Ok(1) => {
            bus.emit(AppEvent {
                event: "app.deprecated".to_string(),
                data: json!({
                    "app_id": id,
                    "name": app_name,
                    "previous_status": current_status,
                    "deprecated_by": key.id,
                    "reason": body.reason,
                    "replacement_app_id": body.replacement_app_id,
                    "sunset_at": body.sunset_at,
                }),
            });

            (
                Status::Ok,
                Json(json!({
                    "message": "App deprecated",
                    "app_id": id,
                    "previous_status": current_status,
                    "reason": body.reason,
                    "replacement_app_id": body.replacement_app_id,
                    "sunset_at": body.sunset_at,
                })),
            )
        }
        Ok(_) => (
            Status::NotFound,
            Json(json!({ "error": "NOT_FOUND", "message": "App not found" })),
        ),
        Err(e) => (
            Status::InternalServerError,
            Json(json!({ "error": "DB_ERROR", "message": e.to_string() })),
        ),
    }
}

/// Undeprecate an app. Admin only.
#[post("/apps/<id>/undeprecate")]
pub fn undeprecate_app(
    key: AuthenticatedKey,
    id: &str,
    db: &rocket::State<DbState>,
    bus: &rocket::State<EventBus>,
) -> (Status, Json<Value>) {
    if !key.is_admin {
        return (
            Status::Forbidden,
            Json(
                json!({ "error": "ADMIN_REQUIRED", "message": "Only admins can undeprecate apps" }),
            ),
        );
    }

    let conn = db.0.lock().unwrap();

    let current: Result<(String, String), _> = conn.query_row(
        "SELECT status, name FROM apps WHERE id = ?1",
        rusqlite::params![id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    );

    let (current_status, app_name) = match current {
        Ok(v) => v,
        Err(_) => {
            return (
                Status::NotFound,
                Json(json!({ "error": "NOT_FOUND", "message": "App not found" })),
            )
        }
    };

    if current_status != "deprecated" {
        return (
            Status::Conflict,
            Json(json!({ "error": "NOT_DEPRECATED", "message": "App is not deprecated" })),
        );
    }

    match conn.execute(
        "UPDATE apps SET status = 'approved', deprecated_reason = NULL, deprecated_by = NULL, deprecated_at = NULL, replacement_app_id = NULL, sunset_at = NULL, updated_at = datetime('now') WHERE id = ?1",
        rusqlite::params![id],
    ) {
        Ok(1) => {
            bus.emit(AppEvent {
                event: "app.undeprecated".to_string(),
                data: json!({
                    "app_id": id,
                    "name": app_name,
                    "restored_to": "approved",
                    "undeprecated_by": key.id,
                }),
            });

            (
                Status::Ok,
                Json(json!({
                    "message": "App undeprecated",
                    "app_id": id,
                    "restored_to": "approved",
                })),
            )
        }
        Ok(_) => (
            Status::NotFound,
            Json(json!({ "error": "NOT_FOUND", "message": "App not found" })),
        ),
        Err(e) => (
            Status::InternalServerError,
            Json(json!({ "error": "DB_ERROR", "message": e.to_string() })),
        ),
    }
}
