use serde::{Deserialize, Serialize};

// === API Key Models ===

#[derive(Debug, Serialize)]
#[allow(dead_code)]
pub struct ApiKeyInfo {
    pub id: String,
    pub name: String,
    pub is_admin: bool,
    pub rate_limit: i64,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateKeyRequest {
    pub name: String,
    pub is_admin: Option<bool>,
    pub rate_limit: Option<i64>,
}

// === App Models ===

#[derive(Debug, Serialize)]
#[allow(dead_code)]
pub struct App {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub short_description: String,
    pub description: String,
    pub homepage_url: Option<String>,
    pub api_url: Option<String>,
    pub api_spec_url: Option<String>,
    pub protocol: String,
    pub category: String,
    pub tags: Vec<String>,
    pub logo_url: Option<String>,
    pub author_name: String,
    pub author_url: Option<String>,
    pub status: String,
    pub avg_rating: f64,
    pub review_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct SubmitAppRequest {
    pub name: String,
    pub short_description: String,
    pub description: String,
    pub homepage_url: Option<String>,
    pub api_url: Option<String>,
    pub api_spec_url: Option<String>,
    pub protocol: Option<String>,
    pub category: Option<String>,
    pub tags: Option<Vec<String>>,
    pub logo_url: Option<String>,
    pub author_name: String,
    pub author_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAppRequest {
    pub name: Option<String>,
    pub short_description: Option<String>,
    pub description: Option<String>,
    pub homepage_url: Option<String>,
    pub api_url: Option<String>,
    pub api_spec_url: Option<String>,
    pub protocol: Option<String>,
    pub category: Option<String>,
    pub tags: Option<Vec<String>>,
    pub logo_url: Option<String>,
    pub author_name: Option<String>,
    pub author_url: Option<String>,
    pub status: Option<String>,
    pub is_featured: Option<bool>,
    pub is_verified: Option<bool>,
}

// === Review Models ===

#[derive(Debug, Serialize)]
#[allow(dead_code)]
pub struct Review {
    pub id: String,
    pub app_id: String,
    pub rating: i64,
    pub title: Option<String>,
    pub body: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct SubmitReviewRequest {
    pub rating: i64,
    pub title: Option<String>,
    pub body: Option<String>,
    pub reviewer_name: Option<String>,
}

// === Search / List Models ===

#[derive(Debug, Deserialize, FromForm)]
#[allow(dead_code)]
pub struct ListAppsQuery {
    pub category: Option<String>,
    pub protocol: Option<String>,
    pub status: Option<String>,
    pub sort: Option<String>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

#[derive(Debug, Deserialize, FromForm)]
#[allow(dead_code)]
pub struct SearchQuery {
    pub q: String,
    pub category: Option<String>,
    pub protocol: Option<String>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

#[derive(Debug, Serialize)]
#[allow(dead_code)]
pub struct PaginatedResponse<T: Serialize> {
    pub items: Vec<T>,
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
}

// === Category Model ===

#[derive(Debug, Serialize)]
#[allow(dead_code)]
pub struct CategoryInfo {
    pub name: String,
    pub count: i64,
}

// === Protocols & Categories (constants) ===

pub const VALID_PROTOCOLS: &[&str] = &[
    "rest",
    "graphql",
    "grpc",
    "mcp",
    "a2a",
    "websocket",
    "other",
];

pub const VALID_CATEGORIES: &[&str] = &[
    "communication",
    "data",
    "developer-tools",
    "finance",
    "media",
    "productivity",
    "search",
    "security",
    "social",
    "ai-ml",
    "infrastructure",
    "other",
];

pub const VALID_STATUSES: &[&str] = &["pending", "approved", "rejected", "deprecated"];

/// Generate a URL-safe slug from a name
pub fn slugify(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<&str>>()
        .join("-")
}
