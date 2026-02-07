use rocket::http::Status;
use rocket::serde::json::Json;
use serde_json::{json, Value};

use crate::auth::AuthenticatedKey;
use crate::DbState;

/// Record a view event for an app.
/// Called internally from get_app route.
pub fn record_view(conn: &rusqlite::Connection, app_id: &str, viewer_key_id: &str) {
    let id = uuid::Uuid::new_v4().to_string();
    let _ = conn.execute(
        "INSERT INTO app_views (id, app_id, viewer_key_id) VALUES (?1, ?2, ?3)",
        rusqlite::params![id, app_id, viewer_key_id],
    );
}

/// Get statistics for a single app.
/// Returns total views, views in last 24h, 7d, 30d, and unique viewers.
#[get("/apps/<id>/stats")]
pub fn get_app_stats(
    _key: AuthenticatedKey,
    id: &str,
    db: &rocket::State<DbState>,
) -> (Status, Json<Value>) {
    let conn = db.0.lock().unwrap();

    // Check app exists
    let app_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM apps WHERE id = ?1 OR slug = ?1",
            rusqlite::params![id],
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

    // Resolve to canonical ID if slug was provided
    let app_id: String = conn
        .query_row(
            "SELECT id FROM apps WHERE id = ?1 OR slug = ?1",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .unwrap();

    let total_views: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM app_views WHERE app_id = ?1",
            rusqlite::params![app_id],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let views_24h: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM app_views WHERE app_id = ?1 AND viewed_at >= datetime('now', '-1 day')",
            rusqlite::params![app_id],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let views_7d: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM app_views WHERE app_id = ?1 AND viewed_at >= datetime('now', '-7 days')",
            rusqlite::params![app_id],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let views_30d: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM app_views WHERE app_id = ?1 AND viewed_at >= datetime('now', '-30 days')",
            rusqlite::params![app_id],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let unique_viewers: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT viewer_key_id) FROM app_views WHERE app_id = ?1",
            rusqlite::params![app_id],
            |r| r.get(0),
        )
        .unwrap_or(0);

    (
        Status::Ok,
        Json(json!({
            "app_id": app_id,
            "total_views": total_views,
            "views_24h": views_24h,
            "views_7d": views_7d,
            "views_30d": views_30d,
            "unique_viewers": unique_viewers,
        })),
    )
}

/// Trending apps — ranked by views in the last 7 days.
/// Returns apps with their view counts and velocity (views per day).
#[get("/apps/trending?<days>&<limit>")]
pub fn trending_apps(
    _key: AuthenticatedKey,
    days: Option<i64>,
    limit: Option<i64>,
    db: &rocket::State<DbState>,
) -> Json<Value> {
    let conn = db.0.lock().unwrap();

    let days = days.unwrap_or(7).clamp(1, 90);
    let limit = limit.unwrap_or(10).clamp(1, 50);
    let interval = format!("-{} days", days);

    let mut stmt = conn
        .prepare(
            "SELECT a.id, a.name, a.slug, a.short_description, a.protocol, a.category,
                    a.tags, a.is_featured, a.is_verified, a.avg_rating, a.review_count,
                    COUNT(v.id) as view_count,
                    COUNT(DISTINCT v.viewer_key_id) as unique_viewers
             FROM apps a
             LEFT JOIN app_views v ON v.app_id = a.id AND v.viewed_at >= datetime('now', ?1)
             WHERE a.status = 'approved'
             GROUP BY a.id
             HAVING view_count > 0
             ORDER BY view_count DESC, unique_viewers DESC
             LIMIT ?2",
        )
        .unwrap();

    let apps: Vec<Value> = stmt
        .query_map(rusqlite::params![interval, limit], |row| {
            let tags_str: String = row.get(6)?;
            let tags: Vec<String> = serde_json::from_str(&tags_str).unwrap_or_default();
            let view_count: i64 = row.get(11)?;
            let unique_viewers: i64 = row.get(12)?;
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "slug": row.get::<_, String>(2)?,
                "short_description": row.get::<_, String>(3)?,
                "protocol": row.get::<_, String>(4)?,
                "category": row.get::<_, String>(5)?,
                "tags": tags,
                "is_featured": row.get::<_, i32>(7)? != 0,
                "is_verified": row.get::<_, i32>(8)? != 0,
                "avg_rating": row.get::<_, f64>(9)?,
                "review_count": row.get::<_, i64>(10)?,
                "view_count": view_count,
                "unique_viewers": unique_viewers,
                "views_per_day": (view_count as f64) / (days as f64),
            }))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    Json(json!({
        "trending": apps,
        "period_days": days,
    }))
}
