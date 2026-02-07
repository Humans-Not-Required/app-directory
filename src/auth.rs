use rocket::http::Status;
use rocket::request::{FromRequest, Outcome, Request};
use rocket::State;
use rusqlite::Connection;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::rate_limit::RateLimiter;
use crate::DbState;

/// Simple hash for API keys and edit tokens (not cryptographic — fine for this use case)
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

/// Authenticated caller info extracted from request (OPTIONAL for most routes now)
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

        // Scope the DB lock so it's dropped before any .await
        let result = {
            let conn = db.0.lock().expect("DB lock poisoned");
            conn.query_row(
                "SELECT id, name, is_admin, rate_limit FROM api_keys WHERE key_hash = ?1 AND revoked = 0",
                rusqlite::params![key_hash],
                |row| {
                    Ok((
                        AuthenticatedKey {
                            id: row.get(0)?,
                            name: row.get(1)?,
                            is_admin: row.get::<_, i32>(2)? != 0,
                        },
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
        };

        match result {
            Ok((auth_key, rate_limit)) => {
                // Get the rate limiter from Rocket state
                let limiter = match request.guard::<&State<RateLimiter>>().await {
                    Outcome::Success(l) => l,
                    _ => {
                        return Outcome::Error((
                            Status::InternalServerError,
                            "Rate limiter unavailable",
                        ))
                    }
                };

                // Enforce rate limit (per-key, fixed window)
                let rl_result = limiter.check(&auth_key.id, rate_limit as u64);

                // Store rate limit info in request-local state for response headers
                let _ = request.local_cache(|| Some(rl_result.clone()));

                if !rl_result.allowed {
                    return Outcome::Error((
                        Status::TooManyRequests,
                        "Rate limit exceeded. Try again later.",
                    ));
                }

                Outcome::Success(auth_key)
            }
            Err(_) => Outcome::Error((Status::Unauthorized, "Invalid API key")),
        }
    }
}

/// Optional authenticated key (allows both authenticated and anonymous requests)
#[derive(Debug)]
pub struct OptionalKey(pub Option<AuthenticatedKey>);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for OptionalKey {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match request.guard::<AuthenticatedKey>().await {
            Outcome::Success(key) => Outcome::Success(OptionalKey(Some(key))),
            _ => Outcome::Success(OptionalKey(None)),
        }
    }
}

/// Edit token authorization (for editing/deleting specific apps)
#[derive(Debug)]
pub struct EditAuth {
    pub app_id: String,
    pub via: EditAuthVia,
}

#[derive(Debug)]
pub enum EditAuthVia {
    EditToken,
    ApiKey(String),  // key_id
    Admin(String),   // key_id
}

impl EditAuth {
    /// Check if the request can edit the given app_id via:
    /// 1. Edit token in query param or header
    /// 2. API key that created the app
    /// 3. Admin API key
    pub async fn from_request_for_app(
        request: &rocket::Request<'_>,
        app_id: &str,
    ) -> Result<Self, Status> {
        let db = request
            .rocket()
            .state::<DbState>()
            .ok_or(Status::InternalServerError)?;

        // Try edit token first (query param or header)
        let edit_token = request
            .query_value::<String>("token")
            .and_then(|r| r.ok())
            .or_else(|| {
                request
                    .headers()
                    .get_one("X-Edit-Token")
                    .map(|s| s.to_string())
            });

        if let Some(token) = edit_token {
            let token_hash = hash_key(&token);
            let conn = db.0.lock().map_err(|_| Status::InternalServerError)?;
            
            // Check if token matches this app
            let valid: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM apps WHERE id = ?1 AND edit_token_hash = ?2",
                    rusqlite::params![app_id, token_hash],
                    |r| r.get(0),
                )
                .unwrap_or(false);

            if valid {
                return Ok(EditAuth {
                    app_id: app_id.to_string(),
                    via: EditAuthVia::EditToken,
                });
            }
        }

        // Try API key
        if let Outcome::Success(api_key) = request.guard::<AuthenticatedKey>().await {
            let conn = db.0.lock().map_err(|_| Status::InternalServerError)?;

            // Admin can edit anything
            if api_key.is_admin {
                return Ok(EditAuth {
                    app_id: app_id.to_string(),
                    via: EditAuthVia::Admin(api_key.id),
                });
            }

            // Check if this API key created the app
            let is_owner: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM apps WHERE id = ?1 AND submitted_by_key_id = ?2",
                    rusqlite::params![app_id, api_key.id],
                    |r| r.get(0),
                )
                .unwrap_or(false);

            if is_owner {
                return Ok(EditAuth {
                    app_id: app_id.to_string(),
                    via: EditAuthVia::ApiKey(api_key.id),
                });
            }
        }

        Err(Status::Forbidden)
    }
}
