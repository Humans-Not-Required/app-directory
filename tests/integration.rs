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
    assert!(body.contains("# App Directory"), "Missing title");
    assert!(body.contains("## Quick Start"), "Missing Quick Start");
    assert!(body.contains("## Auth Model"), "Missing Auth Model");
    assert!(body.contains("Categories"), "Missing categories section");
    assert!(body.contains("Reviews"), "Missing reviews section");
}

#[test]
fn test_skill_md_root() {
    let (client, _) = setup_client();
    let resp = client.get("/SKILL.md").dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body = resp.into_string().unwrap();
    assert!(body.contains("# App Directory"));
    // Verify llms.txt serves same content
    let llms_resp = client.get("/llms.txt").dispatch();
    assert_eq!(llms_resp.status(), Status::Ok);
    let llms_body = llms_resp.into_string().unwrap();
    assert_eq!(body, llms_body, "llms.txt should alias SKILL.md");
}

// ── Anonymous Review Bug Fix ──

#[test]
fn test_anonymous_review_submit() {
    let (client, key) = setup_client();

    // Create an app
    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Anon Review App","short_description":"Test","description":"Testing anonymous reviews","author_name":"Tester"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let app_id = body["app_id"].as_str().unwrap();

    // Submit review WITHOUT auth (anonymous)
    let resp = client
        .post(format!("/api/v1/apps/{}/reviews", app_id))
        .header(ContentType::JSON)
        .body(r#"{"rating":5,"title":"Great!","body":"Works perfectly","reviewer_name":"Agent42"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Created, "Anonymous review should succeed");
    let body: Value = resp.into_json().unwrap();
    assert!(body["id"].is_string());

    // Verify review appears in listing
    let resp = client
        .get(format!("/api/v1/apps/{}/reviews", app_id))
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    assert_eq!(body["total"], 1);
    assert_eq!(body["reviews"][0]["rating"], 5);
    assert_eq!(body["reviews"][0]["reviewer_name"], "Agent42");
}

#[test]
fn test_multiple_anonymous_reviews_allowed() {
    let (client, key) = setup_client();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Multi Review App","short_description":"Test","description":"Testing multiple anonymous reviews","author_name":"Tester"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let app_id = body["app_id"].as_str().unwrap();

    // Submit 3 anonymous reviews
    for rating in [5, 3, 4] {
        let resp = client
            .post(format!("/api/v1/apps/{}/reviews", app_id))
            .header(ContentType::JSON)
            .body(format!(r#"{{"rating":{},"reviewer_name":"anon"}}"#, rating))
            .dispatch();
        assert_eq!(resp.status(), Status::Created);
    }

    // All 3 should be present
    let resp = client
        .get(format!("/api/v1/apps/{}/reviews", app_id))
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    assert_eq!(body["total"], 3);

    // avg_rating should reflect all 3
    let resp = client
        .get(format!("/api/v1/apps/{}", app_id))
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    assert_eq!(body["review_count"], 3);
    // (5+3+4)/3 = 4.0
    assert!((body["avg_rating"].as_f64().unwrap() - 4.0).abs() < 0.01);
}

#[test]
fn test_anonymous_review_default_reviewer_name() {
    let (client, key) = setup_client();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Default Name App","short_description":"Test","description":"Test default reviewer name","author_name":"Tester"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let app_id = body["app_id"].as_str().unwrap();

    // Submit without reviewer_name
    let resp = client
        .post(format!("/api/v1/apps/{}/reviews", app_id))
        .header(ContentType::JSON)
        .body(r#"{"rating":4}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Created);

    // Check reviewer_name defaults to "anonymous"
    let resp = client
        .get(format!("/api/v1/apps/{}/reviews", app_id))
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    assert_eq!(body["reviews"][0]["reviewer_name"], "anonymous");
}

#[test]
fn test_authenticated_review_upsert_still_works() {
    let (client, key) = setup_client();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Upsert Test App","short_description":"Test","description":"Test authenticated upsert","author_name":"Tester"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let app_id = body["app_id"].as_str().unwrap();

    // Submit review with auth
    let resp = client
        .post(format!("/api/v1/apps/{}/reviews", app_id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"rating":3,"title":"Okay","reviewer_name":"TestUser"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Created);

    // Submit again with same key — should update, not create duplicate
    let resp = client
        .post(format!("/api/v1/apps/{}/reviews", app_id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"rating":5,"title":"Actually great","reviewer_name":"TestUser"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Created);

    // Should still be only 1 review
    let resp = client
        .get(format!("/api/v1/apps/{}/reviews", app_id))
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    assert_eq!(body["total"], 1);
    assert_eq!(body["reviews"][0]["rating"], 5);
    assert_eq!(body["reviews"][0]["title"], "Actually great");
}

#[test]
fn test_review_nonexistent_app_anonymous() {
    let (client, _) = setup_client();

    let resp = client
        .post("/api/v1/apps/nonexistent-id/reviews")
        .header(ContentType::JSON)
        .body(r#"{"rating":5}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::NotFound);
}

// ── Approval state machine edge cases ──

#[test]
fn test_approve_rejected_app() {
    let (client, key) = setup_client();

    // Submit app
    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Reject Then Approve","short_description":"Test","description":"State machine test","author_name":"Test"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();

    // Reject first
    let resp = client
        .post(format!("/api/v1/apps/{}/reject", id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"reason":"Not ready yet"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // Then approve — should work (rejected→approved is valid)
    let resp = client
        .post(format!("/api/v1/apps/{}/approve", id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // Verify status is now approved
    let resp = client.get(format!("/api/v1/apps/{}", id)).dispatch();
    let body: Value = resp.into_json().unwrap();
    assert_eq!(body["status"], "approved");
}

#[test]
fn test_deprecate_requires_reason() {
    let (client, key) = setup_client();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Deprecate Test","short_description":"Test","description":"Needs reason","author_name":"Test"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();

    // Approve first
    client
        .post(format!("/api/v1/apps/{}/approve", id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{}"#)
        .dispatch();

    // Try deprecate with empty reason — should fail
    let resp = client
        .post(format!("/api/v1/apps/{}/deprecate", id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"reason":""}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::BadRequest);
}

#[test]
fn test_deprecate_then_approve_blocked() {
    let (client, key) = setup_client();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Dep Then Approve","short_description":"Test","description":"Blocked test","author_name":"Test"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();

    // Approve first
    client
        .post(format!("/api/v1/apps/{}/approve", id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{}"#)
        .dispatch();

    // Deprecate
    client
        .post(format!("/api/v1/apps/{}/deprecate", id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"reason":"Replaced by v2"}"#)
        .dispatch();

    // Try approve deprecated — should fail
    let resp = client
        .post(format!("/api/v1/apps/{}/approve", id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Conflict);
}

// ── Search edge cases ──

#[test]
fn test_search_empty_query() {
    let (client, _) = setup_client();
    let resp = client.get("/api/v1/apps/search?q=").dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: Value = resp.into_json().unwrap();
    // Empty query should return empty results (no crash)
    assert!(body["apps"].is_array());
}

#[test]
fn test_search_special_characters() {
    let (client, key) = setup_client();

    // Submit app with special chars in name
    client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"C++ Parser & Linter (v2.0)","short_description":"Parses C++","description":"A C++ parser","author_name":"Test"}"#)
        .dispatch();

    // Search with special chars — should not crash
    let resp = client.get("/api/v1/apps/search?q=C%2B%2B").dispatch();
    assert_eq!(resp.status(), Status::Ok);
}

// ── Categories with counts ──

#[test]
fn test_categories_reflect_app_counts() {
    let (client, key) = setup_client();

    // Submit 2 apps in developer-tools
    for i in 0..2 {
        client
            .post("/api/v1/apps")
            .header(Header::new("X-API-Key", key.clone()))
            .header(ContentType::JSON)
            .body(format!(r#"{{"name":"DevTool {}","short_description":"A tool","description":"Development tool","category":"developer-tools","author_name":"Test"}}"#, i))
            .dispatch();
    }

    // Submit 1 app in monitoring
    client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Monitor App","short_description":"Monitoring","description":"A monitor","category":"monitoring","author_name":"Test"}"#)
        .dispatch();

    let resp = client.get("/api/v1/categories").dispatch();
    let body: Value = resp.into_json().unwrap();
    let cats = body["categories"].as_array().unwrap();

    // Find developer-tools count
    let dev = cats.iter().find(|c| c["slug"] == "developer-tools");
    if let Some(d) = dev {
        assert!(d["count"].as_i64().unwrap() >= 2, "developer-tools should have at least 2 apps");
    }
}

// ── Key management edge cases ──

#[test]
fn test_revoke_key_then_use() {
    let (client, admin_key, _db_path) = setup_client_with_path();

    // Create a new key
    let resp = client
        .post("/api/v1/keys")
        .header(Header::new("X-API-Key", admin_key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"ephemeral","admin":false}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Created);
    let body: Value = resp.into_json().unwrap();
    let new_key = body["api_key"].as_str().unwrap().to_string();
    assert!(body["message"].is_string());

    // List keys to find the ID
    let resp = client
        .get("/api/v1/keys")
        .header(Header::new("X-API-Key", admin_key.clone()))
        .dispatch();
    let keys_body: Value = resp.into_json().unwrap();
    let keys = keys_body["keys"].as_array().unwrap();
    let ephemeral_key = keys.iter().find(|k| k["name"] == "ephemeral").unwrap();
    let key_id = ephemeral_key["id"].as_str().unwrap();

    // Revoke the key
    let resp = client
        .delete(format!("/api/v1/keys/{}", key_id))
        .header(Header::new("X-API-Key", admin_key.clone()))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // Revoked key should now fail on admin endpoints
    let resp = client
        .get("/api/v1/keys")
        .header(Header::new("X-API-Key", new_key.clone()))
        .dispatch();
    assert_eq!(resp.status(), Status::Unauthorized);
}

// ── App creation validation ──

#[test]
fn test_submit_app_invalid_category() {
    let (client, key) = setup_client();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Bad Category App","short_description":"Test","description":"Test app","category":"not-a-real-category","author_name":"Test"}"#)
        .dispatch();
    // Should either reject or accept with the provided category
    // The API may be permissive — just verify no crash
    assert!(resp.status() == Status::Created || resp.status() == Status::BadRequest);
}

#[test]
fn test_submit_app_with_all_fields() {
    let (client, key) = setup_client();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{
            "name": "Full Feature App",
            "short_description": "Complete submission",
            "description": "An app with every field populated",
            "api_url": "https://api.example.com/v1",
            "homepage_url": "https://example.com",
            "api_spec_url": "https://docs.example.com/openapi.json",
            "protocol": "rest",
            "category": "developer-tools",
            "tags": ["test", "full", "complete"],
            "author_name": "Test Author"
        }"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Created);
    let body: Value = resp.into_json().unwrap();

    // Verify all fields came back
    let id = body["app_id"].as_str().unwrap();
    let resp = client.get(format!("/api/v1/apps/{}", id)).dispatch();
    let app: Value = resp.into_json().unwrap();

    assert_eq!(app["name"], "Full Feature App");
    assert_eq!(app["api_url"], "https://api.example.com/v1");
    assert_eq!(app["homepage_url"], "https://example.com");
    assert_eq!(app["protocol"], "rest");
    assert_eq!(app["category"], "developer-tools");
    assert!(app["tags"].as_array().unwrap().len() >= 3);
    assert_eq!(app["author_name"], "Test Author");
}

// ── Review rating validation ──

#[test]
fn test_review_invalid_rating_too_high() {
    let (client, key) = setup_client();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Rating Test App","short_description":"Test","description":"For rating validation","author_name":"Test"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();

    // Rating > 5 should be rejected
    let resp = client
        .post(format!("/api/v1/apps/{}/reviews", id))
        .header(ContentType::JSON)
        .body(r#"{"rating":6}"#)
        .dispatch();
    assert!(resp.status() == Status::BadRequest || resp.status() == Status::UnprocessableEntity,
        "Rating 6 should be rejected, got {:?}", resp.status());
}

#[test]
fn test_review_invalid_rating_zero() {
    let (client, key) = setup_client();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Zero Rating App","short_description":"Test","description":"For rating validation","author_name":"Test"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();

    // Rating 0 should be rejected (valid range is 1-5)
    let resp = client
        .post(format!("/api/v1/apps/{}/reviews", id))
        .header(ContentType::JSON)
        .body(r#"{"rating":0}"#)
        .dispatch();
    assert!(resp.status() == Status::BadRequest || resp.status() == Status::UnprocessableEntity,
        "Rating 0 should be rejected, got {:?}", resp.status());
}

// ── Review aggregate statistics ──

#[test]
fn test_review_average_rating() {
    let (client, key) = setup_client();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Avg Rating App","short_description":"Test","description":"For avg rating","author_name":"Test"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();

    // Submit 3 anonymous reviews with different ratings
    for rating in [3, 4, 5] {
        client
            .post(format!("/api/v1/apps/{}/reviews", id))
            .header(ContentType::JSON)
            .body(format!(r#"{{"rating":{}}}"#, rating))
            .dispatch();
    }

    // Check app's aggregate rating
    let resp = client.get(format!("/api/v1/apps/{}", id)).dispatch();
    let app: Value = resp.into_json().unwrap();

    assert_eq!(app["review_count"], 3);
    let avg = app["avg_rating"].as_f64().unwrap();
    assert!((avg - 4.0).abs() < 0.01, "Average should be ~4.0, got {}", avg);
}

// ── Slug uniqueness ──

#[test]
fn test_slug_uniqueness_collision() {
    let (client, key) = setup_client();

    // Submit two apps with the same name — slugs should be unique
    let resp1 = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Duplicate Name","short_description":"First","description":"First app with this name","author_name":"Test"}"#)
        .dispatch();
    assert_eq!(resp1.status(), Status::Created);
    let body1: Value = resp1.into_json().unwrap();
    let slug1 = body1["slug"].as_str().unwrap().to_string();

    let resp2 = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Duplicate Name","short_description":"Second","description":"Second app with same name","author_name":"Test"}"#)
        .dispatch();
    assert_eq!(resp2.status(), Status::Created);
    let body2: Value = resp2.into_json().unwrap();
    let slug2 = body2["slug"].as_str().unwrap().to_string();

    // Slugs should be different (one should have a suffix)
    assert_ne!(slug1, slug2, "Duplicate names should get different slugs");

    // Both should be accessible by slug
    let resp = client.get(format!("/api/v1/apps/{}", slug1)).dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let resp = client.get(format!("/api/v1/apps/{}", slug2)).dispatch();
    assert_eq!(resp.status(), Status::Ok);
}

// ── App update edge cases ──

#[test]
fn test_update_app_name_preserves_slug() {
    let (client, key) = setup_client();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Original Name","short_description":"Test","description":"Slug stability test","author_name":"Test"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();
    let original_slug = body["slug"].as_str().unwrap().to_string();

    // Update the name
    let resp = client
        .patch(format!("/api/v1/apps/{}", id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Updated Name"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // Slug should be STABLE (not change when name changes) — for URL reliability
    let resp = client.get(format!("/api/v1/apps/{}", id)).dispatch();
    let app: Value = resp.into_json().unwrap();
    assert_eq!(app["name"], "Updated Name");
    assert_eq!(app["slug"], original_slug, "Slug should remain stable after name change");
}

// ── Health check response structure ──

#[test]
fn test_health_response_structure() {
    let (client, _) = setup_client();
    let resp = client.get("/api/v1/health").dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: Value = resp.into_json().unwrap();

    assert_eq!(body["status"], "ok");
    assert_eq!(body["service"], "app-directory");
    assert!(body["version"].is_string());
}

// ── List apps combined filters ──

#[test]
fn test_list_apps_combined_filters() {
    let (client, key) = setup_client();

    // Submit a featured app in developer-tools
    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Featured DevTool","short_description":"A featured tool","description":"Featured developer tool","category":"developer-tools","author_name":"Test"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();

    // Set as featured
    client
        .patch(format!("/api/v1/apps/{}", id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"featured":true}"#)
        .dispatch();

    // Filter: featured + category
    let resp = client.get("/api/v1/apps?featured=true&category=developer-tools").dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: Value = resp.into_json().unwrap();
    let apps = body["apps"].as_array().unwrap();
    // All returned apps should be featured AND in developer-tools
    for app in apps {
        assert_eq!(app["featured"], true);
        assert_eq!(app["category"], "developer-tools");
    }
}

// ── Webhook event filtering ──

#[test]
fn test_webhook_with_event_filter() {
    let (client, key) = setup_client();

    // Create webhook with only specific events
    let resp = client
        .post("/api/v1/webhooks")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"url":"https://hook.example.com/events","events":["app.submitted","app.approved"]}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Created);
    let body: Value = resp.into_json().unwrap();
    let events = body["events"].as_array().unwrap();
    assert_eq!(events.len(), 2);
    assert!(events.contains(&serde_json::json!("app.submitted")));
    assert!(events.contains(&serde_json::json!("app.approved")));
}

// ── Undeprecate restores to approved ──

#[test]
fn test_undeprecate_restores_status() {
    let (client, key) = setup_client();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Undeprecate Test","short_description":"Test","description":"Undeprecation test","author_name":"Test"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();

    // Approve, then deprecate
    client
        .post(format!("/api/v1/apps/{}/approve", id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{}"#)
        .dispatch();

    client
        .post(format!("/api/v1/apps/{}/deprecate", id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"reason":"Testing deprecation"}"#)
        .dispatch();

    // Verify deprecated
    let resp = client.get(format!("/api/v1/apps/{}", id)).dispatch();
    let app: Value = resp.into_json().unwrap();
    assert_eq!(app["status"], "deprecated");
    assert!(app["deprecated_reason"].is_string());

    // Undeprecate
    let resp = client
        .post(format!("/api/v1/apps/{}/undeprecate", id))
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // Verify status restored and deprecation fields cleared
    let resp = client.get(format!("/api/v1/apps/{}", id)).dispatch();
    let app: Value = resp.into_json().unwrap();
    assert_eq!(app["status"], "approved");
    assert!(app["deprecated_reason"].is_null() || app["deprecated_reason"].as_str().unwrap_or("").is_empty());
}

// ── Delete cascade verification ──

#[test]
fn test_delete_app_cascades_webhooks_events() {
    let (client, key) = setup_client();

    // Submit app
    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Cascade Webhook Test","short_description":"Test","description":"For cascade test","author_name":"Test"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();

    // Add reviews
    client
        .post(format!("/api/v1/apps/{}/reviews", id))
        .header(ContentType::JSON)
        .body(r#"{"rating":5,"title":"Great"}"#)
        .dispatch();

    // Delete app
    let resp = client
        .delete(format!("/api/v1/apps/{}", id))
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // App should be gone
    let resp = client.get(format!("/api/v1/apps/{}", id)).dispatch();
    assert_eq!(resp.status(), Status::NotFound);

    // Reviews endpoint for deleted app should return 404 or empty
    let resp = client.get(format!("/api/v1/apps/{}/reviews", id)).dispatch();
    if resp.status() == Status::Ok {
        // If 200, reviews should be empty
        let body: Value = resp.into_json().unwrap();
        assert_eq!(body["total"], 0);
    } else {
        assert_eq!(resp.status(), Status::NotFound);
    }
}

// ── Pagination edge cases ──

#[test]
fn test_list_apps_page_beyond_data() {
    let (client, _) = setup_client();

    // Request page 999 with no data
    let resp = client.get("/api/v1/apps?page=999&per_page=10").dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: Value = resp.into_json().unwrap();
    assert_eq!(body["apps"].as_array().unwrap().len(), 0);
}

#[test]
fn test_list_apps_per_page_boundary() {
    let (client, key) = setup_client();

    // Submit 3 apps
    for i in 0..3 {
        client
            .post("/api/v1/apps")
            .header(Header::new("X-API-Key", key.clone()))
            .header(ContentType::JSON)
            .body(format!(r#"{{"name":"Paginate App {}","short_description":"Test","description":"Pagination test","author_name":"Test"}}"#, i))
            .dispatch();
    }

    // per_page=2: page 1 should have 2, page 2 should have 1
    let resp = client.get("/api/v1/apps?per_page=2&page=1").dispatch();
    let body: Value = resp.into_json().unwrap();
    assert_eq!(body["apps"].as_array().unwrap().len(), 2);

    let resp = client.get("/api/v1/apps?per_page=2&page=2").dispatch();
    let body: Value = resp.into_json().unwrap();
    assert_eq!(body["apps"].as_array().unwrap().len(), 1);
}

// ═══════════════════════════════════════════════════════════════
// New integration tests: Stats, Health, Lifecycle, Edge Cases
// ═══════════════════════════════════════════════════════════════

// ── Stats: view count increments on each GET ──

#[test]
fn test_stats_view_count_increments() {
    let (client, key) = setup_client();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Stats Counter","short_description":"Test","description":"Test stats","author_name":"Test"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();

    // Check stats initially (0 views since submit doesn't count)
    let resp = client.get(format!("/api/v1/apps/{}/stats", id)).dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let stats: Value = resp.into_json().unwrap();
    let initial = stats["total_views"].as_i64().unwrap();

    // GET the app 3 times to generate views
    for _ in 0..3 {
        client.get(format!("/api/v1/apps/{}", id)).dispatch();
    }

    let resp = client.get(format!("/api/v1/apps/{}/stats", id)).dispatch();
    let stats: Value = resp.into_json().unwrap();
    // Should have at least 3 more views (stats endpoint itself may or may not count)
    assert!(stats["total_views"].as_i64().unwrap() >= initial + 3);
}

// ── Stats: via slug lookup ──

#[test]
fn test_stats_via_slug() {
    let (client, key) = setup_client();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Slug Stats App","short_description":"Test","description":"Test stats via slug","author_name":"Test"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let slug = body["slug"].as_str().unwrap();

    // Stats by slug should work
    let resp = client.get(format!("/api/v1/apps/{}/stats", slug)).dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let stats: Value = resp.into_json().unwrap();
    assert!(stats["app_id"].is_string());
    assert!(stats["total_views"].is_number());
    assert!(stats["views_24h"].is_number());
    assert!(stats["views_7d"].is_number());
    assert!(stats["views_30d"].is_number());
    assert!(stats["unique_viewers"].is_number());
}

// ── Stats: nonexistent app returns 404 ──

#[test]
fn test_stats_nonexistent_app() {
    let (client, _) = setup_client();

    let resp = client
        .get("/api/v1/apps/00000000-0000-0000-0000-000000000000/stats")
        .dispatch();
    assert_eq!(resp.status(), Status::NotFound);
}

// ── Trending: empty result when no views ──

#[test]
fn test_trending_empty() {
    let (client, key) = setup_client();

    // Submit app but don't view it
    client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"No Views App","short_description":"Test","description":"No views","author_name":"Test"}"#)
        .dispatch();

    let resp = client.get("/api/v1/apps/trending").dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: Value = resp.into_json().unwrap();
    assert!(body["trending"].as_array().unwrap().is_empty());
    assert_eq!(body["period_days"], 7);
}

// ── Trending: custom days and limit parameters ──

#[test]
fn test_trending_custom_params() {
    let (client, key) = setup_client();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Trending Test","short_description":"Test","description":"Test trending","author_name":"Test"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();

    // Generate views
    for _ in 0..3 {
        client.get(format!("/api/v1/apps/{}", id)).dispatch();
    }

    // Custom days=1, limit=5
    let resp = client.get("/api/v1/apps/trending?days=1&limit=5").dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: Value = resp.into_json().unwrap();
    assert_eq!(body["period_days"], 1);
    let trending = body["trending"].as_array().unwrap();
    assert!(trending.len() <= 5);
    if !trending.is_empty() {
        assert!(trending[0]["view_count"].as_i64().unwrap() > 0);
        assert!(trending[0]["views_per_day"].as_f64().unwrap() > 0.0);
        assert!(trending[0]["unique_viewers"].is_number());
    }

    // days clamped to 90 max
    let resp = client.get("/api/v1/apps/trending?days=200").dispatch();
    let body: Value = resp.into_json().unwrap();
    assert_eq!(body["period_days"], 90);
}

// ── Trending: response structure fields ──

#[test]
fn test_trending_response_fields() {
    let (client, key) = setup_client();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Trending Fields","short_description":"Test","description":"Test trending fields","author_name":"Test","protocol":"mcp","category":"ai-ml","tags":["test"]}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();

    // Generate views
    client.get(format!("/api/v1/apps/{}", id)).dispatch();

    let resp = client.get("/api/v1/apps/trending?days=7&limit=1").dispatch();
    let body: Value = resp.into_json().unwrap();
    let trending = body["trending"].as_array().unwrap();
    if !trending.is_empty() {
        let app = &trending[0];
        assert!(app["id"].is_string());
        assert!(app["name"].is_string());
        assert!(app["slug"].is_string());
        assert!(app["short_description"].is_string());
        assert!(app["protocol"].is_string());
        assert!(app["category"].is_string());
        assert!(app["tags"].is_array());
        assert!(app["view_count"].is_number());
        assert!(app["unique_viewers"].is_number());
        assert!(app["views_per_day"].is_number());
    }
}

// ── Health history: pagination ──

#[test]
fn test_health_history_pagination() {
    let (client, key) = setup_client();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Health Paginate","short_description":"Test","description":"Test health history pagination","author_name":"Test","api_url":"https://httpbin.org/status/200"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();

    // Check health history with pagination params (no auth needed)
    let resp = client
        .get(format!("/api/v1/apps/{}/health?page=1&per_page=5", id))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: Value = resp.into_json().unwrap();
    assert!(body["checks"].is_array());
    // uptime_pct is null when no checks have been performed yet
    assert!(body["uptime_pct"].is_number() || body["uptime_pct"].is_null());
}

// ── Health check: non-admin rejected ──

#[test]
fn test_health_check_non_admin_rejected() {
    let (client, key, path) = setup_client_with_path();

    // Create a non-admin key
    let conn = rusqlite::Connection::open(&path).unwrap();
    let user_key = app_directory::auth::create_api_key(&conn, "user", false, Some(10000));
    drop(conn);

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Health Reject Test","short_description":"Test","description":"Test non-admin health check","author_name":"Test","api_url":"https://httpbin.org/status/200"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();

    // Non-admin trigger health check → 403
    let resp = client
        .post(format!("/api/v1/apps/{}/health-check", id))
        .header(Header::new("X-API-Key", user_key))
        .dispatch();
    assert_eq!(resp.status(), Status::Forbidden);
}

// ── Health summary: structure ──

#[test]
fn test_health_summary_structure() {
    let (client, key) = setup_client();

    // Submit an app so summary has data to report
    client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Summary App","short_description":"Test","description":"Health summary test","author_name":"Test"}"#)
        .dispatch();

    let resp = client
        .get("/api/v1/apps/health/summary")
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: Value = resp.into_json().unwrap();
    assert!(body["total_approved_apps"].is_number());
    assert!(body["monitored"].is_number());
    assert!(body["healthy"].is_number());
    assert!(body["unhealthy"].is_number());
    assert!(body["unreachable"].is_number());
    assert!(body["issues"].is_array());
}

// ── Schedule endpoint response ──

#[test]
fn test_schedule_endpoint_structure() {
    let (client, key) = setup_client();

    // Admin key required
    let resp = client
        .get("/api/v1/health-check/schedule")
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: Value = resp.into_json().unwrap();
    assert!(body["interval_seconds"].is_number());
    assert!(body["enabled"].is_boolean());
    assert!(body["description"].is_string());
    assert!(body["config_var"].is_string());
}

// ── Schedule endpoint: requires admin ──

#[test]
fn test_schedule_endpoint_requires_admin() {
    let (client, _, path) = setup_client_with_path();

    let conn = rusqlite::Connection::open(&path).unwrap();
    let user_key = app_directory::auth::create_api_key(&conn, "user", false, Some(10000));
    drop(conn);

    let resp = client
        .get("/api/v1/health-check/schedule")
        .header(Header::new("X-API-Key", user_key))
        .dispatch();
    assert_eq!(resp.status(), Status::Forbidden);
}

// ── Review: multiple keys on same app ──

#[test]
fn test_review_multiple_authenticated_keys() {
    let (client, key, path) = setup_client_with_path();

    // Submit app
    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Multi Review App","short_description":"Test","description":"Multi reviewer test","author_name":"Test"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();

    // Create a second key
    let conn = rusqlite::Connection::open(&path).unwrap();
    let key2 = app_directory::auth::create_api_key(&conn, "reviewer2", false, Some(10000));
    drop(conn);

    // Review with key 1
    let resp = client
        .post(format!("/api/v1/apps/{}/reviews", id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"rating":5,"title":"Excellent","comment":"Great work"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Created);

    // Review with key 2
    let resp = client
        .post(format!("/api/v1/apps/{}/reviews", id))
        .header(Header::new("X-API-Key", key2))
        .header(ContentType::JSON)
        .body(r#"{"rating":3,"title":"Decent","comment":"OK but could be better"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Created);

    // Should have 2 reviews, avg 4.0
    let resp = client
        .get(format!("/api/v1/apps/{}/reviews", id))
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    assert_eq!(body["total"], 2);

    // Check aggregate on app
    let resp = client.get(format!("/api/v1/apps/{}", id)).dispatch();
    let app: Value = resp.into_json().unwrap();
    assert_eq!(app["review_count"], 2);
    let avg = app["avg_rating"].as_f64().unwrap();
    assert!((avg - 4.0).abs() < 0.01);
}

// ── Review: anonymous + authenticated on same app ──

#[test]
fn test_review_mixed_anonymous_and_authenticated() {
    let (client, key) = setup_client();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Mixed Review App","short_description":"Test","description":"Mixed review test","author_name":"Test"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();

    // Anonymous review (no auth)
    let resp = client
        .post(format!("/api/v1/apps/{}/reviews", id))
        .header(ContentType::JSON)
        .body(r#"{"rating":4,"title":"Anonymous review","reviewer_name":"Anonymous Agent"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Created);

    // Authenticated review
    let resp = client
        .post(format!("/api/v1/apps/{}/reviews", id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"rating":5,"title":"Admin review"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Created);

    // Second anonymous review (should create new entry)
    let resp = client
        .post(format!("/api/v1/apps/{}/reviews", id))
        .header(ContentType::JSON)
        .body(r#"{"rating":2,"title":"Another anonymous"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Created);

    let resp = client
        .get(format!("/api/v1/apps/{}/reviews", id))
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    // At least 3 reviews (2 anonymous + 1 authenticated)
    assert!(body["total"].as_i64().unwrap() >= 3);
}

// ── Review: reviewer_name field preserved ──

#[test]
fn test_review_reviewer_name_field() {
    let (client, key) = setup_client();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Reviewer Name App","short_description":"Test","description":"Test reviewer name","author_name":"Test"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();

    // Submit anonymous review with reviewer_name
    client
        .post(format!("/api/v1/apps/{}/reviews", id))
        .header(ContentType::JSON)
        .body(r#"{"rating":4,"title":"Named review","reviewer_name":"CoolAgent42"}"#)
        .dispatch();

    let resp = client
        .get(format!("/api/v1/apps/{}/reviews", id))
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let reviews = body["reviews"].as_array().unwrap();
    assert!(!reviews.is_empty());
    // Find the review with our name
    let found = reviews.iter().any(|r| r["reviewer_name"] == "CoolAgent42");
    assert!(found, "reviewer_name should be preserved in review list");
}

// ── Full state machine lifecycle ──

#[test]
fn test_full_approval_state_machine() {
    let (client, key) = setup_client();

    // Admin submit → auto-approved
    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"State Machine App","short_description":"Test","description":"Full lifecycle test","author_name":"Test"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();
    assert_eq!(body["status"], "approved");

    // Reject it
    let resp = client
        .post(format!("/api/v1/apps/{}/reject", id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"reason":"Quality concerns"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // Verify rejected via GET
    let resp = client.get(format!("/api/v1/apps/{}", id)).dispatch();
    let app: Value = resp.into_json().unwrap();
    assert_eq!(app["status"], "rejected");

    // Re-approve it
    let resp = client
        .post(format!("/api/v1/apps/{}/approve", id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"note":"Issues resolved"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // Verify review metadata on app via GET
    let resp = client.get(format!("/api/v1/apps/{}", id)).dispatch();
    let app: Value = resp.into_json().unwrap();
    assert_eq!(app["status"], "approved");
    assert!(app["reviewed_at"].is_string());
}

// ── Deprecation → rejection blocked ──

#[test]
fn test_deprecated_reject_blocked() {
    let (client, key) = setup_client();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Deprecate Block Test","short_description":"Test","description":"Deprecation reject block","author_name":"Test"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();

    // Deprecate
    client
        .post(format!("/api/v1/apps/{}/deprecate", id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"reason":"Deprecated"}"#)
        .dispatch();

    // Reject should be blocked (deprecated apps can't be rejected)
    let resp = client
        .post(format!("/api/v1/apps/{}/reject", id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"reason":"Trying to reject deprecated"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Conflict);
}

// ── Deprecation: sunset_at field ──

#[test]
fn test_deprecation_with_sunset_date() {
    let (client, key) = setup_client();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Sunset Test","short_description":"Test","description":"Test sunset","author_name":"Test"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();

    // Deprecate with sunset date
    let resp = client
        .post(format!("/api/v1/apps/{}/deprecate", id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"reason":"End of life","sunset_at":"2026-06-01T00:00:00Z"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // Verify sunset_at preserved
    let resp = client.get(format!("/api/v1/apps/{}", id)).dispatch();
    let app: Value = resp.into_json().unwrap();
    assert_eq!(app["status"], "deprecated");
    assert!(app["sunset_at"].as_str().unwrap().contains("2026-06-01"));
}

// ── Full lifecycle: submit→approve→update→deprecate→undeprecate→delete ──

#[test]
fn test_full_lifecycle() {
    let (client, key) = setup_client();

    // Submit
    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Lifecycle App","short_description":"Full lifecycle","description":"Testing all stages","author_name":"Test","protocol":"rest","category":"developer-tools","tags":["lifecycle"]}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Created);
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap().to_string();
    assert_eq!(body["status"], "approved");

    // Add review
    let resp = client
        .post(format!("/api/v1/apps/{}/reviews", id))
        .header(ContentType::JSON)
        .body(r#"{"rating":5,"title":"Perfect","comment":"Works great"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Created);

    // Generate views
    client.get(format!("/api/v1/apps/{}", id)).dispatch();
    client.get(format!("/api/v1/apps/{}", id)).dispatch();

    // Update
    let resp = client
        .patch(format!("/api/v1/apps/{}", id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"description":"Updated description for lifecycle test"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // Deprecate
    let resp = client
        .post(format!("/api/v1/apps/{}/deprecate", id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"reason":"Replaced by v2"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // Undeprecate
    let resp = client
        .post(format!("/api/v1/apps/{}/undeprecate", id))
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // Check stats exist
    let resp = client.get(format!("/api/v1/apps/{}/stats", id)).dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let stats: Value = resp.into_json().unwrap();
    assert!(stats["total_views"].as_i64().unwrap() >= 2);

    // Delete
    let resp = client
        .delete(format!("/api/v1/apps/{}", id))
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // Gone
    let resp = client.get(format!("/api/v1/apps/{}", id)).dispatch();
    assert_eq!(resp.status(), Status::NotFound);

    // Stats also 404
    let resp = client.get(format!("/api/v1/apps/{}/stats", id)).dispatch();
    assert_eq!(resp.status(), Status::NotFound);
}

// ── Edit token: full lifecycle ──

#[test]
fn test_edit_token_full_lifecycle() {
    let (client, _) = setup_client();

    // Anonymous submit (no API key) → gets edit token
    let resp = client
        .post("/api/v1/apps")
        .header(ContentType::JSON)
        .body(r#"{"name":"Token Lifecycle App","short_description":"Test","description":"Edit token lifecycle","author_name":"Test Agent"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Created);
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap().to_string();
    let token = body["edit_token"].as_str().unwrap().to_string();
    assert!(!token.is_empty());

    // Update via query param
    let resp = client
        .patch(format!("/api/v1/apps/{}?token={}", id, token))
        .header(ContentType::JSON)
        .body(r#"{"description":"Updated via query param token"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // Update via header
    let resp = client
        .patch(format!("/api/v1/apps/{}", id))
        .header(Header::new("X-Edit-Token", token.clone()))
        .header(ContentType::JSON)
        .body(r#"{"short_description":"Updated via header token"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // Delete via header
    let resp = client
        .delete(format!("/api/v1/apps/{}", id))
        .header(Header::new("X-Edit-Token", token.clone()))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // Confirm deleted
    let resp = client.get(format!("/api/v1/apps/{}", id)).dispatch();
    assert_eq!(resp.status(), Status::NotFound);
}

// ── Webhook: multiple webhooks for same events ──

#[test]
fn test_multiple_webhooks_same_events() {
    let (client, key) = setup_client();

    // Create two webhooks both listening to same event
    let resp = client
        .post("/api/v1/webhooks")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"url":"https://hook1.example.com","events":["app.submitted"]}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Created);
    let wh1: Value = resp.into_json().unwrap();

    let resp = client
        .post("/api/v1/webhooks")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"url":"https://hook2.example.com","events":["app.submitted"]}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Created);
    let wh2: Value = resp.into_json().unwrap();

    // Both should have different IDs and secrets
    assert_ne!(wh1["id"], wh2["id"]);
    assert_ne!(wh1["secret"], wh2["secret"]);

    // List should show both
    let resp = client
        .get("/api/v1/webhooks")
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let webhooks = body["webhooks"].as_array().unwrap();
    assert!(webhooks.len() >= 2);
}

// ── Webhook: deactivate and reactivate ──

#[test]
fn test_webhook_deactivate_reactivate() {
    let (client, key) = setup_client();

    let resp = client
        .post("/api/v1/webhooks")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"url":"https://hook.example.com/toggle"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let wh_id = body["id"].as_str().unwrap();
    assert_eq!(body["active"], true);

    // Deactivate
    let resp = client
        .patch(format!("/api/v1/webhooks/{}", wh_id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"active":false}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: Value = resp.into_json().unwrap();
    assert_eq!(body["active"], false);

    // Reactivate
    let resp = client
        .patch(format!("/api/v1/webhooks/{}", wh_id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"active":true}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: Value = resp.into_json().unwrap();
    assert_eq!(body["active"], true);
}

// ── Webhook: HMAC secret only shown once ──

#[test]
fn test_webhook_secret_only_on_create() {
    let (client, key) = setup_client();

    // Create webhook — secret visible
    let resp = client
        .post("/api/v1/webhooks")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"url":"https://hook.example.com/secret-test"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    assert!(body["secret"].is_string());
    let wh_id = body["id"].as_str().unwrap();

    // List webhooks — secret should be hidden
    let resp = client
        .get("/api/v1/webhooks")
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let webhooks = body["webhooks"].as_array().unwrap();
    let our_wh = webhooks.iter().find(|w| w["id"] == wh_id);
    assert!(our_wh.is_some());
    // Secret should be null or absent in list
    let wh = our_wh.unwrap();
    assert!(wh.get("secret").is_none() || wh["secret"].is_null());
}

// ── Key management: create + list + revoke lifecycle ──

#[test]
fn test_key_full_lifecycle() {
    let (client, key) = setup_client();

    // Create a new key
    let resp = client
        .post("/api/v1/keys")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"test-lifecycle-key"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Created);
    let body: Value = resp.into_json().unwrap();
    let new_key = body["api_key"].as_str().unwrap().to_string();
    assert!(!new_key.is_empty());

    // Use the new key to submit an app
    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", new_key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Key Lifecycle App","short_description":"Test","description":"Test key lifecycle","author_name":"Test"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Created);

    // List keys to find the new key's ID
    let resp = client
        .get("/api/v1/keys")
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let keys = body["keys"].as_array().unwrap();
    // Find the key with name "test-lifecycle-key"
    let our_key = keys
        .iter()
        .find(|k| k["name"] == "test-lifecycle-key")
        .expect("should find our key in list");
    let key_id = our_key["id"].as_str().unwrap().to_string();

    // Revoke it
    let resp = client
        .delete(format!("/api/v1/keys/{}", key_id))
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // Revoked key should no longer work
    let resp = client
        .get("/api/v1/keys")
        .header(Header::new("X-API-Key", new_key))
        .dispatch();
    assert_eq!(resp.status(), Status::Unauthorized);
}

// ── Search: combined category + protocol filter ──

#[test]
fn test_search_combined_filters() {
    let (client, key) = setup_client();

    client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Search Filter MCP","short_description":"MCP tool","description":"An MCP tool for testing","author_name":"Test","protocol":"mcp","category":"ai-ml"}"#)
        .dispatch();

    client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Search Filter REST","short_description":"REST tool","description":"A REST tool for testing","author_name":"Test","protocol":"rest","category":"ai-ml"}"#)
        .dispatch();

    // Search with category filter
    let resp = client
        .get("/api/v1/apps/search?q=tool&category=ai-ml")
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: Value = resp.into_json().unwrap();
    let results = body["apps"].as_array().unwrap();
    for app in results {
        assert_eq!(app["category"], "ai-ml");
    }

    // Search with protocol filter
    let resp = client
        .get("/api/v1/apps/search?q=tool&protocol=mcp")
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: Value = resp.into_json().unwrap();
    let results = body["apps"].as_array().unwrap();
    for app in results {
        assert_eq!(app["protocol"], "mcp");
    }
}

// ── Error response format consistency ──

#[test]
fn test_error_404_response_format() {
    let (client, _) = setup_client();

    let resp = client
        .get("/api/v1/apps/nonexistent-slug-that-does-not-exist")
        .dispatch();
    assert_eq!(resp.status(), Status::NotFound);
    let body: Value = resp.into_json().unwrap();
    assert!(body["error"].is_string());
    assert!(body["message"].is_string());
}

#[test]
fn test_error_401_response_format() {
    let (client, _) = setup_client();

    let resp = client.get("/api/v1/keys").dispatch();
    assert_eq!(resp.status(), Status::Unauthorized);
    // 401 may be Rocket's default handler (not JSON) or custom JSON
    let body_str = resp.into_string().unwrap_or_default();
    assert!(!body_str.is_empty(), "401 response should have a body");
}

#[test]
fn test_error_invalid_api_key() {
    let (client, _) = setup_client();

    let resp = client
        .get("/api/v1/keys")
        .header(Header::new("X-API-Key", "totally-invalid-key"))
        .dispatch();
    assert_eq!(resp.status(), Status::Unauthorized);
}

// ── Submission: all protocols accepted ──

#[test]
fn test_all_protocols_accepted() {
    let (client, key) = setup_client();

    let protocols = ["rest", "graphql", "grpc", "mcp", "a2a", "websocket", "other"];

    for proto in protocols {
        let resp = client
            .post("/api/v1/apps")
            .header(Header::new("X-API-Key", key.clone()))
            .header(ContentType::JSON)
            .body(format!(
                r#"{{"name":"Proto {} App","short_description":"Test","description":"Protocol test","author_name":"Test","protocol":"{}"}}"#,
                proto, proto
            ))
            .dispatch();
        assert_eq!(resp.status(), Status::Created, "Protocol {} should be accepted", proto);

        // Verify protocol on GET
        let body: Value = resp.into_json().unwrap();
        let id = body["app_id"].as_str().unwrap();
        let resp = client.get(format!("/api/v1/apps/{}", id)).dispatch();
        let app: Value = resp.into_json().unwrap();
        assert_eq!(app["protocol"], proto);
    }
}

// ── Submission: all valid categories accepted ──

#[test]
fn test_all_categories_accepted() {
    let (client, key) = setup_client();

    let categories = [
        "communication", "data", "developer-tools", "finance", "media",
        "productivity", "search", "security", "social", "ai-ml",
        "infrastructure", "other",
    ];

    for cat in categories {
        let resp = client
            .post("/api/v1/apps")
            .header(Header::new("X-API-Key", key.clone()))
            .header(ContentType::JSON)
            .body(format!(
                r#"{{"name":"Cat {} App","short_description":"Test","description":"Category test","author_name":"Test","category":"{}"}}"#,
                cat, cat
            ))
            .dispatch();
        assert_eq!(resp.status(), Status::Created, "Category {} should be accepted", cat);
    }
}

// ── Sort: newest first (default) ──

#[test]
fn test_list_apps_default_newest_first() {
    let (client, key) = setup_client();

    for i in 0..3 {
        client
            .post("/api/v1/apps")
            .header(Header::new("X-API-Key", key.clone()))
            .header(ContentType::JSON)
            .body(format!(
                r#"{{"name":"Sort App {}","short_description":"Test","description":"Sort test {}","author_name":"Test"}}"#,
                i, i
            ))
            .dispatch();
        // Small sleep not needed since SQLite datetime has second resolution
    }

    let resp = client.get("/api/v1/apps").dispatch();
    let body: Value = resp.into_json().unwrap();
    let apps = body["apps"].as_array().unwrap();
    assert!(apps.len() >= 3);

    // Default sort is newest first → last submitted should be first in list
    if apps.len() >= 2 {
        let first_created = apps[0]["created_at"].as_str().unwrap();
        let second_created = apps[1]["created_at"].as_str().unwrap();
        assert!(first_created >= second_created, "Default sort should be newest first");
    }
}

// ── Sort: by name ascending ──

#[test]
fn test_list_apps_sort_by_name_ascending() {
    let (client, key) = setup_client();

    let names = ["Zebra App", "Alpha App", "Middle App"];
    for name in names {
        client
            .post("/api/v1/apps")
            .header(Header::new("X-API-Key", key.clone()))
            .header(ContentType::JSON)
            .body(format!(
                r#"{{"name":"{}","short_description":"Test","description":"Sort test","author_name":"Test"}}"#,
                name
            ))
            .dispatch();
    }

    let resp = client.get("/api/v1/apps?sort=name").dispatch();
    let body: Value = resp.into_json().unwrap();
    let apps = body["apps"].as_array().unwrap();
    assert!(apps.len() >= 3);

    // Verify alphabetical order
    for i in 0..apps.len() - 1 {
        let a = apps[i]["name"].as_str().unwrap().to_lowercase();
        let b = apps[i + 1]["name"].as_str().unwrap().to_lowercase();
        assert!(a <= b, "Apps should be sorted alphabetically: {} <= {}", a, b);
    }
}

// ── Unicode content handling ──

#[test]
fn test_unicode_app_content() {
    let (client, key) = setup_client();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"日本語アプリ","short_description":"テストアプリ","description":"🚀 Unicode app with CJK characters: 中文, 한국어, العربية","author_name":"テスター","tags":["emoji","🎯","cjk"]}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Created);
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();

    // Verify unicode content via GET
    let resp = client.get(format!("/api/v1/apps/{}", id)).dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let app: Value = resp.into_json().unwrap();
    assert_eq!(app["name"], "日本語アプリ");
    assert!(app["description"].as_str().unwrap().contains("🚀"));
    let tags = app["tags"].as_array().unwrap();
    assert!(tags.contains(&serde_json::json!("🎯")));
}

// ── Unicode review content ──

#[test]
fn test_unicode_review() {
    let (client, key) = setup_client();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Unicode Review Target","short_description":"Test","description":"For unicode reviews","author_name":"Test"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();

    let resp = client
        .post(format!("/api/v1/apps/{}/reviews", id))
        .header(ContentType::JSON)
        .body(r#"{"rating":5,"title":"素晴らしい！","comment":"これは最高のアプリです 🎉","reviewer_name":"日本のエージェント"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Created);

    let resp = client
        .get(format!("/api/v1/apps/{}/reviews", id))
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let reviews = body["reviews"].as_array().unwrap();
    assert!(!reviews.is_empty());
    assert_eq!(reviews[0]["title"], "素晴らしい！");
}

// ── App response timestamps ──

#[test]
fn test_app_timestamps() {
    let (client, key) = setup_client();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Timestamp App","short_description":"Test","description":"Timestamps test","author_name":"Test"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();

    // Check timestamps via GET
    let resp = client.get(format!("/api/v1/apps/{}", id)).dispatch();
    let app: Value = resp.into_json().unwrap();
    assert!(app["created_at"].is_string());

    // Update
    let resp = client
        .patch(format!("/api/v1/apps/{}", id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"description":"Updated for timestamp check"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    let resp = client.get(format!("/api/v1/apps/{}", id)).dispatch();
    let app: Value = resp.into_json().unwrap();
    assert!(app["created_at"].is_string());
    assert!(app["updated_at"].is_string());
}

// ── Tags replacement on update ──

#[test]
fn test_tags_replaced_on_update() {
    let (client, key) = setup_client();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Tags Update App","short_description":"Test","description":"Tags update test","author_name":"Test","tags":["old","original"]}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();

    // Update tags
    let resp = client
        .patch(format!("/api/v1/apps/{}", id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"tags":["new","replaced"]}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    let resp = client.get(format!("/api/v1/apps/{}", id)).dispatch();
    let app: Value = resp.into_json().unwrap();
    let tags: Vec<String> = app["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap().to_string())
        .collect();
    assert!(tags.contains(&"new".to_string()));
    assert!(tags.contains(&"replaced".to_string()));
    assert!(!tags.contains(&"old".to_string()));
}

// ── Categories count reflects actual apps ──

#[test]
fn test_categories_count_accuracy() {
    let (client, key) = setup_client();

    // Submit 2 apps in "security", 1 in "infrastructure"
    for i in 0..2 {
        client
            .post("/api/v1/apps")
            .header(Header::new("X-API-Key", key.clone()))
            .header(ContentType::JSON)
            .body(format!(
                r#"{{"name":"Security App {}","short_description":"Test","description":"Security test","author_name":"Test","category":"security"}}"#,
                i
            ))
            .dispatch();
    }
    client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Infra App","short_description":"Test","description":"Infrastructure test","author_name":"Test","category":"infrastructure"}"#)
        .dispatch();

    let resp = client.get("/api/v1/categories").dispatch();
    let body: Value = resp.into_json().unwrap();
    let cats = body["categories"].as_array().unwrap();

    let security = cats.iter().find(|c| c["name"] == "security");
    assert!(security.is_some());
    assert_eq!(security.unwrap()["count"], 2);

    let infra = cats.iter().find(|c| c["name"] == "infrastructure");
    assert!(infra.is_some());
    assert_eq!(infra.unwrap()["count"], 1);
}

// ── OpenAPI spec structure validation ──

#[test]
fn test_openapi_spec_structure() {
    let (client, _) = setup_client();

    let resp = client.get("/api/v1/openapi.json").dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let spec: Value = resp.into_json().unwrap();
    assert_eq!(spec["openapi"], "3.0.3");
    assert!(spec["info"]["title"].is_string());
    assert!(spec["info"]["version"].is_string());
    assert!(spec["paths"].is_object());
    // Should have at least 15 paths
    let paths = spec["paths"].as_object().unwrap();
    assert!(paths.len() >= 15, "OpenAPI should have at least 15 paths, got {}", paths.len());
}

// ── My apps: shows only submitter's apps ──

#[test]
fn test_my_apps_isolation() {
    let (client, key, path) = setup_client_with_path();

    // Create second key
    let conn = rusqlite::Connection::open(&path).unwrap();
    let key2 = app_directory::auth::create_api_key(&conn, "agent2", false, Some(10000));
    drop(conn);

    // Key 1 submits app
    client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Key1 App","short_description":"Test","description":"Belongs to key1","author_name":"Test"}"#)
        .dispatch();

    // Key 2 submits app
    client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key2.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Key2 App","short_description":"Test","description":"Belongs to key2","author_name":"Test"}"#)
        .dispatch();

    // Key 1 /mine should only show key1's app
    let resp = client
        .get("/api/v1/apps/mine")
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let apps = body["apps"].as_array().unwrap();
    for app in apps {
        assert_ne!(app["name"], "Key2 App", "Key1 should not see Key2's apps in /mine");
    }

    // Key 2 /mine should only show key2's app
    let resp = client
        .get("/api/v1/apps/mine")
        .header(Header::new("X-API-Key", key2))
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let apps = body["apps"].as_array().unwrap();
    for app in apps {
        assert_ne!(app["name"], "Key1 App", "Key2 should not see Key1's apps in /mine");
    }
}

// ── Batch health check: requires admin ──

#[test]
fn test_batch_health_check_requires_admin() {
    let (client, _, path) = setup_client_with_path();

    let conn = rusqlite::Connection::open(&path).unwrap();
    let user_key = app_directory::auth::create_api_key(&conn, "user", false, Some(10000));
    drop(conn);

    let resp = client
        .post("/api/v1/apps/health-check/batch")
        .header(Header::new("X-API-Key", user_key))
        .dispatch();
    assert_eq!(resp.status(), Status::Forbidden);
}

// ── Batch health check: admin success ──

#[test]
fn test_batch_health_check_admin() {
    let (client, key) = setup_client();

    // Submit an app with URL
    client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Batch Health App","short_description":"Test","description":"Batch health check test","author_name":"Test","api_url":"https://httpbin.org/status/200"}"#)
        .dispatch();

    let resp = client
        .post("/api/v1/apps/health-check/batch")
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: Value = resp.into_json().unwrap();
    assert!(body["total"].is_number());
    assert!(body["results"].is_array());
}

// ── Delete with wrong edit token ──

#[test]
fn test_delete_wrong_edit_token() {
    let (client, _) = setup_client();

    let resp = client
        .post("/api/v1/apps")
        .header(ContentType::JSON)
        .body(r#"{"name":"Wrong Token Delete","short_description":"Test","description":"Test","author_name":"Test"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();

    let resp = client
        .delete(format!("/api/v1/apps/{}", id))
        .header(Header::new("X-Edit-Token", "wrong-token-value"))
        .dispatch();
    assert!(
        resp.status() == Status::Forbidden || resp.status() == Status::Unauthorized,
        "Wrong token should be rejected"
    );
}

// ── Large tags array ──

#[test]
fn test_large_tags_array() {
    let (client, key) = setup_client();

    let tags: Vec<String> = (0..20).map(|i| format!("tag-{}", i)).collect();
    let tags_json = serde_json::to_string(&tags).unwrap();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(format!(
            r#"{{"name":"Many Tags App","short_description":"Test","description":"Test with many tags","author_name":"Test","tags":{}}}"#,
            tags_json
        ))
        .dispatch();
    assert_eq!(resp.status(), Status::Created);
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();

    // Verify tags via GET
    let resp = client.get(format!("/api/v1/apps/{}", id)).dispatch();
    let app: Value = resp.into_json().unwrap();
    let result_tags = app["tags"].as_array().unwrap();
    assert_eq!(result_tags.len(), 20);
}

// ── App with all optional fields ──

#[test]
fn test_submit_with_all_optional_fields() {
    let (client, key) = setup_client();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{
            "name": "Full App",
            "short_description": "A fully-specified app",
            "description": "This app has every optional field set",
            "author_name": "Complete Author",
            "protocol": "mcp",
            "category": "ai-ml",
            "tags": ["complete", "full"],
            "homepage_url": "https://example.com",
            "api_url": "https://api.example.com/v1",
            "api_spec_url": "https://api.example.com/openapi.json",
            "logo_url": "https://example.com/logo.png",
            "author_url": "https://example.com/author"
        }"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Created);
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();

    // Verify all fields via GET
    let resp = client.get(format!("/api/v1/apps/{}", id)).dispatch();
    let app: Value = resp.into_json().unwrap();
    assert_eq!(app["name"], "Full App");
    assert_eq!(app["protocol"], "mcp");
    assert_eq!(app["category"], "ai-ml");
    assert!(app["homepage_url"].is_string());
    assert!(app["api_url"].is_string());
    assert!(app["api_spec_url"].is_string());
    assert!(app["logo_url"].is_string());
    assert!(app["author_url"].is_string());
}

// ── Deprecation: verified fields cleared on undeprecate ──

#[test]
fn test_undeprecate_clears_all_fields() {
    let (client, key) = setup_client();

    // Submit + submit second app for replacement
    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Replacement App","short_description":"Test","description":"This replaces the old one","author_name":"Test"}"#)
        .dispatch();
    let replacement: Value = resp.into_json().unwrap();
    let replacement_id = replacement["app_id"].as_str().unwrap();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"To Be Undeprecated","short_description":"Test","description":"Will be undeprecated","author_name":"Test"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();

    // Deprecate with replacement + sunset
    client
        .post(format!("/api/v1/apps/{}/deprecate", id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(format!(
            r#"{{"reason":"Being replaced","replacement_app_id":"{}","sunset_at":"2026-12-31T00:00:00Z"}}"#,
            replacement_id
        ))
        .dispatch();

    // Verify all deprecation fields set
    let resp = client.get(format!("/api/v1/apps/{}", id)).dispatch();
    let app: Value = resp.into_json().unwrap();
    assert_eq!(app["status"], "deprecated");
    assert!(app["deprecated_reason"].is_string());
    assert!(app["deprecated_at"].is_string());
    assert!(app["replacement_app_id"].is_string());
    assert!(app["sunset_at"].is_string());

    // Undeprecate
    client
        .post(format!("/api/v1/apps/{}/undeprecate", id))
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();

    // All deprecation fields should be cleared
    let resp = client.get(format!("/api/v1/apps/{}", id)).dispatch();
    let app: Value = resp.into_json().unwrap();
    assert_eq!(app["status"], "approved");
    assert!(
        app["deprecated_reason"].is_null()
            || app["deprecated_reason"].as_str().unwrap_or("").is_empty()
    );
    assert!(app["replacement_app_id"].is_null());
    assert!(app["sunset_at"].is_null());
}

// ── Slug: special characters stripped ──

#[test]
fn test_slug_strips_special_characters() {
    let (client, key) = setup_client();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"My App! (v2.0) - Special @#$","short_description":"Test","description":"Slug special char test","author_name":"Test"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Created);
    let body: Value = resp.into_json().unwrap();
    let slug = body["slug"].as_str().unwrap();
    // Slug should be lowercase, no special chars
    assert!(!slug.contains('!'));
    assert!(!slug.contains('@'));
    assert!(!slug.contains('#'));
    assert!(!slug.contains('$'));
    assert!(slug.chars().all(|c| c.is_alphanumeric() || c == '-'));
}

// ── Verified badge filter ──

#[test]
fn test_filter_verified_apps() {
    let (client, key) = setup_client();

    // Submit and verify one app
    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Verified App","short_description":"Test","description":"A verified app","author_name":"Test"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();

    // Set verified badge
    client
        .patch(format!("/api/v1/apps/{}", id))
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"is_verified":true}"#)
        .dispatch();

    // Submit non-verified app
    client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Unverified App","short_description":"Test","description":"Not verified","author_name":"Test"}"#)
        .dispatch();

    // Filter verified=true
    let resp = client.get("/api/v1/apps?verified=true").dispatch();
    let body: Value = resp.into_json().unwrap();
    let apps = body["apps"].as_array().unwrap();
    for app in apps {
        assert_eq!(app["is_verified"], true);
    }
    assert!(!apps.is_empty());
}

// ── Review ordering: newest first ──

#[test]
fn test_review_ordering() {
    let (client, key) = setup_client();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Review Order App","short_description":"Test","description":"Review ordering test","author_name":"Test"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap();

    // Submit multiple anonymous reviews
    for i in 1..=4 {
        client
            .post(format!("/api/v1/apps/{}/reviews", id))
            .header(ContentType::JSON)
            .body(format!(r#"{{"rating":{},"title":"Review {}"}}"#, i, i))
            .dispatch();
    }

    let resp = client
        .get(format!("/api/v1/apps/{}/reviews", id))
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    assert!(body["total"].as_i64().unwrap() >= 4);
    let reviews = body["reviews"].as_array().unwrap();
    // Verify ordering — newest first (most recent should be rating 4)
    if reviews.len() >= 2 {
        let first_created = reviews[0]["created_at"].as_str().unwrap_or("");
        let second_created = reviews[1]["created_at"].as_str().unwrap_or("");
        assert!(first_created >= second_created, "Reviews should be newest first");
    }
}

// ── SSE stream endpoint accessible ──

#[test]
fn test_sse_stream_accessible() {
    let (client, _) = setup_client();

    // SSE endpoint should be public (no auth)
    let resp = client.get("/api/v1/events/stream").dispatch();
    // Should return 200 with event-stream content type
    assert_eq!(resp.status(), Status::Ok);
    let ct = resp.content_type();
    assert!(ct.is_some());
}

// ── Delete cascade: views cleaned up ──

#[test]
fn test_delete_cascade_views_cleaned() {
    let (client, key) = setup_client();

    let resp = client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Delete Views App","short_description":"Test","description":"Delete cascade views","author_name":"Test"}"#)
        .dispatch();
    let body: Value = resp.into_json().unwrap();
    let id = body["app_id"].as_str().unwrap().to_string();

    // Generate some views
    for _ in 0..5 {
        client.get(format!("/api/v1/apps/{}", id)).dispatch();
    }

    // Verify views exist
    let resp = client.get(format!("/api/v1/apps/{}/stats", id)).dispatch();
    let stats: Value = resp.into_json().unwrap();
    assert!(stats["total_views"].as_i64().unwrap() >= 5);

    // Delete app
    client
        .delete(format!("/api/v1/apps/{}", id))
        .header(Header::new("X-API-Key", key.clone()))
        .dispatch();

    // Stats should 404
    let resp = client.get(format!("/api/v1/apps/{}/stats", id)).dispatch();
    assert_eq!(resp.status(), Status::NotFound);
}

// ── Multiple apps: category isolation ──

#[test]
fn test_category_filter_isolation() {
    let (client, key) = setup_client();

    client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Data App","short_description":"Test","description":"A data app","author_name":"Test","category":"data"}"#)
        .dispatch();

    client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Finance App","short_description":"Test","description":"A finance app","author_name":"Test","category":"finance"}"#)
        .dispatch();

    // Filter by data should not include finance
    let resp = client.get("/api/v1/apps?category=data").dispatch();
    let body: Value = resp.into_json().unwrap();
    let apps = body["apps"].as_array().unwrap();
    for app in apps {
        assert_eq!(app["category"], "data");
    }

    // Filter by finance should not include data
    let resp = client.get("/api/v1/apps?category=finance").dispatch();
    let body: Value = resp.into_json().unwrap();
    let apps = body["apps"].as_array().unwrap();
    for app in apps {
        assert_eq!(app["category"], "finance");
    }
}

#[test]
fn test_list_apps_search_param() {
    let (client, admin_key) = setup_client();

    // Create two apps with distinct names/descriptions (admin key → auto-approved)
    client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", admin_key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"SearchTarget Alpha","short_description":"Unique identifier alpha","description":"An app for testing search","author_name":"Tester","category":"developer-tools"}"#)
        .dispatch();

    client
        .post("/api/v1/apps")
        .header(Header::new("X-API-Key", admin_key.clone()))
        .header(ContentType::JSON)
        .body(r#"{"name":"Unrelated Beta","short_description":"Completely different thing","description":"No match here","author_name":"Tester","category":"developer-tools"}"#)
        .dispatch();

    // Search via ?search= on the list endpoint — should filter by name
    let resp = client.get("/api/v1/apps?search=alpha").dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: Value = resp.into_json().unwrap();
    let apps = body["apps"].as_array().unwrap();
    assert!(
        apps.iter().any(|a| a["name"].as_str().unwrap_or("").to_lowercase().contains("alpha")),
        "search=alpha should return apps matching 'alpha'"
    );
    // Beta app should not appear in alpha search
    assert!(
        !apps.iter().any(|a| a["name"].as_str().unwrap_or("") == "Unrelated Beta"),
        "search=alpha should not return 'Unrelated Beta'"
    );

    // Search by short_description term
    let resp = client.get("/api/v1/apps?search=unique%20identifier%20alpha").dispatch();
    let body: Value = resp.into_json().unwrap();
    let total = body["total"].as_i64().unwrap_or(0);
    assert!(total >= 1, "search by short_description term should find at least 1 result");

    // Search for non-existent term returns empty
    let resp = client.get("/api/v1/apps?search=zzz_nonexistent_zzz_xyzabc").dispatch();
    let body: Value = resp.into_json().unwrap();
    let apps = body["apps"].as_array().unwrap();
    assert!(apps.is_empty(), "search for non-existent term should return empty list");

    // Search with empty string returns all apps (same as no filter)
    let resp = client.get("/api/v1/apps?search=").dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: Value = resp.into_json().unwrap();
    let total_empty = body["total"].as_i64().unwrap_or(0);
    assert!(total_empty >= 2, "empty search= should return all approved apps");
}

