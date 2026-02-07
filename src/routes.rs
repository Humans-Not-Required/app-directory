use rocket::http::Status;
use rocket::serde::json::Json;
use serde_json::{json, Value};

use crate::auth::{self, AuthenticatedKey};
use crate::models::*;
use crate::DbState;

// === Health ===

#[get("/health")]
pub fn health() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "service": "app-directory",
        "version": "0.1.0"
    }))
}

// === CORS Preflight ===

#[options("/<_path..>")]
pub fn cors_preflight(_path: std::path::PathBuf) -> Status {
    Status::NoContent
}

// === OpenAPI Spec ===

#[get("/openapi.json")]
pub fn openapi() -> (Status, (rocket::http::ContentType, String)) {
    let spec = include_str!("../openapi.json");
    (
        Status::Ok,
        (rocket::http::ContentType::JSON, spec.to_string()),
    )
}

// === App Submission ===

#[post("/apps", data = "<body>")]
pub fn submit_app(
    key: AuthenticatedKey,
    body: Json<SubmitAppRequest>,
    db: &rocket::State<DbState>,
) -> (Status, Json<Value>) {
    let conn = db.0.lock().unwrap();

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

    // Auto-approve for admin keys, otherwise pending
    let status = if key.is_admin { "approved" } else { "pending" };

    let result = conn.execute(
        "INSERT INTO apps (id, name, slug, short_description, description, homepage_url, api_url, api_spec_url, protocol, category, tags, logo_url, author_name, author_url, submitted_by_key_id, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
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
            key.id,
            status,
        ],
    );

    match result {
        Ok(_) => (
            Status::Created,
            Json(json!({
                "id": id,
                "slug": final_slug,
                "status": status,
                "message": if status == "approved" { "App submitted and approved" } else { "App submitted for review" }
            })),
        ),
        Err(e) => (
            Status::InternalServerError,
            Json(json!({ "error": "DB_ERROR", "message": e.to_string() })),
        ),
    }
}

// === List Apps ===

#[get(
    "/apps?<category>&<protocol>&<status>&<featured>&<verified>&<health>&<sort>&<page>&<per_page>"
)]
#[allow(clippy::too_many_arguments)]
pub fn list_apps(
    _key: AuthenticatedKey,
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
    let conn = db.0.lock().unwrap();

    let page = page.unwrap_or(1).max(1);
    let per_page = per_page.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * per_page;

    let mut conditions = vec!["1=1".to_string()];
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    // Default to approved unless status filter is specified
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
            _ => {} // ignore invalid values
        }
    }

    let where_clause = conditions.join(" AND ");

    let order = match sort.as_deref() {
        Some("rating") => "avg_rating DESC, review_count DESC",
        Some("name") => "name ASC",
        Some("oldest") => "created_at ASC",
        _ => "created_at DESC",
    };

    // Count total
    let count_sql = format!("SELECT COUNT(*) FROM apps WHERE {}", where_clause);
    let total: i64 = conn
        .query_row(
            &count_sql,
            rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
            |r| r.get(0),
        )
        .unwrap_or(0);

    // Fetch page
    let query = format!(
        "SELECT id, name, slug, short_description, description, homepage_url, api_url, api_spec_url, protocol, category, tags, logo_url, author_name, author_url, status, is_featured, is_verified, avg_rating, review_count, created_at, updated_at, last_health_status, last_checked_at, uptime_pct
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
            |row| {
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
                }))
            },
        )
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    Json(json!({
        "items": apps,
        "total": total,
        "page": page,
        "per_page": per_page,
    }))
}

// === Get Single App ===

#[get("/apps/<id_or_slug>")]
pub fn get_app(
    _key: AuthenticatedKey,
    id_or_slug: &str,
    db: &rocket::State<DbState>,
) -> (Status, Json<Value>) {
    let conn = db.0.lock().unwrap();

    let result = conn.query_row(
        "SELECT id, name, slug, short_description, description, homepage_url, api_url, api_spec_url, protocol, category, tags, logo_url, author_name, author_url, status, is_featured, is_verified, avg_rating, review_count, created_at, updated_at, last_health_status, last_checked_at, uptime_pct
         FROM apps WHERE id = ?1 OR slug = ?1",
        rusqlite::params![id_or_slug],
        |row| {
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
            }))
        },
    );

    match result {
        Ok(app) => (Status::Ok, Json(app)),
        Err(_) => (
            Status::NotFound,
            Json(json!({ "error": "NOT_FOUND", "message": "App not found" })),
        ),
    }
}

// === Update App ===

#[patch("/apps/<id>", data = "<body>")]
pub fn update_app(
    key: AuthenticatedKey,
    id: &str,
    body: Json<UpdateAppRequest>,
    db: &rocket::State<DbState>,
) -> (Status, Json<Value>) {
    let conn = db.0.lock().unwrap();

    // Check app exists and get owner
    let owner_key_id: Result<String, _> = conn.query_row(
        "SELECT submitted_by_key_id FROM apps WHERE id = ?1",
        rusqlite::params![id],
        |r| r.get(0),
    );

    let owner_key_id = match owner_key_id {
        Ok(k) => k,
        Err(_) => {
            return (
                Status::NotFound,
                Json(json!({ "error": "NOT_FOUND", "message": "App not found" })),
            )
        }
    };

    // Only the submitter or an admin can update
    if owner_key_id != key.id && !key.is_admin {
        return (
            Status::Forbidden,
            Json(
                json!({ "error": "FORBIDDEN", "message": "Only the submitter or an admin can update this app" }),
            ),
        );
    }

    // Only admins can change status or badges
    if body.status.is_some() && !key.is_admin {
        return (
            Status::Forbidden,
            Json(json!({ "error": "FORBIDDEN", "message": "Only admins can change app status" })),
        );
    }
    if (body.is_featured.is_some() || body.is_verified.is_some()) && !key.is_admin {
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

    // Build dynamic update
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
        Ok(_) => (Status::Ok, Json(json!({ "message": "App updated" }))),
        Err(e) => (
            Status::InternalServerError,
            Json(json!({ "error": "DB_ERROR", "message": e.to_string() })),
        ),
    }
}

// === Delete App ===

#[delete("/apps/<id>")]
pub fn delete_app(
    key: AuthenticatedKey,
    id: &str,
    db: &rocket::State<DbState>,
) -> (Status, Json<Value>) {
    let conn = db.0.lock().unwrap();

    let owner_key_id: Result<String, _> = conn.query_row(
        "SELECT submitted_by_key_id FROM apps WHERE id = ?1",
        rusqlite::params![id],
        |r| r.get(0),
    );

    let owner_key_id = match owner_key_id {
        Ok(k) => k,
        Err(_) => {
            return (
                Status::NotFound,
                Json(json!({ "error": "NOT_FOUND", "message": "App not found" })),
            )
        }
    };

    if owner_key_id != key.id && !key.is_admin {
        return (
            Status::Forbidden,
            Json(
                json!({ "error": "FORBIDDEN", "message": "Only the submitter or an admin can delete this app" }),
            ),
        );
    }

    // Delete reviews first, then app
    conn.execute(
        "DELETE FROM reviews WHERE app_id = ?1",
        rusqlite::params![id],
    )
    .ok();

    match conn.execute("DELETE FROM apps WHERE id = ?1", rusqlite::params![id]) {
        Ok(1) => (Status::Ok, Json(json!({ "message": "App deleted" }))),
        Ok(_) => (
            Status::NotFound,
            Json(json!({ "error": "NOT_FOUND", "message": "App not found" })),
        ),
        Err(e) => (
            Status::InternalServerError,
            Json(json!({ "error": "DB_ERROR", "message": e.to_string() })),
        ),
    }
}

// === Search ===

#[get("/apps/search?<q>&<category>&<protocol>&<page>&<per_page>")]
pub fn search_apps(
    _key: AuthenticatedKey,
    q: &str,
    category: Option<String>,
    protocol: Option<String>,
    page: Option<i64>,
    per_page: Option<i64>,
    db: &rocket::State<DbState>,
) -> Json<Value> {
    let conn = db.0.lock().unwrap();

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
        "items": apps,
        "total": total,
        "page": page,
        "per_page": per_page,
    }))
}

// === Reviews ===

#[post("/apps/<app_id>/reviews", data = "<body>")]
pub fn submit_review(
    key: AuthenticatedKey,
    app_id: &str,
    body: Json<SubmitReviewRequest>,
    db: &rocket::State<DbState>,
) -> (Status, Json<Value>) {
    let conn = db.0.lock().unwrap();

    // Validate rating
    if body.rating < 1 || body.rating > 5 {
        return (
            Status::BadRequest,
            Json(json!({ "error": "INVALID_RATING", "message": "Rating must be 1-5" })),
        );
    }

    // Check app exists
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

    // Upsert review (one review per key per app)
    let result = conn.execute(
        "INSERT INTO reviews (id, app_id, reviewer_key_id, rating, title, body)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(app_id, reviewer_key_id) DO UPDATE SET
           rating = excluded.rating,
           title = excluded.title,
           body = excluded.body,
           created_at = datetime('now')",
        rusqlite::params![id, app_id, key.id, body.rating, body.title, body.body],
    );

    if let Err(e) = result {
        return (
            Status::InternalServerError,
            Json(json!({ "error": "DB_ERROR", "message": e.to_string() })),
        );
    }

    // Recalculate aggregate rating
    let _ = conn.execute(
        "UPDATE apps SET
           avg_rating = (SELECT COALESCE(AVG(CAST(rating AS REAL)), 0.0) FROM reviews WHERE app_id = ?1),
           review_count = (SELECT COUNT(*) FROM reviews WHERE app_id = ?1),
           updated_at = datetime('now')
         WHERE id = ?1",
        rusqlite::params![app_id],
    );

    (
        Status::Created,
        Json(json!({ "message": "Review submitted", "id": id })),
    )
}

#[get("/apps/<app_id>/reviews?<page>&<per_page>")]
pub fn get_reviews(
    _key: AuthenticatedKey,
    app_id: &str,
    page: Option<i64>,
    per_page: Option<i64>,
    db: &rocket::State<DbState>,
) -> Json<Value> {
    let conn = db.0.lock().unwrap();

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

    let mut stmt = conn
        .prepare(
            "SELECT id, app_id, rating, title, body, created_at
             FROM reviews WHERE app_id = ?1 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
        )
        .unwrap();

    let reviews: Vec<Value> = stmt
        .query_map(rusqlite::params![app_id, per_page, offset], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "app_id": row.get::<_, String>(1)?,
                "rating": row.get::<_, i64>(2)?,
                "title": row.get::<_, Option<String>>(3)?,
                "body": row.get::<_, Option<String>>(4)?,
                "created_at": row.get::<_, String>(5)?,
            }))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    Json(json!({
        "items": reviews,
        "total": total,
        "page": page,
        "per_page": per_page,
    }))
}

// === Categories ===

#[get("/categories")]
pub fn list_categories(_key: AuthenticatedKey, db: &rocket::State<DbState>) -> Json<Value> {
    let conn = db.0.lock().unwrap();

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

// === Admin: API Keys ===

#[get("/keys")]
pub fn list_keys(key: AuthenticatedKey, db: &rocket::State<DbState>) -> (Status, Json<Value>) {
    if !key.is_admin {
        return (
            Status::Forbidden,
            Json(json!({ "error": "ADMIN_REQUIRED" })),
        );
    }

    let conn = db.0.lock().unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT id, name, is_admin, rate_limit, created_at FROM api_keys WHERE revoked = 0",
        )
        .unwrap();

    let keys: Vec<Value> = stmt
        .query_map([], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "is_admin": row.get::<_, i32>(2)? != 0,
                "rate_limit": row.get::<_, i64>(3)?,
                "created_at": row.get::<_, String>(4)?,
            }))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    (Status::Ok, Json(json!({ "keys": keys })))
}

#[post("/keys", data = "<body>")]
pub fn create_key(
    key: AuthenticatedKey,
    body: Json<models::CreateKeyRequest>,
    db: &rocket::State<DbState>,
) -> (Status, Json<Value>) {
    if !key.is_admin {
        return (
            Status::Forbidden,
            Json(json!({ "error": "ADMIN_REQUIRED" })),
        );
    }

    let conn = db.0.lock().unwrap();
    let raw_key = auth::create_api_key(
        &conn,
        &body.name,
        body.is_admin.unwrap_or(false),
        body.rate_limit,
    );

    (
        Status::Created,
        Json(json!({
            "key": raw_key,
            "message": "Save this key — it won't be shown again"
        })),
    )
}

#[delete("/keys/<id>")]
pub fn delete_key(
    key: AuthenticatedKey,
    id: &str,
    db: &rocket::State<DbState>,
) -> (Status, Json<Value>) {
    if !key.is_admin {
        return (
            Status::Forbidden,
            Json(json!({ "error": "ADMIN_REQUIRED" })),
        );
    }

    let conn = db.0.lock().unwrap();
    match conn.execute(
        "UPDATE api_keys SET revoked = 1 WHERE id = ?1",
        rusqlite::params![id],
    ) {
        Ok(1) => (Status::Ok, Json(json!({ "message": "Key revoked" }))),
        Ok(_) => (Status::NotFound, Json(json!({ "error": "NOT_FOUND" }))),
        Err(e) => (
            Status::InternalServerError,
            Json(json!({ "error": "DB_ERROR", "message": e.to_string() })),
        ),
    }
}

// Use the models module
use crate::models;
