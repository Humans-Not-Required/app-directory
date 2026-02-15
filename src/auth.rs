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

/// Edit token extracted from ?token= query param or X-Edit-Token header (optional)
#[derive(Debug)]
pub struct EditTokenParam(pub Option<String>);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for EditTokenParam {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let token = request
            .query_value::<String>("token")
            .and_then(|r| r.ok())
            .or_else(|| {
                request
                    .headers()
                    .get_one("X-Edit-Token")
                    .map(|s| s.to_string())
            });
        Outcome::Success(EditTokenParam(token))
    }
}

/// Result of checking edit access for an app
#[derive(Debug)]
pub enum EditAccess {
    /// Authenticated via per-app edit token
    EditToken,
    /// Authenticated via API key (owner of the app)
    Owner(String),
    /// Authenticated via admin API key
    Admin(String),
}

impl EditAccess {
    /// Returns true if the access level allows admin-only operations (status, badges)
    pub fn is_admin(&self) -> bool {
        matches!(self, EditAccess::Admin(_))
    }
}

/// Check if the caller can edit a specific app.
/// Tries: (1) edit token, (2) API key owner, (3) admin key.
/// Returns Ok(EditAccess) or Err((Status, error json)).
pub fn check_edit_access(
    conn: &Connection,
    app_id: &str,
    edit_token: &Option<String>,
    api_key: &Option<AuthenticatedKey>,
) -> Result<EditAccess, (Status, serde_json::Value)> {
    // First, verify app exists
    let app_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM apps WHERE id = ?1",
            rusqlite::params![app_id],
            |r| r.get(0),
        )
        .unwrap_or(false);

    if !app_exists {
        return Err((
            Status::NotFound,
            serde_json::json!({ "error": "NOT_FOUND", "message": "App not found" }),
        ));
    }

    // Try edit token first
    if let Some(token) = edit_token {
        let token_hash = hash_key(token);
        let valid: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM apps WHERE id = ?1 AND edit_token_hash = ?2",
                rusqlite::params![app_id, token_hash],
                |r| r.get(0),
            )
            .unwrap_or(false);

        if valid {
            return Ok(EditAccess::EditToken);
        }
    }

    // Try API key
    if let Some(key) = api_key {
        if key.is_admin {
            return Ok(EditAccess::Admin(key.id.clone()));
        }

        let is_owner: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM apps WHERE id = ?1 AND submitted_by_key_id = ?2",
                rusqlite::params![app_id, key.id],
                |r| r.get(0),
            )
            .unwrap_or(false);

        if is_owner {
            return Ok(EditAccess::Owner(key.id.clone()));
        }
    }

    // No valid auth
    if edit_token.is_some() || api_key.is_some() {
        Err((
            Status::Forbidden,
            serde_json::json!({ "error": "FORBIDDEN", "message": "You don't have permission to edit this app" }),
        ))
    } else {
        Err((
            Status::Unauthorized,
            serde_json::json!({ "error": "UNAUTHORIZED", "message": "Edit token or API key required. Pass edit token via ?token= query param or X-Edit-Token header." }),
        ))
    }
}
