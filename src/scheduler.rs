use std::sync::{Arc, Mutex};
use std::time::Duration;

use rocket::fairing::{Fairing, Info, Kind};
use rocket::{Orbit, Rocket};

use crate::events::{AppEvent, EventBus};

/// Shared database connection for the scheduler (separate from main).
pub type SchedulerDb = Arc<Mutex<rusqlite::Connection>>;

/// Default health check interval: 5 minutes.
const DEFAULT_INTERVAL_SECS: u64 = 300;

/// HTTP timeout for health check requests.
const CHECK_TIMEOUT_SECS: u64 = 10;

/// Maximum redirects to follow.
const MAX_REDIRECTS: usize = 5;

/// Open a separate database connection for the scheduler.
pub fn init_scheduler_db() -> SchedulerDb {
    let db_path = std::env::var("DATABASE_PATH").unwrap_or_else(|_| "app_directory.db".to_string());
    let conn = rusqlite::Connection::open(&db_path).expect("Failed to open scheduler DB");
    conn.execute_batch("PRAGMA journal_mode=WAL;")
        .expect("Failed to set WAL mode for scheduler DB");
    Arc::new(Mutex::new(conn))
}

/// Rocket fairing that spawns a background task to periodically
/// check the health of all approved apps.
pub struct ScheduledHealthChecks;

#[rocket::async_trait]
impl Fairing for ScheduledHealthChecks {
    fn info(&self) -> Info {
        Info {
            name: "Scheduled Health Checks",
            kind: Kind::Liftoff,
        }
    }

    async fn on_liftoff(&self, rocket: &Rocket<Orbit>) {
        let interval_secs: u64 = std::env::var("HEALTH_CHECK_INTERVAL_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_INTERVAL_SECS);

        // 0 disables scheduled checks
        if interval_secs == 0 {
            rocket::info!("Scheduled health checks disabled (HEALTH_CHECK_INTERVAL_SECS=0)");
            return;
        }

        // Clone the EventBus (cheap — internally Arc-wrapped)
        let bus = rocket
            .state::<EventBus>()
            .expect("EventBus not managed")
            .clone();

        // Create a separate DB connection for the scheduler
        let scheduler_db = init_scheduler_db();

        // Clone the shutdown handle to stop gracefully
        let shutdown = rocket.shutdown();

        rocket::info!(
            "Scheduled health checks enabled: every {} seconds",
            interval_secs
        );

        tokio::spawn(async move {
            let interval = Duration::from_secs(interval_secs);

            // Wait one full interval before the first check
            tokio::time::sleep(interval).await;

            loop {
                run_scheduled_checks(&scheduler_db, &bus).await;

                // Use tokio::select to handle graceful shutdown
                tokio::select! {
                    _ = tokio::time::sleep(interval) => {},
                    _ = shutdown.clone() => {
                        rocket::info!("Scheduled health checks stopping (server shutdown)");
                        break;
                    }
                }
            }
        });
    }
}

/// Run health checks on all approved apps that have a URL.
async fn run_scheduled_checks(db: &SchedulerDb, bus: &EventBus) {
    // Collect apps to check
    let apps: Vec<(String, String, String)> = {
        let conn = match db.lock() {
            Ok(c) => c,
            Err(_) => {
                rocket::error!("Scheduled health check: failed to acquire DB lock");
                return;
            }
        };

        let mut stmt = match conn.prepare(
            "SELECT id, name, COALESCE(api_url, homepage_url) as check_url
             FROM apps
             WHERE status = 'approved'
               AND (api_url IS NOT NULL OR homepage_url IS NOT NULL)",
        ) {
            Ok(s) => s,
            Err(e) => {
                rocket::error!("Scheduled health check: query error: {}", e);
                return;
            }
        };

        let result: Vec<(String, String, String)> = match stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        }) {
            Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
            Err(e) => {
                rocket::error!("Scheduled health check: row mapping error: {}", e);
                return;
            }
        };
        result
    };

    if apps.is_empty() {
        return;
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(CHECK_TIMEOUT_SECS))
        .redirect(reqwest::redirect::Policy::limited(MAX_REDIRECTS))
        .build()
        .unwrap_or_default();

    let total = apps.len();
    let mut healthy = 0usize;
    let mut unhealthy = 0usize;
    let mut unreachable_count = 0usize;

    for (app_id, app_name, check_url) in &apps {
        let start = std::time::Instant::now();
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
            _ => unreachable_count += 1,
        }

        // Record result in database
        let check_id = uuid::Uuid::new_v4().to_string();
        {
            let conn = match db.lock() {
                Ok(c) => c,
                Err(_) => continue,
            };

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

            // Recalculate uptime from last 100 checks
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

        // Emit event (includes `scheduled: true` to distinguish from manual checks)
        bus.emit(AppEvent {
            event: "health.checked".to_string(),
            data: serde_json::json!({
                "app_id": app_id,
                "app_name": app_name,
                "status": health_status,
                "status_code": status_code,
                "response_time_ms": response_time_ms,
                "scheduled": true,
            }),
        });
    }

    rocket::info!(
        "Scheduled health check complete: {}/{} healthy, {} unhealthy, {} unreachable",
        healthy,
        total,
        unhealthy,
        unreachable_count
    );
}

/// API endpoint to view scheduler configuration and status.
#[get("/health-check/schedule")]
pub fn get_schedule(
    key: crate::auth::AuthenticatedKey,
) -> (
    rocket::http::Status,
    rocket::serde::json::Json<serde_json::Value>,
) {
    if !key.is_admin {
        return (
            rocket::http::Status::Forbidden,
            rocket::serde::json::Json(serde_json::json!({
                "error": "ADMIN_REQUIRED",
                "message": "Only admins can view scheduler status"
            })),
        );
    }

    let interval_secs: u64 = std::env::var("HEALTH_CHECK_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_INTERVAL_SECS);

    let enabled = interval_secs > 0;

    (
        rocket::http::Status::Ok,
        rocket::serde::json::Json(serde_json::json!({
            "enabled": enabled,
            "interval_seconds": interval_secs,
            "description": if enabled {
                format!("Health checks run every {} seconds", interval_secs)
            } else {
                "Scheduled health checks are disabled".to_string()
            },
            "config_var": "HEALTH_CHECK_INTERVAL_SECS",
            "default_interval": DEFAULT_INTERVAL_SECS,
        })),
    )
}
