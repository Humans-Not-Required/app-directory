#[macro_use]
extern crate rocket;

pub mod auth;
pub mod db;
pub mod events;
pub mod health;
pub mod models;
pub mod rate_limit;
pub mod routes;
pub mod scheduler;
pub mod stats;
pub mod webhooks;

use rate_limit::{RateLimitHeaders, RateLimiter};
use rocket::fairing::{Fairing, Info, Kind};
use rocket::fs::{FileServer, Options};
use rocket::http::Header;
use rocket::{Request, Response};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

pub struct Cors;

#[rocket::async_trait]
impl Fairing for Cors {
    fn info(&self) -> Info {
        Info {
            name: "CORS",
            kind: Kind::Response,
        }
    }

    async fn on_response<'r>(&self, request: &'r Request<'_>, response: &mut Response<'r>) {
        response.set_header(Header::new("Access-Control-Allow-Origin", "*"));
        response.set_header(Header::new(
            "Access-Control-Allow-Methods",
            "GET, POST, PUT, PATCH, DELETE, OPTIONS",
        ));
        response.set_header(Header::new(
            "Access-Control-Allow-Headers",
            "Content-Type, Authorization, X-API-Key",
        ));

        if request.method() == rocket::http::Method::Options {
            response.set_status(rocket::http::Status::NoContent);
        }
    }
}

pub struct DbState(pub Mutex<rusqlite::Connection>);

impl DbState {
    /// Get a database connection with mutex poison recovery.
    /// If a previous request panicked while holding the lock, this recovers
    /// gracefully instead of propagating the panic to all subsequent requests.
    pub fn conn(&self) -> std::sync::MutexGuard<'_, rusqlite::Connection> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// SPA catch-all: serves index.html for any unmatched GET (client-side routing)
#[get("/<_..>", rank = 20)]
pub async fn spa_fallback() -> Option<rocket::fs::NamedFile> {
    let static_dir: PathBuf = std::env::var("STATIC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("frontend/dist"));
    rocket::fs::NamedFile::open(static_dir.join("index.html"))
        .await
        .ok()
}

pub fn rocket() -> rocket::Rocket<rocket::Build> {
    dotenvy::dotenv().ok();

    let db_path = std::env::var("DATABASE_PATH").unwrap_or_else(|_| "app_directory.db".to_string());
    rocket_with_path(&db_path)
}

/// Build a Rocket instance with the given database path.
/// Prefer this over `rocket()` in tests to avoid process-global env var races.
pub fn rocket_with_path(db_path: &str) -> rocket::Rocket<rocket::Build> {
    let conn = db::init_db(db_path);

    // Create admin key if none exist
    let key_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM api_keys", [], |r| r.get(0))
        .unwrap();

    if key_count == 0 {
        let admin_key = auth::create_api_key(&conn, "default-admin", true, None);
        println!("=== ADMIN API KEY (save this!) ===");
        println!("{}", admin_key);
        println!("==================================");
    }

    // Allow seeding an admin key from environment variable (for recovery/automation)
    if let Ok(env_key) = std::env::var("ADMIN_API_KEY") {
        if !env_key.is_empty() {
            let key_hash = auth::hash_key(&env_key);
            let exists: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM api_keys WHERE key_hash = ?1",
                    rusqlite::params![key_hash],
                    |r| r.get(0),
                )
                .unwrap_or(false);
            if !exists {
                let id = uuid::Uuid::new_v4().to_string();
                conn.execute(
                    "INSERT INTO api_keys (id, name, key_hash, is_admin, rate_limit) VALUES (?1, ?2, ?3, 1, 10000)",
                    rusqlite::params![id, "env-admin", key_hash],
                )
                .expect("Failed to create env admin key");
                println!("✅ Admin key from ADMIN_API_KEY env var registered");
            }
        }
    }

    let addr = std::env::var("ROCKET_ADDRESS").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port: u16 = std::env::var("ROCKET_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8000);

    // Rate limit window: configurable via RATE_LIMIT_WINDOW_SECS (default: 60s)
    let window_secs: u64 = std::env::var("RATE_LIMIT_WINDOW_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);

    let figment = rocket::Config::figment()
        .merge(("address", addr))
        .merge(("port", port));

    let webhook_db = webhooks::init_webhook_db();
    let event_bus = events::EventBus::with_webhooks(webhook_db);

    // Frontend static files directory
    let static_dir: PathBuf = std::env::var("STATIC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("frontend/dist"));

    let mut rocket = rocket::custom(figment)
        .manage(DbState(Mutex::new(conn)))
        .manage(RateLimiter::new(Duration::from_secs(window_secs)))
        .manage(event_bus)
        .attach(Cors)
        .attach(RateLimitHeaders)
        .attach(scheduler::ScheduledHealthChecks)
        .mount(
            "/api/v1",
            routes![
                routes::health,
                routes::llms_txt,
                routes::openapi,
                routes::submit_app,
                routes::list_apps,
                routes::list_pending_apps,
                routes::get_app,
                routes::list_my_apps,
                routes::update_app,
                routes::delete_app,
                routes::approve_app,
                routes::reject_app,
                routes::deprecate_app,
                routes::undeprecate_app,
                routes::search_apps,
                routes::submit_review,
                routes::get_reviews,
                routes::list_categories,
                routes::list_keys,
                routes::create_key,
                routes::delete_key,
                routes::cors_preflight,
                routes::create_webhook,
                routes::list_webhooks,
                routes::update_webhook,
                routes::delete_webhook,
                routes::event_stream,
                health::health_summary,
                health::batch_health_check,
                health::check_app_health,
                health::get_health_history,
                scheduler::get_schedule,
                stats::get_app_stats,
                stats::trending_apps,
                routes::api_skills_skill_md,
            ],
        );

    // Mount SKILL.md, llms.txt + well-known skills at root level for standard discovery
    rocket = rocket.mount("/", routes![
        routes::skill_md,
        routes::root_llms_txt,
        routes::skills_index,
        routes::skills_skill_md,
    ]);

    // Serve frontend static files if the directory exists
    if static_dir.is_dir() {
        println!("📦 Serving frontend from: {}", static_dir.display());
        rocket = rocket
            .mount("/", FileServer::new(&static_dir, Options::Index))
            .mount("/", routes![spa_fallback]);
    } else {
        println!(
            "⚡ API-only mode (no frontend at {})",
            static_dir.display()
        );
    }

    rocket
}
