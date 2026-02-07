#!/bin/bash
# Script to update routes.rs with new auth model

cd ~/humans-not-required/app-directory

# Create the new update_app function
cat > /tmp/new_update_app.rs << 'EOF'
// === Update App (Edit Token or API Key Required) ===

#[patch("/apps/<id>", data = "<body>")]
pub async fn update_app(
    request: &rocket::Request<'_>,
    id: String,
    body: Json<UpdateAppRequest>,
    db: &rocket::State<DbState>,
    bus: &rocket::State<EventBus>,
) -> (Status, Json<Value>) {
    // Check edit authorization
    let edit_auth = match EditAuth::from_request_for_app(request, &id).await {
        Ok(auth) => auth,
        Err(status) => {
            return (
                status,
                Json(json!({ "error": "FORBIDDEN", "message": "Edit token, owning API key, or admin key required" })),
            );
        }
    };

    let conn = db.0.lock().unwrap();

    // Only admins can change status or badges
    let is_admin = matches!(edit_auth.via, crate::auth::EditAuthVia::Admin(_));
    
    if body.status.is_some() && !is_admin {
        return (
            Status::Forbidden,
            Json(json!({ "error": "FORBIDDEN", "message": "Only admins can change app status" })),
        );
    }
    if (body.is_featured.is_some() || body.is_verified.is_some()) && !is_admin {
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
        Err(e) => (
            Status::InternalServerError,
            Json(json!({ "error": "DB_ERROR", "message": e.to_string() })),
        ),
    }
}
EOF

echo "Created new update_app function"
echo "Manual integration needed - routes.rs is too complex for automated replacement"
echo "Please see /tmp/new_update_app.rs for the new function"
