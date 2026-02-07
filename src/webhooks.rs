use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::sync::{Arc, Mutex};

type HmacSha256 = Hmac<Sha256>;

/// Shared database connection for async webhook delivery (separate from main).
pub type WebhookDb = Arc<Mutex<rusqlite::Connection>>;

/// Open a separate database connection for async webhook delivery.
pub fn init_webhook_db() -> WebhookDb {
    let db_path = std::env::var("DATABASE_PATH").unwrap_or_else(|_| "app_directory.db".to_string());
    let conn = rusqlite::Connection::open(&db_path).expect("Failed to open webhook DB");
    conn.execute_batch("PRAGMA journal_mode=WAL;")
        .expect("Failed to set WAL mode for webhook DB");
    Arc::new(Mutex::new(conn))
}

/// Compute HMAC-SHA256 signature for a payload.
fn sign_payload(secret: &str, payload: &[u8]) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
    mac.update(payload);
    let result = mac.finalize();
    hex::encode(result.into_bytes())
}

/// A webhook event to deliver.
#[derive(Debug, Clone)]
pub struct WebhookEvent {
    pub event: String,
    pub data: serde_json::Value,
}

/// Loaded webhook target from DB.
#[derive(Debug, Clone)]
struct WebhookTarget {
    id: String,
    url: String,
    secret: String,
    events: Vec<String>,
}

/// Fire-and-forget delivery of a webhook event to all matching registered webhooks.
pub fn deliver_webhooks(db: WebhookDb, event: WebhookEvent, client: reqwest::Client) {
    tokio::spawn(async move {
        let targets = {
            let conn = db.lock().unwrap();
            let mut stmt = match conn.prepare(
                "SELECT id, url, secret, events FROM webhooks WHERE active = 1 AND failure_count < 10",
            ) {
                Ok(s) => s,
                Err(_) => return,
            };

            stmt.query_map([], |row| {
                let events_str: String = row.get(3)?;
                let events: Vec<String> = serde_json::from_str(&events_str).unwrap_or_default();
                Ok(WebhookTarget {
                    id: row.get(0)?,
                    url: row.get(1)?,
                    secret: row.get(2)?,
                    events,
                })
            })
            .ok()
            .map(|rows| rows.filter_map(|r| r.ok()).collect::<Vec<_>>())
            .unwrap_or_default()
        };

        if targets.is_empty() {
            return;
        }

        let payload = serde_json::json!({
            "event": event.event,
            "data": event.data,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });
        let payload_bytes = serde_json::to_vec(&payload).unwrap_or_default();

        for target in targets {
            // Filter: if webhook has specific events configured, check match
            if !target.events.is_empty() && !target.events.contains(&event.event) {
                continue;
            }

            let signature = sign_payload(&target.secret, &payload_bytes);

            let result = client
                .post(&target.url)
                .header("Content-Type", "application/json")
                .header("X-AppDirectory-Signature", format!("sha256={}", signature))
                .header("X-AppDirectory-Event", &event.event)
                .body(payload_bytes.clone())
                .timeout(std::time::Duration::from_secs(10))
                .send()
                .await;

            let success = match result {
                Ok(resp) => resp.status().is_success(),
                Err(_) => false,
            };

            // Update stats
            let db_ref = db.clone();
            let webhook_id = target.id.clone();
            let conn = db_ref.lock().unwrap();
            if success {
                let _ = conn.execute(
                    "UPDATE webhooks SET failure_count = 0, last_triggered_at = datetime('now') WHERE id = ?1",
                    rusqlite::params![webhook_id],
                );
            } else {
                let _ = conn.execute(
                    "UPDATE webhooks SET failure_count = failure_count + 1, last_triggered_at = datetime('now') WHERE id = ?1",
                    rusqlite::params![webhook_id],
                );
            }
        }
    });
}
