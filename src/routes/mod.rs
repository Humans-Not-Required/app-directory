mod admin;
mod apps;
mod keys;
mod reviews;
mod system;
mod webhook_routes;

// Re-export all route handlers for mounting in lib.rs
pub use admin::{approve_app, deprecate_app, reject_app, undeprecate_app};
pub use apps::{
    delete_app, get_app, list_apps, list_my_apps, list_pending_apps, search_apps, submit_app,
    update_app,
};
pub use keys::{create_key, delete_key, list_keys};
pub use reviews::{get_reviews, list_categories, submit_review};
pub use system::{cors_preflight, event_stream, health, llms_txt, openapi, root_llms_txt};
pub use webhook_routes::{create_webhook, delete_webhook, list_webhooks, update_webhook};
