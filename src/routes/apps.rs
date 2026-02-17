use rocket::http::Status;
use rocket::serde::json::Json;
use serde_json::{json, Value};

use crate::auth::{self, AuthenticatedKey, EditTokenParam, OptionalKey, check_edit_access};
use crate::events::{AppEvent, EventBus};
use crate::models::*;
use crate::DbState;

// === App Submission (NO AUTH REQUIRED) ===

#[post("/apps", data = "<body>")]
pub fn submit_app(
    opt_key: OptionalKey,
    body: Json<SubmitAppRequest>,
    db: &rocket::State<DbState>,
    bus: &rocket::State<EventBus>,
) -> (Status, Json<Value>) {
    let conn = db.conn();

    let protocol = body.protocol.as_deref().unwrap_or("rest");
    if !VALID_PROTOCOLS.contains(&protocol) {
        return (
            Status::BadRequest,
            Json(json!({
                "error": "INVALID_PROTOCOL",
                "message": format!("Valid protocols: {}", VALID_PROTOCOLS.join(", "))
            })),
        );
    }

    let category = body.category.as_deref().unwrap_or("other");
    if !VALID_CATEGORIES.contains(&category) {
        return (
            Status::BadRequest,
            Json(json!({
                "error": "INVALID_CATEGORY",
                "message": format!("Valid categories: {}", VALID_CATEGORIES.join(", "))
            })),
        );
    }

    let id = uuid::Uuid::new_v4().to_string();
    let slug = slugify(&body.name);
    let tags_json = serde_json::to_string(&body.tags.clone().unwrap_or_default()).unwrap();

    // Check slug uniqueness
    let slug_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM apps WHERE slug = ?1",
            rusqlite::params![slug],
            |r| r.get::<_, i64>(0),
        )
        .unwrap()
        > 0;

    let final_slug = if slug_exists {
        format!("{}-{}", slug, &id[..8])
    } else {
        slug
    };

    // Generate edit token for this specific app
    let edit_token = format!("ad_{}", uuid::Uuid::new_v4().to_string().replace('-', ""));
    let edit_token_hash = auth::hash_key(&edit_token);

    // Determine status and key association
    let (status, submitted_by_key_id) = match &opt_key.0 {
        Some(key) if key.is_admin => ("approved", Some(key.id.clone())),
        Some(key) => ("approved", Some(key.id.clone())),
        None => ("approved", None),
    };

    let result = conn.execute(
        "INSERT INTO apps (id, name, slug, short_description, description, homepage_url, api_url, api_spec_url, protocol, category, tags, logo_url, author_name, author_url, submitted_by_key_id, status, edit_token_hash)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        rusqlite::params![
            id,
            body.name,
            final_slug,
            body.short_description,
            body.description,
            body.homepage_url,
            body.api_url,
            body.api_spec_url,
            protocol,
            category,
            tags_json,
            body.logo_url,
            body.author_name,
            body.author_url,
            submitted_by_key_id,
            status,
            edit_token_hash,
        ],
    );

    match result {
        Ok(_) => {
            bus.emit(AppEvent {
                event: "app.approved".to_string(),
                data: json!({
                    "app_id": id,
                    "name": body.name,
                    "slug": final_slug,
                    "status": status,
                }),
            });

            let edit_url = format!("/apps/{}/edit?token={}", id, edit_token);
            let listing_url = format!("/apps/{}", id);

            (
                Status::Created,
                Json(json!({
                    "app_id": id,
                    "slug": final_slug,
                    "status": status,
                    "edit_token": edit_token,
                    "edit_url": edit_url,
                    "listing_url": listing_url,
                    "message": "App listing created! Save your edit token to modify or delete this listing later."
                })),
            )
        }
        Err(_) => (
            Status::InternalServerError,
            Json(json!({ "error": "DB_ERROR", "message": "Internal server error" })),
        ),
    }
}

// === List Apps (NO AUTH REQUIRED) ===

#[get(
    "/apps?<category>&<protocol>&<status>&<featured>&<verified>&<health>&<sort>&<page>&<per_page>"
)]
#[allow(clippy::too_many_arguments)]
pub fn list_apps(
    category: Option<String>,
    protocol: Option<String>,
    status: Option<String>,
    featured: Option<bool>,
    verified: Option<bool>,
    health: Option<String>,
    sort: Option<String>,
    page: Option<i64>,
    per_page: Option<i64>,
    db: &rocket::State<DbState>,
) -> Json<Value> {
    let conn = db.conn();

    let page = page.unwrap_or(1).max(1);
    let per_page = per_page.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * per_page;

    let mut conditions = vec!["1=1".to_string()];
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    let status_filter = status.unwrap_or_else(|| "approved".to_string());
    if status_filter != "all" {
        conditions.push(format!("status = ?{}", params.len() + 1));
        params.push(Box::new(status_filter));
    }

    if let Some(ref cat) = category {
        conditions.push(format!("category = ?{}", params.len() + 1));
        params.push(Box::new(cat.clone()));
    }

    if let Some(ref proto) = protocol {
        conditions.push(format!("protocol = ?{}", params.len() + 1));
        params.push(Box::new(proto.clone()));
    }

    if let Some(true) = featured {
        conditions.push("is_featured = 1".to_string());
    }

    if let Some(true) = verified {
        conditions.push("is_verified = 1".to_string());
    }

    if let Some(ref h) = health {
        match h.as_str() {
            "healthy" | "unhealthy" | "unreachable" => {
                conditions.push(format!("last_health_status = ?{}", params.len() + 1));
                params.push(Box::new(h.clone()));
            }
            "unknown" => {
                conditions.push("last_health_status IS NULL".to_string());
            }
            _ => {}
        }
    }

    let where_clause = conditions.join(" AND ");

    let order = match sort.as_deref() {
        Some("rating") => "avg_rating DESC, review_count DESC",
        Some("name") => "name ASC",
        Some("oldest") => "created_at ASC",
        _ => "created_at DESC",
    };

    let count_sql = format!("SELECT COUNT(*) FROM apps WHERE {}", where_clause);
    let total: i64 = conn
        .query_row(
            &count_sql,
            rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
            |r| r.get(0),
        )
        .unwrap_or(0);

    let query = format!(
        "SELECT id, name, slug, short_description, description, homepage_url, api_url, api_spec_url, protocol, category, tags, logo_url, author_name, author_url, status, is_featured, is_verified, avg_rating, review_count, created_at, updated_at, last_health_status, last_checked_at, uptime_pct, review_note, reviewed_by, reviewed_at, deprecated_reason, deprecated_by, deprecated_at, replacement_app_id, sunset_at
         FROM apps WHERE {} ORDER BY {} LIMIT ?{} OFFSET ?{}",
        where_clause,
        order,
        params.len() + 1,
        params.len() + 2,
    );

    params.push(Box::new(per_page));
    params.push(Box::new(offset));

    let mut stmt = conn.prepare(&query).unwrap();
    let apps: Vec<Value> = stmt
        .query_map(
            rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
            app_row_to_json,
        )
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    Json(json!({
        "apps": apps,
        "total": total,
        "page": page,
        "per_page": per_page,
    }))
}

// === Get Single App (NO AUTH REQUIRED) ===

#[get("/apps/<id_or_slug>")]
pub fn get_app(
    opt_key: OptionalKey,
    id_or_slug: &str,
    db: &rocket::State<DbState>,
) -> (Status, Json<Value>) {
    let conn = db.conn();

    let result = conn.query_row(
        "SELECT id, name, slug, short_description, description, homepage_url, api_url, api_spec_url, protocol, category, tags, logo_url, author_name, author_url, status, is_featured, is_verified, avg_rating, review_count, created_at, updated_at, last_health_status, last_checked_at, uptime_pct, review_note, reviewed_by, reviewed_at, deprecated_reason, deprecated_by, deprecated_at, replacement_app_id, sunset_at
         FROM apps WHERE id = ?1 OR slug = ?1",
        rusqlite::params![id_or_slug],
        app_row_to_json,
    );

    match result {
        Ok(app) => {
            if let Some(app_id) = app.get("id").and_then(|v| v.as_str()) {
                let viewer_id = opt_key.0.as_ref().map(|k| k.id.as_str()).unwrap_or("anonymous");
                crate::stats::record_view(&conn, app_id, viewer_id);
            }
            (Status::Ok, Json(app))
        }
        Err(_) => (
            Status::NotFound,
            Json(json!({ "error": "NOT_FOUND", "message": "App not found" })),
        ),
    }
}

// === List My Apps (API Key Required) ===

#[get("/apps/mine")]
pub fn list_my_apps(
    key: AuthenticatedKey,
    db: &rocket::State<DbState>,
) -> (Status, Json<Value>) {
    let conn = db.conn();

    let mut stmt = conn
        .prepare(
            "SELECT id, name, slug, short_description, status, created_at, updated_at
             FROM apps WHERE submitted_by_key_id = ?1 ORDER BY created_at DESC",
        )
        .unwrap();

    let apps: Vec<Value> = stmt
        .query_map(rusqlite::params![key.id], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "slug": row.get::<_, String>(2)?,
                "short_description": row.get::<_, String>(3)?,
                "status": row.get::<_, String>(4)?,
                "created_at": row.get::<_, String>(5)?,
                "updated_at": row.get::<_, String>(6)?,
            }))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    (
        Status::Ok,
        Json(json!({
            "apps": apps,
            "total": apps.len(),
        })),
    )
}

// === Update App ===

#[patch("/apps/<id>", data = "<body>")]
pub fn update_app(
    opt_key: OptionalKey,
    edit_token: EditTokenParam,
    id: &str,
    body: Json<UpdateAppRequest>,
    db: &rocket::State<DbState>,
    bus: &rocket::State<EventBus>,
) -> (Status, Json<Value>) {
    let conn = db.conn();

    // Check edit access via edit token, API key owner, or admin
    let access = match check_edit_access(&conn, id, &edit_token.0, &opt_key.0) {
        Ok(a) => a,
        Err((status, err)) => return (status, Json(err)),
    };

    // Admin-only fields: status, featured, verified badges
    if body.status.is_some() && !access.is_admin() {
        return (
            Status::Forbidden,
            Json(json!({ "error": "FORBIDDEN", "message": "Only admins can change app status" })),
        );
    }
    if (body.is_featured.is_some() || body.is_verified.is_some()) && !access.is_admin() {
        return (
            Status::Forbidden,
            Json(
                json!({ "error": "FORBIDDEN", "message": "Only admins can set featured/verified badges" }),
            ),
        );
    }

    if let Some(ref status) = body.status {
        if !VALID_STATUSES.contains(&status.as_str()) {
            return (
                Status::BadRequest,
                Json(json!({
                    "error": "INVALID_STATUS",
                    "message": format!("Valid statuses: {}", VALID_STATUSES.join(", "))
                })),
            );
        }
    }

    if let Some(ref protocol) = body.protocol {
        if !VALID_PROTOCOLS.contains(&protocol.as_str()) {
            return (
                Status::BadRequest,
                Json(json!({
                    "error": "INVALID_PROTOCOL",
                    "message": format!("Valid protocols: {}", VALID_PROTOCOLS.join(", "))
                })),
            );
        }
    }

    if let Some(ref category) = body.category {
        if !VALID_CATEGORIES.contains(&category.as_str()) {
            return (
                Status::BadRequest,
                Json(json!({
                    "error": "INVALID_CATEGORY",
                    "message": format!("Valid categories: {}", VALID_CATEGORIES.join(", "))
                })),
            );
        }
    }

    let mut sets: Vec<String> = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    macro_rules! maybe_set {
        ($field:ident, $col:expr) => {
            if let Some(ref val) = body.$field {
                params.push(Box::new(val.clone()));
                sets.push(format!("{} = ?{}", $col, params.len()));
            }
        };
    }

    maybe_set!(name, "name");
    maybe_set!(short_description, "short_description");
    maybe_set!(description, "description");
    maybe_set!(homepage_url, "homepage_url");
    maybe_set!(api_url, "api_url");
    maybe_set!(api_spec_url, "api_spec_url");
    maybe_set!(protocol, "protocol");
    maybe_set!(category, "category");
    maybe_set!(logo_url, "logo_url");
    maybe_set!(author_name, "author_name");
    maybe_set!(author_url, "author_url");
    maybe_set!(status, "status");

    if let Some(ref tags) = body.tags {
        let tags_json = serde_json::to_string(tags).unwrap();
        params.push(Box::new(tags_json));
        sets.push(format!("tags = ?{}", params.len()));
    }

    if let Some(featured) = body.is_featured {
        params.push(Box::new(featured as i32));
        sets.push(format!("is_featured = ?{}", params.len()));
    }

    if let Some(verified) = body.is_verified {
        params.push(Box::new(verified as i32));
        sets.push(format!("is_verified = ?{}", params.len()));
    }

    if sets.is_empty() {
        return (
            Status::BadRequest,
            Json(json!({ "error": "NO_CHANGES", "message": "No fields to update" })),
        );
    }

    sets.push("updated_at = datetime('now')".to_string());

    params.push(Box::new(id.to_string()));
    let sql = format!(
        "UPDATE apps SET {} WHERE id = ?{}",
        sets.join(", "),
        params.len()
    );

    match conn.execute(
        &sql,
        rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
    ) {
        Ok(_) => {
            let event_name = if body.status.as_deref() == Some("approved") {
                "app.approved"
            } else {
                "app.updated"
            };
            bus.emit(AppEvent {
                event: event_name.to_string(),
                data: json!({ "app_id": id }),
            });

            (Status::Ok, Json(json!({ "message": "App updated" })))
        }
        Err(_) => (
            Status::InternalServerError,
            Json(json!({ "error": "DB_ERROR", "message": "Internal server error" })),
        ),
    }
}

// === Delete App ===

#[delete("/apps/<id>")]
pub fn delete_app(
    opt_key: OptionalKey,
    edit_token: EditTokenParam,
    id: &str,
    db: &rocket::State<DbState>,
    bus: &rocket::State<EventBus>,
) -> (Status, Json<Value>) {
    let conn = db.conn();

    // Check edit access via edit token, API key owner, or admin
    match check_edit_access(&conn, id, &edit_token.0, &opt_key.0) {
        Ok(_) => {}
        Err((status, err)) => return (status, Json(err)),
    }

    // Clean up all dependent records before deleting the app
    conn.execute("DELETE FROM reviews WHERE app_id = ?1", rusqlite::params![id]).ok();
    conn.execute("DELETE FROM app_views WHERE app_id = ?1", rusqlite::params![id]).ok();
    conn.execute("DELETE FROM health_checks WHERE app_id = ?1", rusqlite::params![id]).ok();

    match conn.execute("DELETE FROM apps WHERE id = ?1", rusqlite::params![id]) {
        Ok(1) => {
            bus.emit(AppEvent {
                event: "app.deleted".to_string(),
                data: json!({ "app_id": id }),
            });
            (Status::Ok, Json(json!({ "message": "App deleted" })))
        }
        Ok(_) => (
            Status::NotFound,
            Json(json!({ "error": "NOT_FOUND", "message": "App not found" })),
        ),
        Err(e) => {
            eprintln!("❌ Delete app {id} failed: {e}");
            (
                Status::InternalServerError,
                Json(json!({ "error": "DB_ERROR", "message": "Internal server error" })),
            )
        }
    }
}

// === Search (NO AUTH REQUIRED) ===

#[get("/apps/search?<q>&<category>&<protocol>&<page>&<per_page>")]
pub fn search_apps(
    q: &str,
    category: Option<String>,
    protocol: Option<String>,
    page: Option<i64>,
    per_page: Option<i64>,
    db: &rocket::State<DbState>,
) -> Json<Value> {
    let conn = db.conn();

    let page = page.unwrap_or(1).max(1);
    let per_page = per_page.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * per_page;

    let search_pattern = format!("%{}%", q.to_lowercase());

    let mut conditions = vec![
        "status = 'approved'".to_string(),
        "(LOWER(name) LIKE ?1 OR LOWER(short_description) LIKE ?1 OR LOWER(description) LIKE ?1 OR LOWER(tags) LIKE ?1)".to_string(),
    ];
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(search_pattern)];

    if let Some(ref cat) = category {
        params.push(Box::new(cat.clone()));
        conditions.push(format!("category = ?{}", params.len()));
    }

    if let Some(ref proto) = protocol {
        params.push(Box::new(proto.clone()));
        conditions.push(format!("protocol = ?{}", params.len()));
    }

    let where_clause = conditions.join(" AND ");

    let count_sql = format!("SELECT COUNT(*) FROM apps WHERE {}", where_clause);
    let total: i64 = conn
        .query_row(
            &count_sql,
            rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
            |r| r.get(0),
        )
        .unwrap_or(0);

    let query = format!(
        "SELECT id, name, slug, short_description, protocol, category, tags, is_featured, is_verified, avg_rating, review_count
         FROM apps WHERE {} ORDER BY avg_rating DESC, review_count DESC LIMIT ?{} OFFSET ?{}",
        where_clause,
        params.len() + 1,
        params.len() + 2,
    );
    params.push(Box::new(per_page));
    params.push(Box::new(offset));

    let mut stmt = conn.prepare(&query).unwrap();
    let apps: Vec<Value> = stmt
        .query_map(
            rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
            |row| {
                let tags_str: String = row.get(6)?;
                let tags: Vec<String> = serde_json::from_str(&tags_str).unwrap_or_default();
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
                }))
            },
        )
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    Json(json!({
        "apps": apps,
        "total": total,
        "page": page,
        "per_page": per_page,
    }))
}

/// List pending apps. Admin only. Convenience endpoint.
#[get("/apps/pending?<page>&<per_page>")]
pub fn list_pending_apps(
    key: AuthenticatedKey,
    page: Option<i64>,
    per_page: Option<i64>,
    db: &rocket::State<DbState>,
) -> (Status, Json<Value>) {
    if !key.is_admin {
        return (
            Status::Forbidden,
            Json(
                json!({ "error": "ADMIN_REQUIRED", "message": "Only admins can view pending apps" }),
            ),
        );
    }

    let conn = db.conn();

    let page = page.unwrap_or(1).max(1);
    let per_page = per_page.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * per_page;

    let total: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM apps WHERE status = 'pending'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let mut stmt = conn
        .prepare(
            "SELECT id, name, slug, short_description, protocol, category, tags, author_name, created_at, submitted_by_key_id
             FROM apps WHERE status = 'pending' ORDER BY created_at ASC LIMIT ?1 OFFSET ?2",
        )
        .unwrap();

    let apps: Vec<Value> = stmt
        .query_map(rusqlite::params![per_page, offset], |row| {
            let tags_str: String = row.get(6)?;
            let tags: Vec<String> = serde_json::from_str(&tags_str).unwrap_or_default();
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "slug": row.get::<_, String>(2)?,
                "short_description": row.get::<_, String>(3)?,
                "protocol": row.get::<_, String>(4)?,
                "category": row.get::<_, String>(5)?,
                "tags": tags,
                "author_name": row.get::<_, String>(7)?,
                "created_at": row.get::<_, String>(8)?,
                "submitted_by_key_id": row.get::<_, String>(9)?,
            }))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    (
        Status::Ok,
        Json(json!({
            "apps": apps,
            "total": total,
            "page": page,
            "per_page": per_page,
        })),
    )
}

/// Helper to map a full app row to JSON.
fn app_row_to_json(row: &rusqlite::Row) -> Result<Value, rusqlite::Error> {
    let tags_str: String = row.get(10)?;
    let tags: Vec<String> = serde_json::from_str(&tags_str).unwrap_or_default();
    Ok(json!({
        "id": row.get::<_, String>(0)?,
        "name": row.get::<_, String>(1)?,
        "slug": row.get::<_, String>(2)?,
        "short_description": row.get::<_, String>(3)?,
        "description": row.get::<_, String>(4)?,
        "homepage_url": row.get::<_, Option<String>>(5)?,
        "api_url": row.get::<_, Option<String>>(6)?,
        "api_spec_url": row.get::<_, Option<String>>(7)?,
        "protocol": row.get::<_, String>(8)?,
        "category": row.get::<_, String>(9)?,
        "tags": tags,
        "logo_url": row.get::<_, Option<String>>(11)?,
        "author_name": row.get::<_, String>(12)?,
        "author_url": row.get::<_, Option<String>>(13)?,
        "status": row.get::<_, String>(14)?,
        "is_featured": row.get::<_, i32>(15)? != 0,
        "is_verified": row.get::<_, i32>(16)? != 0,
        "avg_rating": row.get::<_, f64>(17)?,
        "review_count": row.get::<_, i64>(18)?,
        "created_at": row.get::<_, String>(19)?,
        "updated_at": row.get::<_, String>(20)?,
        "last_health_status": row.get::<_, Option<String>>(21)?,
        "last_checked_at": row.get::<_, Option<String>>(22)?,
        "uptime_pct": row.get::<_, Option<f64>>(23)?,
        "review_note": row.get::<_, Option<String>>(24)?,
        "reviewed_by": row.get::<_, Option<String>>(25)?,
        "reviewed_at": row.get::<_, Option<String>>(26)?,
        "deprecated_reason": row.get::<_, Option<String>>(27)?,
        "deprecated_by": row.get::<_, Option<String>>(28)?,
        "deprecated_at": row.get::<_, Option<String>>(29)?,
        "replacement_app_id": row.get::<_, Option<String>>(30)?,
        "sunset_at": row.get::<_, Option<String>>(31)?,
    }))
}
