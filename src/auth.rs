use rocket::http::Status;
use rocket::request::{FromRequest, Outcome, Request};
use rusqlite::Connection;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::DbState;

/// Simple hash for API keys (not cryptographic — fine for this use case)
pub fn hash_key(key: &str) -> String {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Create an API key and return the raw key string
pub fn create_api_key(
    conn: &Connection,
    name: &str,
    is_admin: bool,
    rate_limit: Option<i64>,
) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let raw_key = format!("ad_{}", uuid::Uuid::new_v4().to_string().replace('-', ""));
    let key_hash = hash_key(&raw_key);
    let rl = rate_limit.unwrap_or(if is_admin { 10_000 } else { 100 });

    conn.execute(
        "INSERT INTO api_keys (id, name, key_hash, is_admin, rate_limit) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![id, name, key_hash, is_admin as i32, rl],
    )
    .expect("Failed to create API key");

    raw_key
}

/// Authenticated caller info extracted from request
#[derive(Debug)]
pub struct AuthenticatedKey {
    pub id: String,
    #[allow(dead_code)]
    pub name: String,
    pub is_admin: bool,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AuthenticatedKey {
    type Error = &'static str;

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        // Extract key from Authorization: Bearer or X-API-Key header
        let raw_key = request
            .headers()
            .get_one("Authorization")
            .and_then(|h| h.strip_prefix("Bearer "))
            .or_else(|| request.headers().get_one("X-API-Key"));

        let raw_key = match raw_key {
            Some(k) => k.to_string(),
            None => return Outcome::Error((Status::Unauthorized, "Missing API key")),
        };

        let key_hash = hash_key(&raw_key);

        let db = request
            .rocket()
            .state::<DbState>()
            .expect("DB not initialized");
        let conn = db.0.lock().expect("DB lock poisoned");

        let result = conn.query_row(
            "SELECT id, name, is_admin FROM api_keys WHERE key_hash = ?1 AND revoked = 0",
            rusqlite::params![key_hash],
            |row| {
                Ok(AuthenticatedKey {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    is_admin: row.get::<_, i32>(2)? != 0,
                })
            },
        );

        match result {
            Ok(key) => Outcome::Success(key),
            Err(_) => Outcome::Error((Status::Unauthorized, "Invalid API key")),
        }
    }
}
