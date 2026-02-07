#[macro_use]
extern crate rocket;

pub mod auth;
pub mod db;
pub mod health;
pub mod models;
pub mod rate_limit;
pub mod routes;

use rate_limit::{RateLimitHeaders, RateLimiter};
use rocket::fairing::{Fairing, Info, Kind};
use rocket::http::Header;
use rocket::{Request, Response};
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

pub fn rocket() -> rocket::Rocket<rocket::Build> {
    dotenvy::dotenv().ok();

    let db_path = std::env::var("DATABASE_PATH").unwrap_or_else(|_| "app_directory.db".to_string());
    let conn = db::init_db(&db_path);

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

    let addr = std::env::var("ROCKET_ADDRESS").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port: u16 = std::env::var("ROCKET_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8002);

    // Rate limit window: configurable via RATE_LIMIT_WINDOW_SECS (default: 60s)
    let window_secs: u64 = std::env::var("RATE_LIMIT_WINDOW_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);

    let figment = rocket::Config::figment()
        .merge(("address", addr))
        .merge(("port", port));

    rocket::custom(figment)
        .manage(DbState(Mutex::new(conn)))
        .manage(RateLimiter::new(Duration::from_secs(window_secs)))
        .attach(Cors)
        .attach(RateLimitHeaders)
        .mount(
            "/api/v1",
            routes![
                routes::health,
                routes::openapi,
                routes::submit_app,
                routes::list_apps,
                routes::get_app,
                routes::update_app,
                routes::delete_app,
                routes::search_apps,
                routes::submit_review,
                routes::get_reviews,
                routes::list_categories,
                routes::list_keys,
                routes::create_key,
                routes::delete_key,
                routes::cors_preflight,
                health::health_summary,
                health::batch_health_check,
                health::check_app_health,
                health::get_health_history,
            ],
        )
}
