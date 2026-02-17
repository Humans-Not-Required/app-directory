use rocket::http::Status;
use rocket::serde::json::Json;
use serde_json::{json, Value};

use crate::auth::OptionalKey;
use crate::events::{AppEvent, EventBus};
use crate::models::*;
use crate::DbState;

// === Reviews (NO AUTH REQUIRED) ===

#[post("/apps/<app_id>/reviews", data = "<body>")]
pub fn submit_review(
    opt_key: OptionalKey,
    app_id: &str,
    body: Json<SubmitReviewRequest>,
    db: &rocket::State<DbState>,
    bus: &rocket::State<EventBus>,
) -> (Status, Json<Value>) {
    let conn = db.conn();

    if body.rating < 1 || body.rating > 5 {
        return (
            Status::BadRequest,
            Json(json!({ "error": "INVALID_RATING", "message": "Rating must be 1-5" })),
        );
    }

    let app_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM apps WHERE id = ?1",
            rusqlite::params![app_id],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;

    if !app_exists {
        return (
            Status::NotFound,
            Json(json!({ "error": "NOT_FOUND", "message": "App not found" })),
        );
    }

    let id = uuid::Uuid::new_v4().to_string();
    let reviewer_key_id: Option<String> = opt_key.0.as_ref().map(|k| k.id.clone());
    let reviewer_name = body.reviewer_name.as_deref().unwrap_or("anonymous");

    // If authenticated, upsert (one review per key per app).
    // If anonymous, always insert a new review.
    let result = if let Some(ref key_id) = reviewer_key_id {
        // Check for existing review by this key on this app
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM reviews WHERE app_id = ?1 AND reviewer_key_id = ?2",
                rusqlite::params![app_id, key_id],
                |r| r.get(0),
            )
            .ok();

        if let Some(existing_id) = existing {
            // Update existing review
            conn.execute(
                "UPDATE reviews SET rating = ?1, title = ?2, body = ?3, reviewer_name = ?4,
                 created_at = datetime('now') WHERE id = ?5",
                rusqlite::params![body.rating, body.title, body.body, reviewer_name, existing_id],
            )
        } else {
            conn.execute(
                "INSERT INTO reviews (id, app_id, reviewer_key_id, reviewer_name, rating, title, body)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![id, app_id, key_id, reviewer_name, body.rating, body.title, body.body],
            )
        }
    } else {
        conn.execute(
            "INSERT INTO reviews (id, app_id, reviewer_key_id, reviewer_name, rating, title, body)
             VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6)",
            rusqlite::params![id, app_id, reviewer_name, body.rating, body.title, body.body],
        )
    };

    if let Err(e) = &result {
        eprintln!("Review insert error: {e}");
        return (
            Status::InternalServerError,
            Json(json!({ "error": "DB_ERROR", "message": "Internal server error" })),
        );
    }

    let _ = conn.execute(
        "UPDATE apps SET
           avg_rating = (SELECT COALESCE(AVG(CAST(rating AS REAL)), 0.0) FROM reviews WHERE app_id = ?1),
           review_count = (SELECT COUNT(*) FROM reviews WHERE app_id = ?1),
           updated_at = datetime('now')
         WHERE id = ?1",
        rusqlite::params![app_id],
    );

    bus.emit(AppEvent {
        event: "review.submitted".to_string(),
        data: json!({
            "app_id": app_id,
            "review_id": id,
            "rating": body.rating,
        }),
    });

    (
        Status::Created,
        Json(json!({ "message": "Review submitted", "id": id })),
    )
}

#[get("/apps/<app_id>/reviews?<page>&<per_page>")]
pub fn get_reviews(
    app_id: &str,
    page: Option<i64>,
    per_page: Option<i64>,
    db: &rocket::State<DbState>,
) -> Json<Value> {
    let conn = db.conn();

    let page = page.unwrap_or(1).max(1);
    let per_page = per_page.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * per_page;

    let total: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM reviews WHERE app_id = ?1",
            rusqlite::params![app_id],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let reviews: Vec<Value> = match conn.prepare(
        "SELECT id, app_id, rating, title, body, created_at, reviewer_name
         FROM reviews WHERE app_id = ?1 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
    ) {
        Ok(mut stmt) => {
            match stmt.query_map(rusqlite::params![app_id, per_page, offset], |row| {
                Ok(json!({
                    "id": row.get::<_, String>(0)?,
                    "app_id": row.get::<_, String>(1)?,
                    "rating": row.get::<_, i64>(2)?,
                    "title": row.get::<_, Option<String>>(3)?,
                    "body": row.get::<_, Option<String>>(4)?,
                    "created_at": row.get::<_, String>(5)?,
                    "reviewer_name": row.get::<_, Option<String>>(6)?,
                }))
            }) {
                Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
                Err(_) => Vec::new(),
            }
        }
        Err(_) => Vec::new(),
    };

    Json(json!({
        "reviews": reviews,
        "total": total,
        "page": page,
        "per_page": per_page,
    }))
}

// === Categories (NO AUTH REQUIRED) ===

#[get("/categories")]
pub fn list_categories(db: &rocket::State<DbState>) -> Json<Value> {
    let conn = db.conn();

    let mut stmt = conn
        .prepare(
            "SELECT category, COUNT(*) as count FROM apps WHERE status = 'approved' GROUP BY category ORDER BY count DESC",
        )
        .unwrap();

    let categories: Vec<Value> = stmt
        .query_map([], |row| {
            Ok(json!({
                "name": row.get::<_, String>(0)?,
                "count": row.get::<_, i64>(1)?,
            }))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    Json(json!({
        "categories": categories,
        "valid_categories": VALID_CATEGORIES,
        "valid_protocols": VALID_PROTOCOLS,
    }))
}
