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
