use rocket::http::{ContentType, Header, Status};
use rocket::local::blocking::Client;
use serde_json::Value;

fn setup_client() -> (Client, String) {
    // Use a unique temp DB per test
    let db_path = format!("/tmp/test_app_dir_{}.db", uuid::Uuid::new_v4());
    std::env::set_var("DATABASE_PATH", &db_path);

    let rocket = app_directory::rocket();
    let client = Client::tracked(rocket).expect("valid rocket instance");

    // Get admin key from stdout capture — instead, create one via the DB
    // We need to extract the admin key. Let's hit the health endpoint first,
    // then create a key via direct DB access.
    // Actually, the admin key is printed to stdout. Let's just create a known key.
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let key: String = conn
        .query_row(
            "SELECT key_hash FROM api_keys WHERE is_admin = 1 LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();

    // We can't reverse the hash, so let's create a new admin key with known value
    let test_key = "ad_testkey12345678901234567890";
    let key_hash = {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        test_key.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    };
    conn.execute(
        "INSERT INTO api_keys (id, name, key_hash, is_admin, rate_limit) VALUES ('test-admin', 'test', ?1, 1, 10000)",
        rusqlite::params![key_hash],
    ).unwrap();

    drop(conn);
    let _ = key; // suppress unused warning

    (client, test_key.to_string())
}

#[test]
fn test_health() {
    let (client, _) = setup_client();
    let response = client.get("/api/v1/health").dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["service"], "app-directory");
}

#[test]
fn test_auth_required() {
    let (client, _) = setup_client();
    let response = client.get("/api/v1/apps").dispatch();
    assert_eq!(response.status(), Status::Unauthorized);
}

#[test]
fn test_submit_and_get_app() {
    let (client, key) = setup_client();

    // Submit an app
    let response = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(
            r#"{
                "name": "Test QR Service",
                "short_description": "Generate QR codes via API",
                "description": "A full-featured QR code generation and decoding service for AI agents",
                "api_url": "https://qr.example.com/api/v1",
                "protocol": "rest",
                "category": "developer-tools",
                "tags": ["qr", "image", "encoding"],
                "author_name": "Nanook"
            }"#,
        )
        .dispatch();

    assert_eq!(response.status(), Status::Created);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = body["id"].as_str().unwrap().to_string();
    assert_eq!(body["status"], "approved"); // admin auto-approves
    assert_eq!(body["slug"], "test-qr-service");

    // Get by ID
    let response = client
        .get(format!("/api/v1/apps/{}", app_id))
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["name"], "Test QR Service");
    assert_eq!(body["protocol"], "rest");
    assert_eq!(body["category"], "developer-tools");

    // Get by slug
    let response = client
        .get("/api/v1/apps/test-qr-service")
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["name"], "Test QR Service");
}

#[test]
fn test_list_apps() {
    let (client, key) = setup_client();

    // Submit two apps
    for name in &["App Alpha", "App Beta"] {
        let body = serde_json::json!({
            "name": name,
            "short_description": "Test app",
            "description": "A test application",
            "author_name": "Tester"
        });
        client
            .post("/api/v1/apps")
            .header(Header::new("X-API-Key", key.clone()))
            .header(ContentType::JSON)
            .body(body.to_string())
            .dispatch();
    }

    // List
    let response = client
        .get("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["total"], 2);
    assert_eq!(body["items"].as_array().unwrap().len(), 2);
}

#[test]
fn test_search_apps() {
    let (client, key) = setup_client();

    // Submit app
    client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(
            r#"{
                "name": "Weather Service",
                "short_description": "Get weather forecasts",
                "description": "Real-time weather data for agents",
                "protocol": "rest",
                "category": "data",
                "tags": ["weather", "forecast", "climate"],
                "author_name": "WeatherBot"
            }"#,
        )
        .dispatch();

    // Search by name
    let response = client
        .get("/api/v1/apps/search?q=weather")
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["total"], 1);

    // Search by tag
    let response = client
        .get("/api/v1/apps/search?q=forecast")
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["total"], 1);

    // Search miss
    let response = client
        .get("/api/v1/apps/search?q=nonexistent")
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["total"], 0);
}

#[test]
fn test_submit_and_get_review() {
    let (client, key) = setup_client();

    // Submit app
    let response = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(
            r#"{
                "name": "Review Target",
                "short_description": "App to review",
                "description": "Testing reviews",
                "author_name": "Author"
            }"#,
        )
        .dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = body["id"].as_str().unwrap().to_string();

    // Submit review
    let response = client
        .post(format!("/api/v1/apps/{}/reviews", app_id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{ "rating": 4, "title": "Solid service", "body": "Works well for my use case" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Created);

    // Get reviews
    let response = client
        .get(format!("/api/v1/apps/{}/reviews", app_id))
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["total"], 1);
    assert_eq!(body["items"][0]["rating"], 4);

    // Check avg rating was updated
    let response = client
        .get(format!("/api/v1/apps/{}", app_id))
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["avg_rating"], 4.0);
    assert_eq!(body["review_count"], 1);
}

#[test]
fn test_update_app() {
    let (client, key) = setup_client();

    let response = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(
            r#"{
                "name": "Original Name",
                "short_description": "Original desc",
                "description": "Original description",
                "author_name": "Author"
            }"#,
        )
        .dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = body["id"].as_str().unwrap().to_string();

    // Update
    let response = client
        .patch(format!("/api/v1/apps/{}", app_id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{ "name": "Updated Name", "category": "ai-ml" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Ok);

    // Verify
    let response = client
        .get(format!("/api/v1/apps/{}", app_id))
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["name"], "Updated Name");
    assert_eq!(body["category"], "ai-ml");
}

#[test]
fn test_delete_app() {
    let (client, key) = setup_client();

    let response = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(
            r#"{
                "name": "To Delete",
                "short_description": "Will be deleted",
                "description": "Delete me",
                "author_name": "Author"
            }"#,
        )
        .dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = body["id"].as_str().unwrap().to_string();

    // Delete
    let response = client
        .delete(format!("/api/v1/apps/{}", app_id))
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);

    // Verify gone
    let response = client
        .get(format!("/api/v1/apps/{}", app_id))
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::NotFound);
}

#[test]
fn test_invalid_protocol() {
    let (client, key) = setup_client();

    let response = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(
            r#"{
                "name": "Bad Protocol",
                "short_description": "Uses invalid protocol",
                "description": "Should fail",
                "protocol": "carrier-pigeon",
                "author_name": "Author"
            }"#,
        )
        .dispatch();
    assert_eq!(response.status(), Status::BadRequest);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["error"], "INVALID_PROTOCOL");
}

#[test]
fn test_categories_endpoint() {
    let (client, key) = setup_client();

    // Submit app in a specific category
    client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(
            r#"{
                "name": "Data Service",
                "short_description": "Data stuff",
                "description": "A data service",
                "category": "data",
                "author_name": "Author"
            }"#,
        )
        .dispatch();

    let response = client
        .get("/api/v1/categories")
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert!(body["valid_categories"].as_array().unwrap().len() > 0);
    assert!(body["valid_protocols"].as_array().unwrap().len() > 0);
    assert_eq!(body["categories"][0]["name"], "data");
    assert_eq!(body["categories"][0]["count"], 1);
}

#[test]
fn test_api_key_management() {
    let (client, key) = setup_client();

    // List keys
    let response = client
        .get("/api/v1/keys")
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);

    // Create key
    let response = client
        .post("/api/v1/keys")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{ "name": "new-agent" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Created);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert!(body["key"].as_str().unwrap().starts_with("ad_"));
}

#[test]
fn test_slugify() {
    use app_directory::models::slugify;
    assert_eq!(slugify("My Cool App"), "my-cool-app");
    assert_eq!(slugify("test!!!app***"), "test-app");
    assert_eq!(slugify("  spaces  everywhere  "), "spaces-everywhere");
}

#[test]
fn test_rate_limiting() {
    // Create a client with a low-rate-limit key
    let db_path = format!("/tmp/test_app_dir_{}.db", uuid::Uuid::new_v4());
    std::env::set_var("DATABASE_PATH", &db_path);
    std::env::set_var("RATE_LIMIT_WINDOW_SECS", "60");

    let rocket = app_directory::rocket();
    let client = Client::tracked(rocket).expect("valid rocket instance");

    // Create a key with rate_limit = 3
    let test_key = "ad_ratelimitkey_test_12345678";
    let key_hash = {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        test_key.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    };
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute(
        "INSERT INTO api_keys (id, name, key_hash, is_admin, rate_limit) VALUES ('rl-test', 'rate-test', ?1, 0, 3)",
        rusqlite::params![key_hash],
    ).unwrap();
    drop(conn);

    // First 3 requests should succeed with rate limit headers (use categories — authenticated endpoint)
    for i in 0..3 {
        let response = client
            .get("/api/v1/categories")
            .header(Header::new("X-API-Key", test_key))
            .dispatch();
        assert_eq!(
            response.status(),
            Status::Ok,
            "Request {} should succeed",
            i + 1
        );

        // Check rate limit headers are present
        let limit = response.headers().get_one("X-RateLimit-Limit").unwrap();
        assert_eq!(limit, "3");

        let remaining = response.headers().get_one("X-RateLimit-Remaining").unwrap();
        assert_eq!(remaining, (2 - i).to_string());
    }

    // 4th request should be rate limited
    let response = client
        .get("/api/v1/categories")
        .header(Header::new("X-API-Key", test_key))
        .dispatch();
    assert_eq!(response.status(), Status::TooManyRequests);
}

#[test]
fn test_rate_limit_headers_present() {
    let (client, key) = setup_client();

    let response = client
        .get("/api/v1/categories")
        .header(Header::new("X-API-Key", key))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);

    // Rate limit headers should be present on authenticated endpoints
    assert!(response.headers().get_one("X-RateLimit-Limit").is_some());
    assert!(response
        .headers()
        .get_one("X-RateLimit-Remaining")
        .is_some());
    assert!(response.headers().get_one("X-RateLimit-Reset").is_some());
}

#[test]
fn test_badges_default_false() {
    let (client, key) = setup_client();

    // Submit app — badges should default to false
    let response = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(
            r#"{
                "name": "Badge Test App",
                "short_description": "Testing badges",
                "description": "An app to test featured/verified badges",
                "author_name": "Tester"
            }"#,
        )
        .dispatch();
    assert_eq!(response.status(), Status::Created);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = body["id"].as_str().unwrap().to_string();

    // Get app — badges should be false
    let response = client
        .get(format!("/api/v1/apps/{}", app_id))
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["is_featured"], false);
    assert_eq!(body["is_verified"], false);
}

#[test]
fn test_admin_set_badges() {
    let (client, key) = setup_client();

    // Submit app
    let response = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(
            r#"{
                "name": "Featured App",
                "short_description": "Will be featured",
                "description": "Admin will feature and verify this app",
                "author_name": "Builder"
            }"#,
        )
        .dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = body["id"].as_str().unwrap().to_string();

    // Admin sets featured badge
    let response = client
        .patch(format!("/api/v1/apps/{}", app_id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{ "is_featured": true, "is_verified": true }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Ok);

    // Verify badges are set
    let response = client
        .get(format!("/api/v1/apps/{}", app_id))
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["is_featured"], true);
    assert_eq!(body["is_verified"], true);

    // Unset featured
    let response = client
        .patch(format!("/api/v1/apps/{}", app_id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{ "is_featured": false }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Ok);

    // Verify only verified remains
    let response = client
        .get(format!("/api/v1/apps/{}", app_id))
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["is_featured"], false);
    assert_eq!(body["is_verified"], true);
}

#[test]
fn test_non_admin_cannot_set_badges() {
    let (client, admin_key) = setup_client();

    // Create a non-admin key
    let response = client
        .post("/api/v1/keys")
        .header(Header::new("X-API-Key", admin_key.clone()))
        .header(ContentType::JSON)
        .body(r#"{ "name": "regular-agent" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Created);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let agent_key = body["key"].as_str().unwrap().to_string();

    // Submit app as regular agent
    let response = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", agent_key.clone()))
        .header(ContentType::JSON)
        .body(
            r#"{
                "name": "Agent App",
                "short_description": "Regular agent app",
                "description": "A normal app submission",
                "author_name": "Agent"
            }"#,
        )
        .dispatch();
    assert_eq!(response.status(), Status::Created);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = body["id"].as_str().unwrap().to_string();

    // Non-admin tries to set badges — should be forbidden
    let response = client
        .patch(format!("/api/v1/apps/{}", app_id))
        .header(Header::new("X-API-Key", agent_key.clone()))
        .header(ContentType::JSON)
        .body(r#"{ "is_featured": true }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Forbidden);

    // Non-admin can still update normal fields
    let response = client
        .patch(format!("/api/v1/apps/{}", app_id))
        .header(Header::new("X-API-Key", agent_key.clone()))
        .header(ContentType::JSON)
        .body(r#"{ "short_description": "Updated description" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
}

#[test]
fn test_filter_featured_apps() {
    let (client, key) = setup_client();

    // Submit 3 apps
    let mut app_ids = Vec::new();
    for name in &["App One", "App Two", "App Three"] {
        let response = client
            .post("/api/v1/apps")
            .header(Header::new("X-API-Key", key.clone()))
            .header(ContentType::JSON)
            .body(
                serde_json::json!({
                    "name": name,
                    "short_description": "Test app",
                    "description": "Testing badge filters",
                    "author_name": "Tester"
                })
                .to_string(),
            )
            .dispatch();
        let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
        app_ids.push(body["id"].as_str().unwrap().to_string());
    }

    // Feature only the first app
    client
        .patch(format!("/api/v1/apps/{}", app_ids[0]))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{ "is_featured": true }"#)
        .dispatch();

    // Verify the second app
    client
        .patch(format!("/api/v1/apps/{}", app_ids[1]))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{ "is_verified": true }"#)
        .dispatch();

    // Filter by featured
    let response = client
        .get("/api/v1/apps?featured=true")
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["total"], 1);
    assert_eq!(body["items"][0]["name"], "App One");

    // Filter by verified
    let response = client
        .get("/api/v1/apps?verified=true")
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["total"], 1);
    assert_eq!(body["items"][0]["name"], "App Two");

    // No filter — all 3 apps
    let response = client
        .get("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["total"], 3);
}

// === Health Check Tests ===

#[test]
fn test_health_check_requires_admin() {
    let (client, key) = setup_client();

    // Create a non-admin key
    let response = client
        .post("/api/v1/keys")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{ "name": "agent-key" }"#)
        .dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let agent_key = body["key"].as_str().unwrap().to_string();

    // Submit an app with a URL
    let response = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(
            r#"{
                "name": "Health Test App",
                "short_description": "For health check testing",
                "description": "Testing health checks",
                "api_url": "https://httpbin.org/status/200",
                "author_name": "Tester"
            }"#,
        )
        .dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = body["id"].as_str().unwrap().to_string();

    // Non-admin should be forbidden
    let response = client
        .post(format!("/api/v1/apps/{}/health-check", app_id))
        .header(Header::new("X-API-Key", agent_key))
        .dispatch();
    assert_eq!(response.status(), Status::Forbidden);
}

#[test]
fn test_health_check_no_url() {
    let (client, key) = setup_client();

    // Submit an app WITHOUT any URL
    let response = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(
            r#"{
                "name": "No URL App",
                "short_description": "Has no URL",
                "description": "Testing health check with no URL",
                "author_name": "Tester"
            }"#,
        )
        .dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = body["id"].as_str().unwrap().to_string();

    // Health check should return 422 (no URL to check)
    let response = client
        .post(format!("/api/v1/apps/{}/health-check", app_id))
        .header(Header::new("X-API-Key", key))
        .dispatch();
    assert_eq!(response.status(), Status::UnprocessableEntity);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["error"], "NO_URL");
}

#[test]
fn test_health_check_not_found() {
    let (client, key) = setup_client();

    let response = client
        .post("/api/v1/apps/nonexistent-id/health-check")
        .header(Header::new("X-API-Key", key))
        .dispatch();
    assert_eq!(response.status(), Status::NotFound);
}

#[test]
fn test_health_history_empty() {
    let (client, key) = setup_client();

    // Submit an app
    let response = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(
            r#"{
                "name": "History App",
                "short_description": "For health history testing",
                "description": "Testing health history",
                "api_url": "https://example.com",
                "author_name": "Tester"
            }"#,
        )
        .dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = body["id"].as_str().unwrap().to_string();

    // Health history should be empty
    let response = client
        .get(format!("/api/v1/apps/{}/health", app_id))
        .header(Header::new("X-API-Key", key))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["total"], 0);
    assert_eq!(body["checks"].as_array().unwrap().len(), 0);
}

#[test]
fn test_health_summary() {
    let (client, key) = setup_client();

    // Health summary with no apps
    let response = client
        .get("/api/v1/apps/health/summary")
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["total_approved_apps"], 0);
    assert_eq!(body["monitored"], 0);
    assert_eq!(body["healthy"], 0);
    assert_eq!(body["issues"].as_array().unwrap().len(), 0);
}

#[test]
fn test_app_includes_health_fields() {
    let (client, key) = setup_client();

    // Submit an app
    let response = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(
            r#"{
                "name": "Fields App",
                "short_description": "Check health fields in response",
                "description": "Testing that health fields appear",
                "author_name": "Tester"
            }"#,
        )
        .dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = body["id"].as_str().unwrap().to_string();

    // Get app — should include health fields (null by default)
    let response = client
        .get(format!("/api/v1/apps/{}", app_id))
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert!(body.get("last_health_status").is_some());
    assert!(body.get("last_checked_at").is_some());
    assert!(body.get("uptime_pct").is_some());
    // Should be null initially
    assert!(body["last_health_status"].is_null());
    assert!(body["last_checked_at"].is_null());

    // List apps — should include health fields
    let response = client
        .get("/api/v1/apps")
        .header(Header::new("X-API-Key", key))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let first_app = &body["items"][0];
    assert!(first_app.get("last_health_status").is_some());
    assert!(first_app.get("last_checked_at").is_some());
    assert!(first_app.get("uptime_pct").is_some());
}

#[test]
fn test_health_filter_on_list() {
    let (client, key) = setup_client();

    // Submit an app
    let response = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(
            r#"{
                "name": "Unmonitored App",
                "short_description": "No health checks yet",
                "description": "Testing health filter",
                "author_name": "Tester"
            }"#,
        )
        .dispatch();
    assert_eq!(response.status(), Status::Created);

    // Filter by health=unknown (no checks yet) — should find the app
    let response = client
        .get("/api/v1/apps?health=unknown")
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["total"], 1);

    // Filter by health=healthy — should find nothing
    let response = client
        .get("/api/v1/apps?health=healthy")
        .header(Header::new("X-API-Key", key))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["total"], 0);
}

// === Webhook Tests ===

#[test]
fn test_webhook_crud() {
    let (client, key) = setup_client();

    // Create webhook
    let response = client
        .post("/api/v1/webhooks")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"url": "https://example.com/hook", "events": ["app.submitted", "review.submitted"]}"#)
        .dispatch();
    assert_eq!(response.status(), Status::Created);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let webhook_id = body["id"].as_str().unwrap().to_string();
    assert!(body["secret"].as_str().unwrap().starts_with("whsec_"));
    assert_eq!(body["active"], true);
    assert_eq!(body["events"].as_array().unwrap().len(), 2);

    // List webhooks
    let response = client
        .get("/api/v1/webhooks")
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["webhooks"].as_array().unwrap().len(), 1);
    // Secret should NOT be shown in list
    assert!(body["webhooks"][0].get("secret").is_none() || body["webhooks"][0]["secret"].is_null());

    // Update webhook (change URL, deactivate)
    let response = client
        .patch(format!("/api/v1/webhooks/{}", webhook_id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"url": "https://example.com/hook2", "active": false}"#)
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["url"], "https://example.com/hook2");
    assert_eq!(body["active"], false);

    // Re-activate (resets failure count)
    let response = client
        .patch(format!("/api/v1/webhooks/{}", webhook_id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"active": true}"#)
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["active"], true);
    assert_eq!(body["failure_count"], 0);

    // Delete webhook
    let response = client
        .delete(format!("/api/v1/webhooks/{}", webhook_id))
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);

    // Verify deleted
    let response = client
        .get("/api/v1/webhooks")
        .header(Header::new("X-API-Key", key))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["webhooks"].as_array().unwrap().len(), 0);
}

#[test]
fn test_webhook_requires_admin() {
    let (client, _admin_key) = setup_client();

    // Create a non-admin key
    let response = client
        .post("/api/v1/keys")
        .header(Header::new("X-API-Key", _admin_key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name": "regular-user"}"#)
        .dispatch();
    assert_eq!(response.status(), Status::Created);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let user_key = body["key"].as_str().unwrap().to_string();

    // Try to create webhook with non-admin key
    let response = client
        .post("/api/v1/webhooks")
        .header(Header::new("X-API-Key", user_key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"url": "https://example.com/hook"}"#)
        .dispatch();
    assert_eq!(response.status(), Status::Forbidden);

    // Try to list webhooks with non-admin key
    let response = client
        .get("/api/v1/webhooks")
        .header(Header::new("X-API-Key", user_key))
        .dispatch();
    assert_eq!(response.status(), Status::Forbidden);
}

#[test]
fn test_webhook_validation() {
    let (client, key) = setup_client();

    // Invalid URL (no http/https)
    let response = client
        .post("/api/v1/webhooks")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"url": "ftp://example.com/hook"}"#)
        .dispatch();
    assert_eq!(response.status(), Status::BadRequest);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["error"], "INVALID_URL");

    // Invalid event type
    let response = client
        .post("/api/v1/webhooks")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"url": "https://example.com/hook", "events": ["invalid.event"]}"#)
        .dispatch();
    assert_eq!(response.status(), Status::BadRequest);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["error"], "INVALID_EVENT");

    // Empty events = subscribe to all (should succeed)
    let response = client
        .post("/api/v1/webhooks")
        .header(Header::new("X-API-Key", key))
        .header(ContentType::JSON)
        .body(r#"{"url": "https://example.com/hook", "events": []}"#)
        .dispatch();
    assert_eq!(response.status(), Status::Created);
}

#[test]
fn test_webhook_not_found() {
    let (client, key) = setup_client();

    // Update non-existent webhook
    let response = client
        .patch("/api/v1/webhooks/nonexistent")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"active": false}"#)
        .dispatch();
    assert_eq!(response.status(), Status::NotFound);

    // Delete non-existent webhook
    let response = client
        .delete("/api/v1/webhooks/nonexistent")
        .header(Header::new("X-API-Key", key))
        .dispatch();
    assert_eq!(response.status(), Status::NotFound);
}
