use serde::Serialize;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

use crate::webhooks::{self, WebhookDb, WebhookEvent};

/// Maximum events buffered per channel before old events are dropped.
const CHANNEL_CAPACITY: usize = 256;

/// Internal shared state for EventBus.
struct EventBusInner {
    /// Global channel for SSE subscribers
    channel: Mutex<Option<broadcast::Sender<AppEvent>>>,
    webhook_db: Option<WebhookDb>,
    http_client: reqwest::Client,
}

/// A global event broadcast system for the app directory.
///
/// Uses a single broadcast channel (all events are global, not per-board).
/// Also delivers events to registered webhooks.
///
/// Cheaply cloneable via internal `Arc`.
#[derive(Clone)]
pub struct EventBus {
    inner: Arc<EventBusInner>,
}

/// A typed event emitted when something happens in the directory.
#[derive(Debug, Clone, Serialize)]
pub struct AppEvent {
    /// The type of event (e.g., "app.submitted", "review.submitted")
    pub event: String,
    /// JSON payload with event-specific data
    pub data: serde_json::Value,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(EventBusInner {
                channel: Mutex::new(None),
                webhook_db: None,
                http_client: reqwest::Client::new(),
            }),
        }
    }

    /// Create an EventBus with webhook delivery support.
    pub fn with_webhooks(webhook_db: WebhookDb) -> Self {
        Self {
            inner: Arc::new(EventBusInner {
                channel: Mutex::new(None),
                webhook_db: Some(webhook_db),
                http_client: reqwest::Client::new(),
            }),
        }
    }

    /// Subscribe to all directory events.
    /// Returns a broadcast receiver that yields AppEvents.
    pub fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        let mut channel = self.inner.channel.lock().unwrap();
        let sender = channel.get_or_insert_with(|| broadcast::channel(CHANNEL_CAPACITY).0);
        sender.subscribe()
    }

    /// Emit an event to all SSE subscribers and webhook targets.
    pub fn emit(&self, event: AppEvent) {
        // Deliver to SSE subscribers
        {
            let channel = self.inner.channel.lock().unwrap();
            if let Some(sender) = channel.as_ref() {
                let _ = sender.send(event.clone());
            }
        }

        // Deliver to webhooks (async, non-blocking)
        if let Some(ref db) = self.inner.webhook_db {
            webhooks::deliver_webhooks(
                db.clone(),
                WebhookEvent {
                    event: event.event,
                    data: event.data,
                },
                self.inner.http_client.clone(),
            );
        }
    }
}
