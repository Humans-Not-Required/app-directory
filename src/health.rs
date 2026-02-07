use rocket::http::Status;
use rocket::serde::json::Json;
use serde_json::{json, Value};
use std::time::Instant;

use crate::auth::AuthenticatedKey;
use crate::webhooks::{self, WebhookDb, WebhookEvent};
use crate::DbState;

/// Perform a health check on a single app.
/// Checks the `api_url` (or `homepage_url` if no api_url) with a GET request.
/// Records the result in the `health_checks` table and updates the app's cached status.
#[post("/apps/<app_id>/health-check")]
pub async fn check_app_health(
    key: AuthenticatedKey,
    app_id: &str,
    db: &rocket::State<DbState>,
    webhook_db: &rocket::State<WebhookDb>,
    http_client: &rocket::State<reqwest::Client>,
) -> (Status, Json<Value>) {
    // Only admins can trigger health checks
    if !key.is_admin {
        return (
            Status::Forbidden,
            Json(
                json!({ "error": "ADMIN_REQUIRED", "message": "Only admins can trigger health checks" }),
            ),
        );
    }

    // Get app info (need api_url or homepage_url)
    let app_info = {
        let conn = db.0.lock().unwrap();
        conn.query_row(
            "SELECT id, name, api_url, homepage_url FROM apps WHERE id = ?1 OR slug = ?1",
            rusqlite::params![app_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
    };

    let (id, name, api_url, homepage_url) = match app_info {
        Ok(info) => info,
        Err(_) => {
            return (
                Status::NotFound,
                Json(json!({ "error": "NOT_FOUND", "message": "App not found" })),
            )
        }
    };

    // Determine which URL to check
    let check_url = match api_url.as_deref().or(homepage_url.as_deref()) {
        Some(url) => url.to_string(),
        None => {
            return (
                Status::UnprocessableEntity,
                Json(json!({
                    "error": "NO_URL",
                    "message": "App has no api_url or homepage_url to check"
                })),
            )
        }
    };

    // Perform the health check (with timeout)
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .unwrap();

    let start = Instant::now();
    let result = client.get(&check_url).send().await;
    let response_time_ms = start.elapsed().as_millis() as i64;

    let (health_status, status_code, error_message) = match result {
        Ok(resp) => {
            let code = resp.status().as_u16() as i64;
            if resp.status().is_success() {
                ("healthy".to_string(), Some(code), None)
            } else {
                (
                    "unhealthy".to_string(),
                    Some(code),
                    Some(format!("HTTP {}", resp.status())),
                )
            }
        }
        Err(e) => {
            let msg = if e.is_timeout() {
                "Connection timed out (10s)".to_string()
            } else if e.is_connect() {
                "Connection refused or DNS failure".to_string()
            } else {
                format!("{}", e)
            };
            ("unreachable".to_string(), None, Some(msg))
        }
    };

    // Record the health check and update app
    let check_id = uuid::Uuid::new_v4().to_string();
    {
        let conn = db.0.lock().unwrap();

        // Insert health check record
        let _ = conn.execute(
            "INSERT INTO health_checks (id, app_id, status, status_code, response_time_ms, error_message, checked_url)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                check_id,
                id,
                health_status,
                status_code,
                response_time_ms,
                error_message,
                check_url,
            ],
        );

        // Update app's cached health status
        let _ = conn.execute(
            "UPDATE apps SET last_health_status = ?1, last_checked_at = datetime('now'), updated_at = datetime('now') WHERE id = ?2",
            rusqlite::params![health_status, id],
        );

        // Recalculate uptime percentage (last 100 checks)
        let uptime: Option<f64> = conn
            .query_row(
                "SELECT CAST(SUM(CASE WHEN status = 'healthy' THEN 1 ELSE 0 END) AS REAL) / COUNT(*) * 100.0
                 FROM (SELECT status FROM health_checks WHERE app_id = ?1 ORDER BY checked_at DESC LIMIT 100)",
                rusqlite::params![id],
                |row| row.get(0),
            )
            .ok();

        if let Some(uptime_val) = uptime {
            let _ = conn.execute(
                "UPDATE apps SET uptime_pct = ?1 WHERE id = ?2",
                rusqlite::params![uptime_val, id],
            );
        }
    }

    webhooks::deliver_webhooks(
        (*webhook_db).clone(),
        WebhookEvent {
            event: "health.checked".to_string(),
            data: json!({
                "app_id": id,
                "app_name": name,
                "status": health_status,
                "status_code": status_code,
                "response_time_ms": response_time_ms,
            }),
        },
        (*http_client).clone(),
    );

    (
        Status::Ok,
        Json(json!({
            "id": check_id,
            "app_id": id,
            "app_name": name,
            "checked_url": check_url,
            "status": health_status,
            "status_code": status_code,
            "response_time_ms": response_time_ms,
            "error_message": error_message,
        })),
    )
}

/// Batch health check: check all approved apps that have a URL.
/// Returns a summary of results.
#[post("/apps/health-check/batch")]
pub async fn batch_health_check(
    key: AuthenticatedKey,
    db: &rocket::State<DbState>,
) -> (Status, Json<Value>) {
    if !key.is_admin {
        return (
            Status::Forbidden,
            Json(
                json!({ "error": "ADMIN_REQUIRED", "message": "Only admins can trigger health checks" }),
            ),
        );
    }

    // Get all approved apps with URLs
    let apps: Vec<(String, String, String)> = {
        let conn = db.0.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, name, COALESCE(api_url, homepage_url) as check_url
                 FROM apps
                 WHERE status = 'approved'
                   AND (api_url IS NOT NULL OR homepage_url IS NOT NULL)",
            )
            .unwrap();

        stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    };

    let total = apps.len();
    let mut healthy = 0;
    let mut unhealthy = 0;
    let mut unreachable = 0;
    let mut results: Vec<Value> = Vec::new();

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .unwrap();

    for (app_id, app_name, check_url) in &apps {
        let start = Instant::now();
        let result = client.get(check_url.as_str()).send().await;
        let response_time_ms = start.elapsed().as_millis() as i64;

        let (health_status, status_code, error_message) = match result {
            Ok(resp) => {
                let code = resp.status().as_u16() as i64;
                if resp.status().is_success() {
                    ("healthy".to_string(), Some(code), None)
                } else {
                    (
                        "unhealthy".to_string(),
                        Some(code),
                        Some(format!("HTTP {}", resp.status())),
                    )
                }
            }
            Err(e) => {
                let msg = if e.is_timeout() {
                    "Connection timed out (10s)".to_string()
                } else if e.is_connect() {
                    "Connection refused or DNS failure".to_string()
                } else {
                    format!("{}", e)
                };
                ("unreachable".to_string(), None, Some(msg))
            }
        };

        match health_status.as_str() {
            "healthy" => healthy += 1,
            "unhealthy" => unhealthy += 1,
            _ => unreachable += 1,
        }

        // Record the health check
        let check_id = uuid::Uuid::new_v4().to_string();
        {
            let conn = db.0.lock().unwrap();

            let _ = conn.execute(
                "INSERT INTO health_checks (id, app_id, status, status_code, response_time_ms, error_message, checked_url)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    check_id,
                    app_id,
                    health_status,
                    status_code,
                    response_time_ms,
                    error_message,
                    check_url,
                ],
            );

            let _ = conn.execute(
                "UPDATE apps SET last_health_status = ?1, last_checked_at = datetime('now'), updated_at = datetime('now') WHERE id = ?2",
                rusqlite::params![health_status, app_id],
            );

            // Recalculate uptime
            let uptime: Option<f64> = conn
                .query_row(
                    "SELECT CAST(SUM(CASE WHEN status = 'healthy' THEN 1 ELSE 0 END) AS REAL) / COUNT(*) * 100.0
                     FROM (SELECT status FROM health_checks WHERE app_id = ?1 ORDER BY checked_at DESC LIMIT 100)",
                    rusqlite::params![app_id],
                    |row| row.get(0),
                )
                .ok();

            if let Some(uptime_val) = uptime {
                let _ = conn.execute(
                    "UPDATE apps SET uptime_pct = ?1 WHERE id = ?2",
                    rusqlite::params![uptime_val, app_id],
                );
            }
        }

        results.push(json!({
            "app_id": app_id,
            "app_name": app_name,
            "status": health_status,
            "status_code": status_code,
            "response_time_ms": response_time_ms,
            "error_message": error_message,
        }));
    }

    (
        Status::Ok,
        Json(json!({
            "total": total,
            "healthy": healthy,
            "unhealthy": unhealthy,
            "unreachable": unreachable,
            "results": results,
        })),
    )
}

/// Get health check history for an app.
#[get("/apps/<app_id>/health?<page>&<per_page>")]
pub fn get_health_history(
    _key: AuthenticatedKey,
    app_id: &str,
    page: Option<i64>,
    per_page: Option<i64>,
    db: &rocket::State<DbState>,
) -> (Status, Json<Value>) {
    let conn = db.0.lock().unwrap();

    // Resolve app ID (support slug lookup)
    let resolved_id: Result<String, _> = conn.query_row(
        "SELECT id FROM apps WHERE id = ?1 OR slug = ?1",
        rusqlite::params![app_id],
        |row| row.get(0),
    );

    let resolved_id = match resolved_id {
        Ok(id) => id,
        Err(_) => {
            return (
                Status::NotFound,
                Json(json!({ "error": "NOT_FOUND", "message": "App not found" })),
            )
        }
    };

    let page = page.unwrap_or(1).max(1);
    let per_page = per_page.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * per_page;

    let total: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM health_checks WHERE app_id = ?1",
            rusqlite::params![resolved_id],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let mut stmt = conn
        .prepare(
            "SELECT id, status, status_code, response_time_ms, error_message, checked_url, checked_at
             FROM health_checks WHERE app_id = ?1 ORDER BY checked_at DESC LIMIT ?2 OFFSET ?3",
        )
        .unwrap();

    let checks: Vec<Value> = stmt
        .query_map(rusqlite::params![resolved_id, per_page, offset], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "status": row.get::<_, String>(1)?,
                "status_code": row.get::<_, Option<i64>>(2)?,
                "response_time_ms": row.get::<_, Option<i64>>(3)?,
                "error_message": row.get::<_, Option<String>>(4)?,
                "checked_url": row.get::<_, String>(5)?,
                "checked_at": row.get::<_, String>(6)?,
            }))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    // Get current uptime
    let uptime: Option<f64> = conn
        .query_row(
            "SELECT uptime_pct FROM apps WHERE id = ?1",
            rusqlite::params![resolved_id],
            |row| row.get(0),
        )
        .ok()
        .flatten();

    (
        Status::Ok,
        Json(json!({
            "app_id": resolved_id,
            "uptime_pct": uptime,
            "checks": checks,
            "total": total,
            "page": page,
            "per_page": per_page,
        })),
    )
}

/// Health summary: overview of all apps' health status.
#[get("/apps/health/summary")]
pub fn health_summary(_key: AuthenticatedKey, db: &rocket::State<DbState>) -> Json<Value> {
    let conn = db.0.lock().unwrap();

    // Get summary counts
    let total_apps: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM apps WHERE status = 'approved'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let monitored: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM apps WHERE status = 'approved' AND last_health_status IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let healthy: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM apps WHERE status = 'approved' AND last_health_status = 'healthy'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let unhealthy: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM apps WHERE status = 'approved' AND last_health_status = 'unhealthy'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let unreachable: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM apps WHERE status = 'approved' AND last_health_status = 'unreachable'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    // Get apps with issues (unhealthy or unreachable)
    let mut stmt = conn
        .prepare(
            "SELECT id, name, slug, last_health_status, last_checked_at, uptime_pct
             FROM apps
             WHERE status = 'approved' AND last_health_status IN ('unhealthy', 'unreachable')
             ORDER BY last_checked_at DESC",
        )
        .unwrap();

    let issues: Vec<Value> = stmt
        .query_map([], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "slug": row.get::<_, String>(2)?,
                "last_health_status": row.get::<_, Option<String>>(3)?,
                "last_checked_at": row.get::<_, Option<String>>(4)?,
                "uptime_pct": row.get::<_, Option<f64>>(5)?,
            }))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    Json(json!({
        "total_approved_apps": total_apps,
        "monitored": monitored,
        "healthy": healthy,
        "unhealthy": unhealthy,
        "unreachable": unreachable,
        "issues": issues,
    }))
}
