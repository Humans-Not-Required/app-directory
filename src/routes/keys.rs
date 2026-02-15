use rocket::http::Status;
use rocket::serde::json::Json;
use serde_json::{json, Value};

use crate::auth::{self, AuthenticatedKey, OptionalKey};
use crate::models;
use crate::DbState;

// === Admin: API Keys ===

#[get("/keys")]
pub fn list_keys(key: AuthenticatedKey, db: &rocket::State<DbState>) -> (Status, Json<Value>) {
    if !key.is_admin {
        return (
            Status::Forbidden,
            Json(json!({ "error": "ADMIN_REQUIRED" })),
        );
    }

    let conn = db.0.lock().unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT id, name, is_admin, rate_limit, created_at FROM api_keys WHERE revoked = 0",
        )
        .unwrap();

    let keys: Vec<Value> = stmt
        .query_map([], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "is_admin": row.get::<_, i32>(2)? != 0,
                "rate_limit": row.get::<_, i64>(3)?,
                "created_at": row.get::<_, String>(4)?,
            }))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    (Status::Ok, Json(json!({ "keys": keys })))
}

#[post("/keys", data = "<body>")]
pub fn create_key(
    opt_key: OptionalKey,
    body: Json<models::CreateKeyRequest>,
    db: &rocket::State<DbState>,
) -> (Status, Json<Value>) {
    let is_admin_request = body.is_admin.unwrap_or(false);
    let requester_is_admin = opt_key.0.as_ref().map(|k| k.is_admin).unwrap_or(false);

    if is_admin_request && !requester_is_admin {
        return (
            Status::Forbidden,
            Json(json!({ "error": "ADMIN_REQUIRED", "message": "Only admins can create admin keys" })),
        );
    }

    let conn = db.0.lock().unwrap();
    let raw_key = auth::create_api_key(
        &conn,
        &body.name,
        is_admin_request,
        body.rate_limit,
    );

    (
        Status::Created,
        Json(json!({
            "api_key": raw_key,
            "message": "Save this key — it won't be shown again"
        })),
    )
}

#[delete("/keys/<id>")]
pub fn delete_key(
    key: AuthenticatedKey,
    id: &str,
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
        "UPDATE api_keys SET revoked = 1 WHERE id = ?1",
        rusqlite::params![id],
    ) {
        Ok(1) => (Status::Ok, Json(json!({ "message": "Key revoked" }))),
        Ok(_) => (Status::NotFound, Json(json!({ "error": "NOT_FOUND" }))),
        Err(e) => (
            Status::InternalServerError,
            Json(json!({ "error": "DB_ERROR", "message": e.to_string() })),
        ),
    }
}
