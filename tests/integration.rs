use rocket::http::{ContentType, Header, Status};
use rocket::local::blocking::Client;
use serde_json::Value;

fn setup_client() -> (Client, String) {
    let (client, key, _path) = setup_client_with_path();
    (client, key)
}

/// Build a test client and return the DB path for tests that need direct DB access.
/// Uses `rocket_with_path` to avoid process-global env var races in parallel tests.
fn setup_client_with_path() -> (Client, String, String) {
    let db_path = format!("/tmp/test_app_dir_{}.db", uuid::Uuid::new_v4());

    // Build the rocket client with the explicit DB path (no env var needed)
    let rocket = app_directory::rocket_with_path(&db_path);
    let client = Client::tracked(rocket).expect("valid rocket instance");

    // Create a test admin key with a known value
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let test_key = app_directory::auth::create_api_key(&conn, "test-admin", true, Some(10000));
    drop(conn);

    (client, test_key, db_path)
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
fn test_public_endpoints_no_auth() {
    let (client, _) = setup_client();
    // List apps is public (no auth required)
    let response = client.get("/api/v1/apps").dispatch();
    assert_eq!(response.status(), Status::Ok);
    // Categories is public
    let response = client.get("/api/v1/categories").dispatch();
    assert_eq!(response.status(), Status::Ok);
    // Search is public
    let response = client.get("/api/v1/apps/search?q=test").dispatch();
    assert_eq!(response.status(), Status::Ok);
    // Admin endpoints still require auth
    let response = client.get("/api/v1/keys").dispatch();
    assert_eq!(response.status(), Status::Unauthorized);
    let response = client.get("/api/v1/webhooks").dispatch();
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
    let app_id = body["app_id"].as_str().unwrap().to_string();
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
    assert_eq!(body["apps"].as_array().unwrap().len(), 2);
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
    let app_id = body["app_id"].as_str().unwrap().to_string();

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
    assert_eq!(body["reviews"][0]["rating"], 4);

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
    let app_id = body["app_id"].as_str().unwrap().to_string();

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
    let app_id = body["app_id"].as_str().unwrap().to_string();

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
    assert!(!body["valid_categories"].as_array().unwrap().is_empty());
    assert!(!body["valid_protocols"].as_array().unwrap().is_empty());
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
    assert!(body["api_key"].as_str().unwrap().starts_with("ad_"));
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

    let rocket = app_directory::rocket_with_path(&db_path);
    let client = Client::tracked(rocket).expect("valid rocket instance");

    // Create a low-rate-limit key
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let test_key = app_directory::auth::create_api_key(&conn, "rate-test", false, Some(3));
    drop(conn);

    // First 3 requests should succeed with rate limit headers (use /apps/mine — requires auth)
    for i in 0..3 {
        let response = client
            .get("/api/v1/apps/mine")
            .header(Header::new("X-API-Key", test_key.clone()))
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
        .get("/api/v1/apps/mine")
        .header(Header::new("X-API-Key", test_key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::TooManyRequests);
}

#[test]
fn test_rate_limit_headers_present() {
    let (client, key) = setup_client();

    // Use /apps/mine — requires auth, so rate limit headers appear
    let response = client
        .get("/api/v1/apps/mine")
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
    let app_id = body["app_id"].as_str().unwrap().to_string();

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
    let app_id = body["app_id"].as_str().unwrap().to_string();

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
    let agent_key = body["api_key"].as_str().unwrap().to_string();

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
    let app_id = body["app_id"].as_str().unwrap().to_string();

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
        app_ids.push(body["app_id"].as_str().unwrap().to_string());
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
    assert_eq!(body["apps"][0]["name"], "App One");

    // Filter by verified
    let response = client
        .get("/api/v1/apps?verified=true")
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["total"], 1);
    assert_eq!(body["apps"][0]["name"], "App Two");

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
    let agent_key = body["api_key"].as_str().unwrap().to_string();

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
    let app_id = body["app_id"].as_str().unwrap().to_string();

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
    let app_id = body["app_id"].as_str().unwrap().to_string();

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
    let app_id = body["app_id"].as_str().unwrap().to_string();

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
    let app_id = body["app_id"].as_str().unwrap().to_string();

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
    let first_app = &body["apps"][0];
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
    let user_key = body["api_key"].as_str().unwrap().to_string();

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

#[test]
fn test_schedule_endpoint() {
    let (client, key, db_path) = setup_client_with_path();

    // Admin can view schedule
    let response = client
        .get("/api/v1/health-check/schedule")
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert!(body["enabled"].is_boolean());
    assert!(body["interval_seconds"].is_number());
    assert_eq!(body["config_var"], "HEALTH_CHECK_INTERVAL_SECS");
    assert_eq!(body["default_interval"], 300);

    // Non-admin cannot view schedule
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let viewer_key = app_directory::auth::create_api_key(&conn, "viewer", false, Some(100));
    drop(conn);

    let response = client
        .get("/api/v1/health-check/schedule")
        .header(Header::new("X-API-Key", viewer_key))
        .dispatch();
    assert_eq!(response.status(), Status::Forbidden);
}

// === Approval Workflow Tests ===

#[test]
fn test_approve_pending_app() {
    let (client, admin_key) = setup_client();

    // All submissions auto-approve in the open-read model
    let response = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", admin_key.clone()))
        .header(ContentType::JSON)
        .body(
            r#"{
                "name": "Auto Approved App",
                "short_description": "Should be approved immediately",
                "description": "All submissions go live immediately",
                "author_name": "Test Agent"
            }"#,
        )
        .dispatch();
    assert_eq!(response.status(), Status::Created);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = body["app_id"].as_str().unwrap().to_string();
    assert_eq!(body["status"], "approved"); // auto-approved

    // Pending list should be empty (no pending queue in open-read model)
    let response = client
        .get("/api/v1/apps/pending")
        .header(Header::new("X-API-Key", admin_key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["total"], 0);

    // Approving an already-approved app should 409
    let response = client
        .post(format!("/api/v1/apps/{}/approve", app_id))
        .header(Header::new("X-API-Key", admin_key.clone()))
        .header(ContentType::JSON)
        .body(r#"{ "note": "Already approved" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Conflict);

    // Admin can reject an approved app
    let response = client
        .post(format!("/api/v1/apps/{}/reject", app_id))
        .header(Header::new("X-API-Key", admin_key.clone()))
        .header(ContentType::JSON)
        .body(r#"{ "reason": "Violates policy" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["message"], "App rejected");
    assert_eq!(body["previous_status"], "approved");

    // Verify app is now rejected
    let response = client
        .get(format!("/api/v1/apps/{}", app_id))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["status"], "rejected");

    // Admin can re-approve a rejected app
    let response = client
        .post(format!("/api/v1/apps/{}/approve", app_id))
        .header(Header::new("X-API-Key", admin_key.clone()))
        .header(ContentType::JSON)
        .body(r#"{ "note": "Reconsidered — looks fine" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["message"], "App approved");
    assert_eq!(body["previous_status"], "rejected");

    // Verify review metadata
    let response = client
        .get(format!("/api/v1/apps/{}", app_id))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["status"], "approved");
    assert_eq!(body["review_note"], "Reconsidered — looks fine");
    assert!(body["reviewed_at"].is_string());
}

#[test]
fn test_reject_pending_app() {
    let (client, admin_key) = setup_client();

    // Submit app (auto-approved)
    let response = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", admin_key.clone()))
        .header(ContentType::JSON)
        .body(
            r#"{
                "name": "Bad App",
                "short_description": "Gonna get rejected",
                "description": "Spam or low quality",
                "author_name": "Spammer"
            }"#,
        )
        .dispatch();
    assert_eq!(response.status(), Status::Created);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = body["app_id"].as_str().unwrap().to_string();
    assert_eq!(body["status"], "approved"); // auto-approved

    // Reject without reason should fail
    let response = client
        .post(format!("/api/v1/apps/{}/reject", app_id))
        .header(Header::new("X-API-Key", admin_key.clone()))
        .header(ContentType::JSON)
        .body(r#"{ "reason": "" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::BadRequest);

    // Reject with reason
    let response = client
        .post(format!("/api/v1/apps/{}/reject", app_id))
        .header(Header::new("X-API-Key", admin_key.clone()))
        .header(ContentType::JSON)
        .body(r#"{ "reason": "Low quality submission, no API URL provided" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["message"], "App rejected");
    assert_eq!(
        body["reason"],
        "Low quality submission, no API URL provided"
    );

    // Verify app status
    let response = client
        .get(format!("/api/v1/apps/{}", app_id))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["status"], "rejected");
    assert_eq!(
        body["review_note"],
        "Low quality submission, no API URL provided"
    );

    // Rejecting again should 409
    let response = client
        .post(format!("/api/v1/apps/{}/reject", app_id))
        .header(Header::new("X-API-Key", admin_key.clone()))
        .header(ContentType::JSON)
        .body(r#"{ "reason": "Still bad" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Conflict);

    // But can re-approve a rejected app
    let response = client
        .post(format!("/api/v1/apps/{}/approve", app_id))
        .header(Header::new("X-API-Key", admin_key.clone()))
        .header(ContentType::JSON)
        .body(r#"{ "note": "On second thought, this is fine" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["previous_status"], "rejected");
}

#[test]
fn test_approval_requires_admin() {
    let (client, admin_key) = setup_client();

    // Create a non-admin key
    let response = client
        .post("/api/v1/keys")
        .header(Header::new("X-API-Key", admin_key.clone()))
        .header(ContentType::JSON)
        .body(r#"{ "name": "agent" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Created);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let agent_key = body["api_key"].as_str().unwrap().to_string();

    // Submit app (auto-approved)
    let response = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", agent_key.clone()))
        .header(ContentType::JSON)
        .body(
            r#"{
                "name": "My App",
                "short_description": "Test",
                "description": "Test app",
                "author_name": "Agent"
            }"#,
        )
        .dispatch();
    assert_eq!(response.status(), Status::Created);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = body["app_id"].as_str().unwrap().to_string();
    assert_eq!(body["status"], "approved"); // auto-approved

    // Non-admin tries to reject — forbidden
    let response = client
        .post(format!("/api/v1/apps/{}/reject", app_id))
        .header(Header::new("X-API-Key", agent_key.clone()))
        .header(ContentType::JSON)
        .body(r#"{ "reason": "I don't like it" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Forbidden);

    // Non-admin tries to view pending list — forbidden
    let response = client
        .get("/api/v1/apps/pending")
        .header(Header::new("X-API-Key", agent_key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Forbidden);
}

#[test]
fn test_app_stats() {
    let (client, admin_key) = setup_client();

    // Submit an app (auto-approved as admin)
    let response = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", admin_key.clone()))
        .header(ContentType::JSON)
        .body(r#"{ "name": "Stats App", "short_description": "Testing stats", "description": "Full desc", "author_name": "TestBot" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Created);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = body["app_id"].as_str().unwrap().to_string();

    // Stats should start at zero
    let response = client
        .get(format!("/api/v1/apps/{}/stats", app_id))
        .header(Header::new("X-API-Key", admin_key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let stats: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(stats["total_views"].as_i64().unwrap(), 0);
    assert_eq!(stats["unique_viewers"].as_i64().unwrap(), 0);

    // View the app 3 times (get_app records views)
    for _ in 0..3 {
        let response = client
            .get(format!("/api/v1/apps/{}", app_id))
            .header(Header::new("X-API-Key", admin_key.clone()))
            .dispatch();
        assert_eq!(response.status(), Status::Ok);
    }

    // Now stats should show 3 views from 1 unique viewer
    let response = client
        .get(format!("/api/v1/apps/{}/stats", app_id))
        .header(Header::new("X-API-Key", admin_key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let stats: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(stats["total_views"].as_i64().unwrap(), 3);
    assert_eq!(stats["views_24h"].as_i64().unwrap(), 3);
    assert_eq!(stats["views_7d"].as_i64().unwrap(), 3);
    assert_eq!(stats["views_30d"].as_i64().unwrap(), 3);
    assert_eq!(stats["unique_viewers"].as_i64().unwrap(), 1);

    // Stats by slug also works
    let slug = body["slug"].as_str().unwrap();
    let response = client
        .get(format!("/api/v1/apps/{}/stats", slug))
        .header(Header::new("X-API-Key", admin_key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let stats: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(stats["total_views"].as_i64().unwrap(), 3); // stats endpoint doesn't add views

    // Stats for non-existent app returns 404
    let response = client
        .get("/api/v1/apps/nonexistent-id/stats")
        .header(Header::new("X-API-Key", admin_key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::NotFound);
}

#[test]
fn test_trending_apps() {
    let (client, admin_key) = setup_client();

    // Trending with no views returns empty
    let response = client
        .get("/api/v1/apps/trending")
        .header(Header::new("X-API-Key", admin_key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["trending"].as_array().unwrap().len(), 0);
    assert_eq!(body["period_days"].as_i64().unwrap(), 7);

    // Submit two apps
    let response = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", admin_key.clone()))
        .header(ContentType::JSON)
        .body(r#"{ "name": "Hot App", "short_description": "Very popular", "description": "Full desc", "author_name": "TestBot" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Created);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let hot_id = body["app_id"].as_str().unwrap().to_string();

    let response = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", admin_key.clone()))
        .header(ContentType::JSON)
        .body(r#"{ "name": "Cold App", "short_description": "Less popular", "description": "Full desc", "author_name": "TestBot" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Created);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let cold_id = body["app_id"].as_str().unwrap().to_string();

    // View hot app 5 times, cold app 2 times
    for _ in 0..5 {
        client
            .get(format!("/api/v1/apps/{}", hot_id))
            .header(Header::new("X-API-Key", admin_key.clone()))
            .dispatch();
    }
    for _ in 0..2 {
        client
            .get(format!("/api/v1/apps/{}", cold_id))
            .header(Header::new("X-API-Key", admin_key.clone()))
            .dispatch();
    }

    // Trending should show hot app first
    let response = client
        .get("/api/v1/apps/trending")
        .header(Header::new("X-API-Key", admin_key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let trending = body["trending"].as_array().unwrap();
    assert_eq!(trending.len(), 2);
    assert_eq!(trending[0]["name"].as_str().unwrap(), "Hot App");
    assert_eq!(trending[0]["view_count"].as_i64().unwrap(), 5);
    assert_eq!(trending[1]["name"].as_str().unwrap(), "Cold App");
    assert_eq!(trending[1]["view_count"].as_i64().unwrap(), 2);

    // Custom period and limit
    let response = client
        .get("/api/v1/apps/trending?days=1&limit=1")
        .header(Header::new("X-API-Key", admin_key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["trending"].as_array().unwrap().len(), 1);
    assert_eq!(body["period_days"].as_i64().unwrap(), 1);
}

#[test]
fn test_deprecation_workflow() {
    let (client, admin_key) = setup_client();

    // Submit an app (auto-approved because admin key)
    let response = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", admin_key.clone()))
        .header(ContentType::JSON)
        .body(r#"{ "name": "Old Service", "short_description": "The original", "description": "Full desc of old service", "author_name": "TestBot" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Created);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let old_app_id = body["app_id"].as_str().unwrap().to_string();

    // Submit a replacement app
    let response = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", admin_key.clone()))
        .header(ContentType::JSON)
        .body(r#"{ "name": "New Service", "short_description": "The replacement", "description": "Full desc of new service", "author_name": "TestBot" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Created);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let new_app_id = body["app_id"].as_str().unwrap().to_string();

    // Deprecate without reason → 400
    let response = client
        .post(format!("/api/v1/apps/{}/deprecate", old_app_id))
        .header(Header::new("X-API-Key", admin_key.clone()))
        .header(ContentType::JSON)
        .body(r#"{ "reason": "" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::BadRequest);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["error"].as_str().unwrap(), "REASON_REQUIRED");

    // Deprecate with invalid replacement → 400
    let response = client
        .post(format!("/api/v1/apps/{}/deprecate", old_app_id))
        .header(Header::new("X-API-Key", admin_key.clone()))
        .header(ContentType::JSON)
        .body(
            r#"{ "reason": "Replaced by newer version", "replacement_app_id": "nonexistent-id" }"#,
        )
        .dispatch();
    assert_eq!(response.status(), Status::BadRequest);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["error"].as_str().unwrap(), "INVALID_REPLACEMENT");

    // Self-replacement → 400
    let response = client
        .post(format!("/api/v1/apps/{}/deprecate", old_app_id))
        .header(Header::new("X-API-Key", admin_key.clone()))
        .header(ContentType::JSON)
        .body(format!(
            r#"{{ "reason": "Self-replace", "replacement_app_id": "{}" }}"#,
            old_app_id
        ))
        .dispatch();
    assert_eq!(response.status(), Status::BadRequest);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["error"].as_str().unwrap(), "INVALID_REPLACEMENT");

    // Deprecate with valid replacement and sunset date → 200
    let response = client
        .post(format!("/api/v1/apps/{}/deprecate", old_app_id))
        .header(Header::new("X-API-Key", admin_key.clone()))
        .header(ContentType::JSON)
        .body(format!(
            r#"{{ "reason": "Replaced by v2", "replacement_app_id": "{}", "sunset_at": "2026-06-01T00:00:00Z" }}"#,
            new_app_id
        ))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["message"].as_str().unwrap(), "App deprecated");
    assert_eq!(body["previous_status"].as_str().unwrap(), "approved");
    assert_eq!(body["replacement_app_id"].as_str().unwrap(), new_app_id);

    // Verify deprecation metadata on get_app
    let response = client
        .get(format!("/api/v1/apps/{}", old_app_id))
        .header(Header::new("X-API-Key", admin_key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["status"].as_str().unwrap(), "deprecated");
    assert_eq!(
        body["deprecated_reason"].as_str().unwrap(),
        "Replaced by v2"
    );
    assert_eq!(body["replacement_app_id"].as_str().unwrap(), new_app_id);
    assert_eq!(body["sunset_at"].as_str().unwrap(), "2026-06-01T00:00:00Z");
    assert!(body["deprecated_at"].as_str().is_some());
    assert!(body["deprecated_by"].as_str().is_some());

    // Already deprecated → 409
    let response = client
        .post(format!("/api/v1/apps/{}/deprecate", old_app_id))
        .header(Header::new("X-API-Key", admin_key.clone()))
        .header(ContentType::JSON)
        .body(r#"{ "reason": "Again" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Conflict);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["error"].as_str().unwrap(), "ALREADY_DEPRECATED");

    // Cannot approve deprecated app → 409
    let response = client
        .post(format!("/api/v1/apps/{}/approve", old_app_id))
        .header(Header::new("X-API-Key", admin_key.clone()))
        .header(ContentType::JSON)
        .body(r#"{ "note": "Try to approve" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Conflict);

    // Cannot reject deprecated app → 409
    let response = client
        .post(format!("/api/v1/apps/{}/reject", old_app_id))
        .header(Header::new("X-API-Key", admin_key.clone()))
        .header(ContentType::JSON)
        .body(r#"{ "reason": "Try to reject" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Conflict);

    // Undeprecate → 200 (restores to approved)
    let response = client
        .post(format!("/api/v1/apps/{}/undeprecate", old_app_id))
        .header(Header::new("X-API-Key", admin_key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["message"].as_str().unwrap(), "App undeprecated");
    assert_eq!(body["restored_to"].as_str().unwrap(), "approved");

    // Verify deprecation metadata cleared
    let response = client
        .get(format!("/api/v1/apps/{}", old_app_id))
        .header(Header::new("X-API-Key", admin_key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["status"].as_str().unwrap(), "approved");
    assert!(body["deprecated_reason"].is_null());
    assert!(body["replacement_app_id"].is_null());
    assert!(body["sunset_at"].is_null());

    // Undeprecate non-deprecated app → 409
    let response = client
        .post(format!("/api/v1/apps/{}/undeprecate", old_app_id))
        .header(Header::new("X-API-Key", admin_key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Conflict);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["error"].as_str().unwrap(), "NOT_DEPRECATED");

    // Undeprecate non-existent app → 404
    let response = client
        .post("/api/v1/apps/nonexistent-id/undeprecate")
        .header(Header::new("X-API-Key", admin_key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::NotFound);
}

#[test]
fn test_update_anonymous_app_as_admin() {
    let (client, key) = setup_client();

    // Submit app anonymously (no auth header)
    let response = client
        .post("/api/v1/apps")
        .header(ContentType::JSON)
        .body(
            r#"{
                "name": "Anon App",
                "short_description": "Submitted without auth",
                "description": "Anonymous submission",
                "author_name": "Anonymous",
                "api_url": "http://old-url.example.com/api"
            }"#,
        )
        .dispatch();
    assert_eq!(response.status(), Status::Created);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = body["app_id"].as_str().unwrap().to_string();

    // Admin should be able to update the anonymous app
    let response = client
        .patch(format!("/api/v1/apps/{}", app_id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{ "api_url": "http://new-url.example.com/api" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Ok);

    // Verify the update took effect
    let response = client
        .get(format!("/api/v1/apps/{}", app_id))
        .dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["api_url"], "http://new-url.example.com/api");
}

// === Edit Token Auth Tests ===

#[test]
fn test_update_app_with_edit_token_query_param() {
    let (client, _key) = setup_client();

    // Submit anonymously — returns edit_token
    let response = client
        .post("/api/v1/apps")
        .header(ContentType::JSON)
        .body(
            r#"{
                "name": "Token Edit Test",
                "short_description": "Test edit token",
                "description": "Testing edit via token",
                "author_name": "Agent"
            }"#,
        )
        .dispatch();
    assert_eq!(response.status(), Status::Created);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = body["app_id"].as_str().unwrap().to_string();
    let edit_token = body["edit_token"].as_str().unwrap().to_string();

    // Update via edit token in query param (no API key)
    let response = client
        .patch(format!("/api/v1/apps/{}?token={}", app_id, edit_token))
        .header(ContentType::JSON)
        .body(r#"{ "name": "Updated Via Token" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Ok);

    // Verify update
    let response = client
        .get(format!("/api/v1/apps/{}", app_id))
        .dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["name"], "Updated Via Token");
}

#[test]
fn test_update_app_with_edit_token_header() {
    let (client, _key) = setup_client();

    // Submit anonymously
    let response = client
        .post("/api/v1/apps")
        .header(ContentType::JSON)
        .body(
            r#"{
                "name": "Header Token Test",
                "short_description": "Test edit token header",
                "description": "Testing edit via X-Edit-Token header",
                "author_name": "Agent"
            }"#,
        )
        .dispatch();
    assert_eq!(response.status(), Status::Created);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = body["app_id"].as_str().unwrap().to_string();
    let edit_token = body["edit_token"].as_str().unwrap().to_string();

    // Update via X-Edit-Token header
    let response = client
        .patch(format!("/api/v1/apps/{}", app_id))
        .header(Header::new("X-Edit-Token", edit_token))
        .header(ContentType::JSON)
        .body(r#"{ "description": "Updated via header" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Ok);

    // Verify
    let response = client
        .get(format!("/api/v1/apps/{}", app_id))
        .dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["description"], "Updated via header");
}

#[test]
fn test_delete_app_with_edit_token() {
    let (client, _key) = setup_client();

    // Submit anonymously
    let response = client
        .post("/api/v1/apps")
        .header(ContentType::JSON)
        .body(
            r#"{
                "name": "Token Delete Test",
                "short_description": "Test delete via token",
                "description": "Will be deleted via edit token",
                "author_name": "Agent"
            }"#,
        )
        .dispatch();
    assert_eq!(response.status(), Status::Created);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = body["app_id"].as_str().unwrap().to_string();
    let edit_token = body["edit_token"].as_str().unwrap().to_string();

    // Delete via edit token (no API key)
    let response = client
        .delete(format!("/api/v1/apps/{}?token={}", app_id, edit_token))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);

    // Verify gone
    let response = client
        .get(format!("/api/v1/apps/{}", app_id))
        .dispatch();
    assert_eq!(response.status(), Status::NotFound);
}

#[test]
fn test_wrong_edit_token_rejected() {
    let (client, _key) = setup_client();

    // Submit anonymously
    let response = client
        .post("/api/v1/apps")
        .header(ContentType::JSON)
        .body(
            r#"{
                "name": "Wrong Token Test",
                "short_description": "Test wrong token",
                "description": "Should reject wrong token",
                "author_name": "Agent"
            }"#,
        )
        .dispatch();
    assert_eq!(response.status(), Status::Created);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = body["app_id"].as_str().unwrap().to_string();

    // Try to update with wrong edit token
    let response = client
        .patch(format!("/api/v1/apps/{}?token=ad_wrong_token_value", app_id))
        .header(ContentType::JSON)
        .body(r#"{ "name": "Should Not Work" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Forbidden);
}

#[test]
fn test_no_auth_update_rejected() {
    let (client, _key) = setup_client();

    // Submit anonymously
    let response = client
        .post("/api/v1/apps")
        .header(ContentType::JSON)
        .body(
            r#"{
                "name": "No Auth Test",
                "short_description": "Test no auth",
                "description": "Should reject with no auth",
                "author_name": "Agent"
            }"#,
        )
        .dispatch();
    assert_eq!(response.status(), Status::Created);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = body["app_id"].as_str().unwrap().to_string();

    // Try to update with NO auth at all
    let response = client
        .patch(format!("/api/v1/apps/{}", app_id))
        .header(ContentType::JSON)
        .body(r#"{ "name": "Should Not Work" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Unauthorized);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["error"], "UNAUTHORIZED");
}

#[test]
fn test_edit_token_cannot_set_badges() {
    let (client, _key) = setup_client();

    // Submit anonymously
    let response = client
        .post("/api/v1/apps")
        .header(ContentType::JSON)
        .body(
            r#"{
                "name": "Badge Test",
                "short_description": "Test badge restriction",
                "description": "Edit token should not set badges",
                "author_name": "Agent"
            }"#,
        )
        .dispatch();
    assert_eq!(response.status(), Status::Created);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = body["app_id"].as_str().unwrap().to_string();
    let edit_token = body["edit_token"].as_str().unwrap().to_string();

    // Try to set featured via edit token (should fail — admin only)
    let response = client
        .patch(format!("/api/v1/apps/{}?token={}", app_id, edit_token))
        .header(ContentType::JSON)
        .body(r#"{ "is_featured": true }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Forbidden);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert!(body["error"].as_str().unwrap().contains("FORBIDDEN"));
}

#[test]
fn test_edit_token_cannot_change_status() {
    let (client, _key) = setup_client();

    // Submit anonymously
    let response = client
        .post("/api/v1/apps")
        .header(ContentType::JSON)
        .body(
            r#"{
                "name": "Status Test",
                "short_description": "Test status restriction",
                "description": "Edit token should not change status",
                "author_name": "Agent"
            }"#,
        )
        .dispatch();
    assert_eq!(response.status(), Status::Created);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = body["app_id"].as_str().unwrap().to_string();
    let edit_token = body["edit_token"].as_str().unwrap().to_string();

    // Try to change status via edit token (should fail — admin only)
    let response = client
        .patch(format!("/api/v1/apps/{}?token={}", app_id, edit_token))
        .header(ContentType::JSON)
        .body(r#"{ "status": "rejected" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Forbidden);
}

#[test]
fn test_event_stream_no_auth_required() {
    let (client, _key) = setup_client();

    // SSE event stream should work without auth now
    let response = client
        .get("/api/v1/events/stream")
        .dispatch();
    // SSE returns 200 with a streaming body
    assert_eq!(response.status(), Status::Ok);
}

#[test]
fn test_update_nonexistent_app_with_token() {
    let (client, _key) = setup_client();

    // Try to update a non-existent app
    let response = client
        .patch("/api/v1/apps/nonexistent-id?token=ad_some_token")
        .header(ContentType::JSON)
        .body(r#"{ "name": "Ghost" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::NotFound);
}

#[test]
fn test_delete_nonexistent_app_with_token() {
    let (client, _key) = setup_client();

    // Try to delete a non-existent app
    let response = client
        .delete("/api/v1/apps/nonexistent-id?token=ad_some_token")
        .dispatch();
    assert_eq!(response.status(), Status::NotFound);
}

#[test]
fn test_cross_app_token_rejected() {
    let (client, _key) = setup_client();

    // Submit two apps anonymously
    let response = client
        .post("/api/v1/apps")
        .header(ContentType::JSON)
        .body(
            r#"{
                "name": "App One",
                "short_description": "First app",
                "description": "First",
                "author_name": "Agent"
            }"#,
        )
        .dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let _app1_id = body["app_id"].as_str().unwrap().to_string();
    let app1_token = body["edit_token"].as_str().unwrap().to_string();

    let response = client
        .post("/api/v1/apps")
        .header(ContentType::JSON)
        .body(
            r#"{
                "name": "App Two",
                "short_description": "Second app",
                "description": "Second",
                "author_name": "Agent"
            }"#,
        )
        .dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app2_id = body["app_id"].as_str().unwrap().to_string();

    // Try to use app1's token on app2 (should fail)
    let response = client
        .patch(format!("/api/v1/apps/{}?token={}", app2_id, app1_token))
        .header(ContentType::JSON)
        .body(r#"{ "name": "Cross-Token Attack" }"#)
        .dispatch();
    assert_eq!(response.status(), Status::Forbidden);
}

// ── Pagination & Sorting ──

#[test]
fn test_list_apps_pagination() {
    let (client, key) = setup_client();

    // Submit 5 apps
    for i in 1..=5 {
        let body = serde_json::json!({
            "name": format!("Paginated App {}", i),
            "short_description": "Test",
            "description": "Test app",
            "author_name": "Tester"
        });
        client.post("/api/v1/apps")
            .header(Header::new("X-API-Key", key.clone()))
            .header(ContentType::JSON)
            .body(body.to_string())
            .dispatch();
    }

    // Page 1, 2 per page
    let response = client.get("/api/v1/apps?per_page=2&page=1").dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["total"], 5);
    assert_eq!(body["per_page"], 2);
    assert_eq!(body["page"], 1);
    assert_eq!(body["apps"].as_array().unwrap().len(), 2);

    // Page 3 (last page, 1 item)
    let response = client.get("/api/v1/apps?per_page=2&page=3").dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["apps"].as_array().unwrap().len(), 1);

    // Page 4 (beyond data, empty)
    let response = client.get("/api/v1/apps?per_page=2&page=4").dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["apps"].as_array().unwrap().len(), 0);
    assert_eq!(body["total"], 5); // total still correct
}

#[test]
fn test_list_apps_sort_by_name() {
    let (client, key) = setup_client();

    for name in &["Zebra Service", "Alpha Tool", "Middle App"] {
        let body = serde_json::json!({
            "name": name,
            "short_description": "Test",
            "description": "Test app",
            "author_name": "Tester"
        });
        client.post("/api/v1/apps")
            .header(Header::new("X-API-Key", key.clone()))
            .header(ContentType::JSON)
            .body(body.to_string())
            .dispatch();
    }

    let response = client.get("/api/v1/apps?sort=name").dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let apps = body["apps"].as_array().unwrap();
    assert_eq!(apps[0]["name"], "Alpha Tool");
    assert_eq!(apps[1]["name"], "Middle App");
    assert_eq!(apps[2]["name"], "Zebra Service");
}

#[test]
fn test_list_apps_sort_options() {
    let (client, key) = setup_client();

    for name in &["First", "Second", "Third"] {
        let body = serde_json::json!({
            "name": name,
            "short_description": "Test",
            "description": "Test app",
            "author_name": "Tester"
        });
        client.post("/api/v1/apps")
            .header(Header::new("X-API-Key", key.clone()))
            .header(ContentType::JSON)
            .body(body.to_string())
            .dispatch();
    }

    // Oldest sort returns results in created_at ASC order
    let response = client.get("/api/v1/apps?sort=oldest").dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let apps = body["apps"].as_array().unwrap();
    assert_eq!(apps.len(), 3);
    // With oldest sort, we get ASC order — first created is first in list
    // (all have same second but insertion order is preserved by rowid)
    assert_eq!(apps[0]["name"], "First");
    assert_eq!(apps[2]["name"], "Third");

    // Name sort returns alphabetical
    let response = client.get("/api/v1/apps?sort=name").dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let apps = body["apps"].as_array().unwrap();
    assert_eq!(apps[0]["name"], "First");
    assert_eq!(apps[1]["name"], "Second");
    assert_eq!(apps[2]["name"], "Third");
}

// ── Filter Tests ──

#[test]
fn test_list_apps_filter_by_category() {
    let (client, key) = setup_client();

    let body1 = serde_json::json!({
        "name": "Dev Tool",
        "short_description": "A dev tool",
        "description": "Developer tool",
        "category": "developer-tools",
        "author_name": "Tester"
    });
    let body2 = serde_json::json!({
        "name": "Monitor App",
        "short_description": "A monitor",
        "description": "Monitoring app",
        "category": "monitoring",
        "author_name": "Tester"
    });
    client.post("/api/v1/apps").header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON).body(body1.to_string()).dispatch();
    client.post("/api/v1/apps").header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON).body(body2.to_string()).dispatch();

    let response = client.get("/api/v1/apps?category=developer-tools").dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["total"], 1);
    assert_eq!(body["apps"][0]["name"], "Dev Tool");
}

#[test]
fn test_list_apps_filter_by_protocol() {
    let (client, key) = setup_client();

    let body1 = serde_json::json!({
        "name": "REST API",
        "short_description": "REST",
        "description": "REST service",
        "protocol": "rest",
        "author_name": "Tester"
    });
    let body2 = serde_json::json!({
        "name": "GraphQL API",
        "short_description": "GraphQL",
        "description": "GraphQL service",
        "protocol": "graphql",
        "author_name": "Tester"
    });
    client.post("/api/v1/apps").header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON).body(body1.to_string()).dispatch();
    client.post("/api/v1/apps").header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON).body(body2.to_string()).dispatch();

    let response = client.get("/api/v1/apps?protocol=graphql").dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["total"], 1);
    assert_eq!(body["apps"][0]["name"], "GraphQL API");
}

#[test]
fn test_list_apps_filter_status_all() {
    let (client, key) = setup_client();

    // Submit two apps (both auto-approved)
    let body1 = serde_json::json!({
        "name": "Approved App",
        "short_description": "OK",
        "description": "Approved",
        "author_name": "Admin"
    });
    client.post("/api/v1/apps").header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON).body(body1.to_string()).dispatch();

    let body2 = serde_json::json!({
        "name": "Soon Rejected App",
        "short_description": "Will be rejected",
        "description": "Will be rejected",
        "author_name": "Anon"
    });
    let response = client.post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON).body(body2.to_string()).dispatch();
    let created: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let rejected_id = created["app_id"].as_str().unwrap();

    // Reject the second app
    client.post(format!("/api/v1/apps/{}/reject", rejected_id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"reason": "Test rejection"}"#)
        .dispatch();

    // Default list shows only approved
    let response = client.get("/api/v1/apps").dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["total"], 1);

    // status=all shows both
    let response = client.get("/api/v1/apps?status=all").dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["total"], 2);

    // status=rejected shows only rejected
    let response = client.get("/api/v1/apps?status=rejected").dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["total"], 1);
    assert_eq!(body["apps"][0]["name"], "Soon Rejected App");
}

// ── Slug-Based Lookup ──

#[test]
fn test_get_app_by_slug() {
    let (client, key) = setup_client();

    let body = serde_json::json!({
        "name": "My Cool Service",
        "short_description": "Very cool",
        "description": "A cool service",
        "author_name": "Builder"
    });
    let response = client.post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(body.to_string())
        .dispatch();
    assert_eq!(response.status(), Status::Created);
    let created: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = created["app_id"].as_str().unwrap();

    // Get by ID
    let response = client.get(format!("/api/v1/apps/{}", app_id)).dispatch();
    assert_eq!(response.status(), Status::Ok);
    let by_id: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let slug = by_id["slug"].as_str().unwrap().to_string();
    assert!(slug.contains("my-cool-service"));

    // Get by slug
    let response = client.get(format!("/api/v1/apps/{}", slug)).dispatch();
    assert_eq!(response.status(), Status::Ok);
    let by_slug: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(by_slug["id"], app_id);
    assert_eq!(by_slug["name"], "My Cool Service");
}

#[test]
fn test_get_nonexistent_app() {
    let (client, _) = setup_client();
    let response = client.get("/api/v1/apps/nonexistent-slug-or-id").dispatch();
    assert_eq!(response.status(), Status::NotFound);
}

// ── My Apps ──

#[test]
fn test_list_my_apps() {
    let (client, key) = setup_client();

    // Submit 2 apps with admin key
    for name in &["My First App", "My Second App"] {
        let body = serde_json::json!({
            "name": name,
            "short_description": "Mine",
            "description": "My app",
            "author_name": "Me"
        });
        client.post("/api/v1/apps")
            .header(Header::new("X-API-Key", key.clone()))
            .header(ContentType::JSON)
            .body(body.to_string())
            .dispatch();
    }

    // Submit 1 app anonymously
    client.post("/api/v1/apps")
        .header(ContentType::JSON)
        .body(r#"{"name": "Anon App", "short_description": "X", "description": "Y", "author_name": "Z"}"#)
        .dispatch();

    // /apps/mine should only return the 2 admin-submitted apps
    let response = client.get("/api/v1/apps/mine")
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["total"], 2);
}

#[test]
fn test_list_my_apps_no_auth() {
    let (client, _) = setup_client();
    let response = client.get("/api/v1/apps/mine").dispatch();
    assert_eq!(response.status(), Status::Unauthorized);
}

// ── Review Edge Cases ──

#[test]
fn test_review_upsert() {
    let (client, key) = setup_client();

    // Submit an app
    let body = serde_json::json!({
        "name": "Reviewable App",
        "short_description": "Review me",
        "description": "App to review",
        "author_name": "Author"
    });
    let response = client.post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(body.to_string())
        .dispatch();
    let created: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = created["app_id"].as_str().unwrap();

    // Submit initial review (3 stars)
    let review = serde_json::json!({ "rating": 3, "body": "Decent" });
    client.post(format!("/api/v1/apps/{}/reviews", app_id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(review.to_string())
        .dispatch();

    // Upsert review (update to 5 stars) — ON CONFLICT DO UPDATE still returns Created
    let review2 = serde_json::json!({ "rating": 5, "body": "Actually great!" });
    let response = client.post(format!("/api/v1/apps/{}/reviews", app_id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(review2.to_string())
        .dispatch();
    assert_eq!(response.status(), Status::Created);

    // Verify only 1 review exists and rating updated
    let response = client.get(format!("/api/v1/apps/{}/reviews", app_id)).dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["total"], 1);
    assert_eq!(body["reviews"][0]["rating"], 5);
    assert_eq!(body["reviews"][0]["body"], "Actually great!");
}

#[test]
fn test_review_for_nonexistent_app() {
    let (client, key) = setup_client();
    let review = serde_json::json!({ "rating": 3, "body": "Hmm" });
    let response = client.post("/api/v1/apps/nonexistent-id/reviews")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(review.to_string())
        .dispatch();
    assert_eq!(response.status(), Status::NotFound);
}

#[test]
fn test_review_pagination() {
    let (client, key) = setup_client();

    let body = serde_json::json!({
        "name": "Multi-Review App",
        "short_description": "Many reviews",
        "description": "App with many reviews",
        "author_name": "Author"
    });
    let response = client.post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(body.to_string())
        .dispatch();
    let created: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = created["app_id"].as_str().unwrap();

    // Submit a review
    let review = serde_json::json!({ "rating": 4, "body": "Good" });
    client.post(format!("/api/v1/apps/{}/reviews", app_id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(review.to_string())
        .dispatch();

    // Verify pagination structure
    let response = client.get(format!("/api/v1/apps/{}/reviews?per_page=1", app_id)).dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["page"], 1);
    assert_eq!(body["per_page"], 1);
    assert!(body["total"].as_i64().unwrap() >= 1);
}

// ── Search Edge Cases ──

#[test]
fn test_search_with_category_filter() {
    let (client, key) = setup_client();

    let body1 = serde_json::json!({
        "name": "Search Dev Tool",
        "short_description": "Searchable dev tool",
        "description": "A developer tool that is searchable",
        "category": "developer-tools",
        "author_name": "Tester"
    });
    let body2 = serde_json::json!({
        "name": "Search Monitor",
        "short_description": "Searchable monitor",
        "description": "A monitoring tool that is searchable",
        "category": "monitoring",
        "author_name": "Tester"
    });
    client.post("/api/v1/apps").header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON).body(body1.to_string()).dispatch();
    client.post("/api/v1/apps").header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON).body(body2.to_string()).dispatch();

    // Search "searchable" filtered to developer-tools
    let response = client.get("/api/v1/apps/search?q=searchable&category=developer-tools").dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["total"], 1);
    assert_eq!(body["apps"][0]["name"], "Search Dev Tool");
}

#[test]
fn test_search_pagination() {
    let (client, key) = setup_client();

    for i in 1..=5 {
        let body = serde_json::json!({
            "name": format!("Findable App {}", i),
            "short_description": "Findable",
            "description": "This app is findable",
            "author_name": "Tester"
        });
        client.post("/api/v1/apps").header(Header::new("X-API-Key", key.clone()))
            .header(ContentType::JSON).body(body.to_string()).dispatch();
    }

    let response = client.get("/api/v1/apps/search?q=findable&per_page=2&page=1").dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["total"], 5);
    assert_eq!(body["apps"].as_array().unwrap().len(), 2);
    assert_eq!(body["page"], 1);
    assert_eq!(body["per_page"], 2);
}

#[test]
fn test_search_no_results() {
    let (client, _) = setup_client();
    let response = client.get("/api/v1/apps/search?q=zzz_nonexistent_zzz").dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["total"], 0);
    assert_eq!(body["apps"].as_array().unwrap().len(), 0);
}

#[test]
fn test_search_by_tags() {
    let (client, key) = setup_client();

    let body = serde_json::json!({
        "name": "Tag Search App",
        "short_description": "Has unique tags",
        "description": "App with tags",
        "tags": ["quantum-computing", "neural"],
        "author_name": "Tester"
    });
    client.post("/api/v1/apps").header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON).body(body.to_string()).dispatch();

    // Search by tag content
    let response = client.get("/api/v1/apps/search?q=quantum").dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["total"], 1);
    assert_eq!(body["apps"][0]["name"], "Tag Search App");
}

// ── Submission Validation ──

#[test]
fn test_submit_missing_name() {
    let (client, key) = setup_client();
    let body = serde_json::json!({
        "short_description": "Missing name",
        "description": "No name provided",
        "author_name": "Tester"
    });
    let response = client.post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(body.to_string())
        .dispatch();
    assert_eq!(response.status(), Status::UnprocessableEntity);
}

#[test]
fn test_submit_missing_description() {
    let (client, key) = setup_client();
    let body = serde_json::json!({
        "name": "No Desc App",
        "short_description": "Has short desc",
        "author_name": "Tester"
    });
    let response = client.post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(body.to_string())
        .dispatch();
    assert_eq!(response.status(), Status::UnprocessableEntity);
}

#[test]
fn test_submit_anonymous_returns_edit_token() {
    let (client, _) = setup_client();
    let body = serde_json::json!({
        "name": "Anon Submit Test",
        "short_description": "Anonymous",
        "description": "Anonymous submission",
        "author_name": "Anon"
    });
    let response = client.post("/api/v1/apps")
        .header(ContentType::JSON)
        .body(body.to_string())
        .dispatch();
    assert_eq!(response.status(), Status::Created);
    let result: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert!(result["edit_token"].is_string());
    assert!(result["edit_token"].as_str().unwrap().starts_with("ad_"));
    assert!(result["edit_url"].is_string());
    assert!(result["listing_url"].is_string());
    assert!(result["app_id"].is_string());
    // All submissions are auto-approved (no approval queue in v1)
    assert_eq!(result["status"], "approved");
}

// ── Approval Edge Cases ──

#[test]
fn test_approve_already_approved() {
    let (client, key) = setup_client();

    // Admin-submitted apps are auto-approved
    let body = serde_json::json!({
        "name": "Auto Approved",
        "short_description": "Already approved",
        "description": "Already approved via admin key",
        "author_name": "Admin"
    });
    let response = client.post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(body.to_string())
        .dispatch();
    let created: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = created["app_id"].as_str().unwrap();

    // Try to approve again
    let response = client.post(format!("/api/v1/apps/{}/approve", app_id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"note": "Approving again"}"#)
        .dispatch();
    assert_eq!(response.status(), Status::Conflict);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["error"], "ALREADY_APPROVED");
}

#[test]
fn test_reject_already_rejected() {
    let (client, key) = setup_client();

    // Submit anonymously (pending)
    let body = serde_json::json!({
        "name": "To Be Rejected",
        "short_description": "Reject me",
        "description": "Will be rejected",
        "author_name": "Anon"
    });
    let response = client.post("/api/v1/apps")
        .header(ContentType::JSON)
        .body(body.to_string())
        .dispatch();
    let created: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = created["app_id"].as_str().unwrap();

    // Reject it
    client.post(format!("/api/v1/apps/{}/reject", app_id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"reason": "Spam"}"#)
        .dispatch();

    // Try to reject again
    let response = client.post(format!("/api/v1/apps/{}/reject", app_id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"reason": "Still spam"}"#)
        .dispatch();
    assert_eq!(response.status(), Status::Conflict);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["error"], "ALREADY_REJECTED");
}

#[test]
fn test_reject_empty_reason() {
    let (client, key) = setup_client();

    let body = serde_json::json!({
        "name": "Reject Empty Reason",
        "short_description": "Test",
        "description": "Test",
        "author_name": "Anon"
    });
    let response = client.post("/api/v1/apps")
        .header(ContentType::JSON)
        .body(body.to_string())
        .dispatch();
    let created: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = created["app_id"].as_str().unwrap();

    let response = client.post(format!("/api/v1/apps/{}/reject", app_id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"reason": ""}"#)
        .dispatch();
    assert_eq!(response.status(), Status::BadRequest);
}

#[test]
fn test_approve_nonexistent_app() {
    let (client, key) = setup_client();
    let response = client.post("/api/v1/apps/nonexistent-id/approve")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"note": "test"}"#)
        .dispatch();
    assert_eq!(response.status(), Status::NotFound);
}

// ── Pending Apps List ──

#[test]
fn test_list_pending_apps_empty() {
    let (client, key) = setup_client();

    // All submissions are auto-approved, so pending list should be empty
    let body = serde_json::json!({
        "name": "Auto Approved",
        "short_description": "Will be approved",
        "description": "Auto approved app",
        "author_name": "Tester"
    });
    client.post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(body.to_string())
        .dispatch();

    // List pending (admin only) — should be empty since all auto-approved
    let response = client.get("/api/v1/apps/pending")
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["total"], 0);
}

#[test]
fn test_list_pending_no_auth() {
    let (client, _) = setup_client();
    let response = client.get("/api/v1/apps/pending").dispatch();
    assert_eq!(response.status(), Status::Unauthorized);
}

// ── Partial Update ──

#[test]
fn test_update_partial_fields() {
    let (client, key) = setup_client();

    let body = serde_json::json!({
        "name": "Original Name",
        "short_description": "Original short desc",
        "description": "Original description",
        "protocol": "rest",
        "author_name": "Author"
    });
    let response = client.post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(body.to_string())
        .dispatch();
    let created: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = created["app_id"].as_str().unwrap();

    // Update only the name
    let update = serde_json::json!({ "name": "Updated Name" });
    let response = client.patch(format!("/api/v1/apps/{}", app_id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(update.to_string())
        .dispatch();
    assert_eq!(response.status(), Status::Ok);

    // Verify name changed but other fields preserved
    let response = client.get(format!("/api/v1/apps/{}", app_id)).dispatch();
    let app: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(app["name"], "Updated Name");
    assert_eq!(app["description"], "Original description");
    assert_eq!(app["protocol"], "rest");
}

// ── System Endpoints ──

#[test]
fn test_llms_txt_endpoint() {
    let (client, _) = setup_client();
    let response = client.get("/api/v1/llms.txt").dispatch();
    assert_eq!(response.status(), Status::Ok);
    let text = response.into_string().unwrap();
    assert!(text.contains("App Directory"));
    assert!(text.contains("/api/v1"));
}

#[test]
fn test_openapi_json_endpoint() {
    let (client, _) = setup_client();
    let response = client.get("/api/v1/openapi.json").dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert!(body["openapi"].is_string());
    assert!(body["paths"].is_object());
    assert!(body["info"]["title"].is_string());
}

#[test]
fn test_root_llms_txt() {
    let (client, _) = setup_client();
    let response = client.get("/llms.txt").dispatch();
    assert_eq!(response.status(), Status::Ok);
    let text = response.into_string().unwrap();
    assert!(text.contains("App Directory"));
}

// ── Delete Cascade ──

#[test]
fn test_delete_app_cascades_reviews() {
    let (client, key) = setup_client();

    let body = serde_json::json!({
        "name": "Cascade Delete Test",
        "short_description": "Will be deleted",
        "description": "App to test cascade delete",
        "author_name": "Author"
    });
    let response = client.post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(body.to_string())
        .dispatch();
    let created: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = created["app_id"].as_str().unwrap();

    // Add a review
    let review = serde_json::json!({ "rating": 4, "body": "Soon to be gone" });
    client.post(format!("/api/v1/apps/{}/reviews", app_id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(review.to_string())
        .dispatch();

    // Verify review exists
    let response = client.get(format!("/api/v1/apps/{}/reviews", app_id)).dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(body["total"], 1);

    // Delete the app
    let response = client.delete(format!("/api/v1/apps/{}", app_id))
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);

    // App should be gone
    let response = client.get(format!("/api/v1/apps/{}", app_id)).dispatch();
    assert_eq!(response.status(), Status::NotFound);
}

#[test]
fn test_delete_app_cascades_views_and_health_checks() {
    let (client, key) = setup_client();

    let body = serde_json::json!({
        "name": "View Cascade Test",
        "short_description": "Has views",
        "description": "App with app_views to test cascade delete",
        "author_name": "Author"
    });
    let response = client.post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(body.to_string())
        .dispatch();
    assert!(response.status() == Status::Ok || response.status() == Status::Created);
    let created: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = created["app_id"].as_str().unwrap();

    // View the app multiple times to create app_views entries
    for _ in 0..3 {
        let response = client.get(format!("/api/v1/apps/{}", app_id))
            .header(Header::new("X-API-Key", key.clone()))
            .dispatch();
        assert_eq!(response.status(), Status::Ok);
    }

    // Delete should succeed despite app_views
    let response = client.delete(format!("/api/v1/apps/{}", app_id))
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);

    // Verify app is gone
    let response = client.get(format!("/api/v1/apps/{}", app_id)).dispatch();
    assert_eq!(response.status(), Status::NotFound);
}

// ── Deprecation Edge Cases ──

#[test]
fn test_deprecate_with_replacement() {
    let (client, key) = setup_client();

    // Create two apps
    let body1 = serde_json::json!({
        "name": "Old App",
        "short_description": "To be deprecated",
        "description": "Old app",
        "author_name": "Author"
    });
    let response = client.post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON).body(body1.to_string()).dispatch();
    let old: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let old_id = old["app_id"].as_str().unwrap().to_string();

    let body2 = serde_json::json!({
        "name": "New App",
        "short_description": "Replacement",
        "description": "New app",
        "author_name": "Author"
    });
    let response = client.post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON).body(body2.to_string()).dispatch();
    let new: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let new_id = new["app_id"].as_str().unwrap().to_string();

    // Deprecate old with replacement
    let dep = serde_json::json!({
        "reason": "Superseded",
        "replacement_app_id": new_id,
        "sunset_at": "2026-06-01T00:00:00Z"
    });
    let response = client.post(format!("/api/v1/apps/{}/deprecate", old_id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON).body(dep.to_string()).dispatch();
    assert_eq!(response.status(), Status::Ok);

    // Verify deprecation fields
    let response = client.get(format!("/api/v1/apps/{}", old_id)).dispatch();
    let app: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert_eq!(app["status"], "deprecated");
    assert_eq!(app["deprecated_reason"], "Superseded");
    assert_eq!(app["replacement_app_id"], new_id);
    assert!(app["sunset_at"].is_string());
}

#[test]
fn test_deprecate_self_reference() {
    let (client, key) = setup_client();

    let body = serde_json::json!({
        "name": "Self Ref App",
        "short_description": "Test",
        "description": "Test",
        "author_name": "Author"
    });
    let response = client.post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON).body(body.to_string()).dispatch();
    let created: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = created["app_id"].as_str().unwrap();

    // Try to deprecate with self as replacement
    let dep = serde_json::json!({
        "reason": "Self replace",
        "replacement_app_id": app_id
    });
    let response = client.post(format!("/api/v1/apps/{}/deprecate", app_id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON).body(dep.to_string()).dispatch();
    assert_eq!(response.status(), Status::BadRequest);
}

#[test]
fn test_undeprecate_non_deprecated() {
    let (client, key) = setup_client();

    let body = serde_json::json!({
        "name": "Not Deprecated",
        "short_description": "Test",
        "description": "Not deprecated",
        "author_name": "Author"
    });
    let response = client.post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON).body(body.to_string()).dispatch();
    let created: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = created["app_id"].as_str().unwrap();

    let response = client.post(format!("/api/v1/apps/{}/undeprecate", app_id))
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Conflict);
}

// ── Key Management Edge Cases ──

#[test]
fn test_delete_nonexistent_key() {
    let (client, key) = setup_client();
    let response = client.delete("/api/v1/keys/nonexistent-key-id")
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::NotFound);
}

#[test]
fn test_create_key_returns_key_info() {
    let (client, key) = setup_client();
    let body = serde_json::json!({ "name": "My Test Key" });
    let response = client.post("/api/v1/keys")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(body.to_string())
        .dispatch();
    assert_eq!(response.status(), Status::Created);
    let result: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    assert!(result["api_key"].is_string());
    assert!(result["message"].is_string());
}

// ── Webhook Update ──

#[test]
fn test_webhook_update() {
    let (client, key) = setup_client();

    // Create a webhook
    let wh = serde_json::json!({
        "url": "https://example.com/webhook",
        "events": ["app.submitted"],
        "active": true
    });
    let response = client.post("/api/v1/webhooks")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(wh.to_string())
        .dispatch();
    assert_eq!(response.status(), Status::Created);
    let created: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let wh_id = created["id"].as_str().unwrap();

    // Update it
    let update = serde_json::json!({
        "url": "https://example.com/webhook-v2",
        "events": ["app.submitted", "app.approved"],
        "active": false
    });
    let response = client.patch(format!("/api/v1/webhooks/{}", wh_id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(update.to_string())
        .dispatch();
    assert_eq!(response.status(), Status::Ok);

    // Verify update
    let response = client.get("/api/v1/webhooks")
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    let body: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let webhooks = body["webhooks"].as_array().unwrap();
    let updated = webhooks.iter().find(|w| w["id"] == wh_id).unwrap();
    assert_eq!(updated["url"], "https://example.com/webhook-v2");
    assert_eq!(updated["active"], false);
}

// ── CORS Preflight ──

#[test]
fn test_cors_preflight() {
    let (client, _) = setup_client();
    let response = client.options("/api/v1/apps").dispatch();
    assert_eq!(response.status(), Status::NoContent);
}

// ── Health Check on App Without URL ──

#[test]
fn test_health_check_app_without_homepage() {
    let (client, key) = setup_client();

    // Submit an app with api_url for health checking
    let body = serde_json::json!({
        "name": "Headless App",
        "short_description": "No homepage",
        "description": "App without homepage URL",
        "api_url": "https://api.example.com/health",
        "author_name": "Author"
    });
    let response = client.post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(body.to_string())
        .dispatch();
    let created: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = created["app_id"].as_str().unwrap();

    // Health status should be included in app data
    let response = client.get(format!("/api/v1/apps/{}", app_id)).dispatch();
    assert_eq!(response.status(), Status::Ok);
    let app: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    // last_health_status should be null initially
    assert!(app["last_health_status"].is_null());
}

// ── App Response Field Completeness ──

#[test]
fn test_app_response_includes_all_fields() {
    let (client, key) = setup_client();

    let body = serde_json::json!({
        "name": "Complete Fields App",
        "short_description": "All fields",
        "description": "App with all fields",
        "homepage_url": "https://example.com",
        "api_url": "https://api.example.com",
        "api_spec_url": "https://api.example.com/spec",
        "protocol": "rest",
        "category": "developer-tools",
        "tags": ["test", "complete"],
        "author_name": "Full Author",
        "author_url": "https://author.example.com"
    });
    let response = client.post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(body.to_string())
        .dispatch();
    let created: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let app_id = created["app_id"].as_str().unwrap();

    let response = client.get(format!("/api/v1/apps/{}", app_id)).dispatch();
    let app: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();

    // Verify all expected fields are present
    assert!(app["id"].is_string());
    assert!(app["name"].is_string());
    assert!(app["slug"].is_string());
    assert!(app["short_description"].is_string());
    assert!(app["description"].is_string());
    assert_eq!(app["homepage_url"], "https://example.com");
    assert_eq!(app["api_url"], "https://api.example.com");
    assert_eq!(app["api_spec_url"], "https://api.example.com/spec");
    assert_eq!(app["protocol"], "rest");
    assert_eq!(app["category"], "developer-tools");
    assert!(app["tags"].is_array());
    assert_eq!(app["tags"].as_array().unwrap().len(), 2);
    assert_eq!(app["author_name"], "Full Author");
    assert_eq!(app["author_url"], "https://author.example.com");
    assert!(app["created_at"].is_string());
    assert!(app["updated_at"].is_string());
    assert_eq!(app["is_featured"], false);
    assert_eq!(app["is_verified"], false);
    assert_eq!(app["status"], "approved");
    assert_eq!(app["avg_rating"], 0.0);
    assert_eq!(app["review_count"], 0);
}

// ── Well-Known Skills Discovery ──

#[test]
fn test_skills_index_json() {
    let (client, _) = setup_client();
    let resp = client.get("/.well-known/skills/index.json").dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: serde_json::Value = resp.into_json().unwrap();
    let skills = body["skills"].as_array().unwrap();
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0]["name"], "app-directory");
    assert!(skills[0]["description"].as_str().unwrap().contains("agent"));
    let files = skills[0]["files"].as_array().unwrap();
    assert!(files.contains(&serde_json::json!("SKILL.md")));
}

#[test]
fn test_skills_skill_md() {
    let (client, _) = setup_client();
    let resp = client.get("/.well-known/skills/app-directory/SKILL.md").dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body = resp.into_string().unwrap();
    assert!(body.starts_with("---"), "Missing YAML frontmatter");
    assert!(body.contains("name: app-directory"), "Missing skill name");
    assert!(body.contains("## Quick Start"), "Missing Quick Start");
    assert!(body.contains("## Auth Model"), "Missing Auth Model");
    assert!(body.contains("Categories"), "Missing categories section");
    assert!(body.contains("Reviews"), "Missing reviews section");
}
