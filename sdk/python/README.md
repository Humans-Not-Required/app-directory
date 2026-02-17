# App Directory Python SDK

Zero-dependency Python client for the [HNR App Directory](../../README.md). Works with Python 3.8+ using only the standard library.

## Install

Copy `app_directory.py` into your project — no pip install needed.

```python
from app_directory import AppDirectory
```

## Quick Start

```python
from app_directory import AppDirectory

ad = AppDirectory("http://localhost:3003")

# Browse apps
apps = ad.list_apps(category="data")
for app in apps["apps"]:
    print(f"{app['name']} — {app['short_description']}")

# Search
results = ad.search("monitoring")

# Submit an app (no auth required!)
submitted = ad.submit(
    name="My Service",
    short_description="A useful tool",
    description="Full description...",
    author_name="Alice",
    protocol="rest",
    category="infrastructure",
    tags=["monitoring", "alerts"],
)
token = submitted["edit_token"]  # Save this!

# Update later using edit token
ad.update_app(submitted["id"], edit_token=token, description="Updated desc")
```

## Configuration

```python
# Explicit URL
ad = AppDirectory("http://192.168.0.79:3003")

# From environment
# export APP_DIRECTORY_URL=http://192.168.0.79:3003
ad = AppDirectory()

# With admin API key
ad = AppDirectory("http://localhost:3003", api_key="ad_...")

# With default edit token
ad = AppDirectory("http://localhost:3003", edit_token="ad_...")
```

## Browse & Search

```python
# List with filters
apps = ad.list_apps(category="data", protocol="rest", sort="rating", per_page=10)

# Full-text search
results = ad.search("monitoring", category="infrastructure")

# Get by ID or slug
app = ad.get_app("my-service")
app = ad.get_app("uuid-here")

# Find by exact name
app = ad.find_by_name("Watchpost")

# Categories with counts
cats = ad.categories()
```

## Submit & Manage Apps

```python
# Submit (returns edit_token)
app = ad.submit(
    name="My Service",
    short_description="Brief tagline",
    description="Full description",
    author_name="Alice",
    homepage_url="https://example.com",
    api_url="https://api.example.com",
    protocol="rest",         # rest, graphql, grpc, mcp, a2a, websocket, other
    category="data",         # communication, data, developer-tools, finance, etc.
    tags=["monitoring"],
)

# Update with edit token
ad.update_app(app["id"], edit_token=app["edit_token"], name="New Name")

# Delete
ad.delete_app(app["id"], edit_token=app["edit_token"])
```

## Admin Operations

Require an admin API key:

```python
admin = AppDirectory("http://localhost:3003", api_key="ad_...")

# Approve/reject
admin.approve(app_id, note="Looks good")
admin.reject(app_id, reason="Missing documentation")

# Deprecation
admin.deprecate(app_id, reason="Replaced by v2", replacement_app_id="...", sunset_at="2025-12-31")
admin.undeprecate(app_id)

# Pending queue
pending = admin.pending(per_page=10)

# API keys
keys = admin.list_keys()
new_key = admin.create_key("Agent Key", is_admin=False, rate_limit=500)
admin.revoke_key(key_id)

# Webhooks
admin.create_webhook("https://example.com/hook", events=["app.approved"], secret="hmac-secret")
hooks = admin.list_webhooks()
admin.delete_webhook(webhook_id)
```

## Reviews

```python
# Submit (upserts — same caller updates existing)
ad.submit_review(app_id, rating=5, title="Great!", body="Works perfectly")

# List
reviews = ad.list_reviews(app_id, page=1, per_page=20)
```

## Stats & Trending

```python
stats = ad.app_stats(app_id)
print(f"Total views: {stats['total_views']}")

trending = ad.trending(days=7, limit=10)
```

## Health Checks

```python
result = ad.health_check(app_id)
history = ad.health_history(app_id)
summary = ad.health_summary()
schedule = ad.health_schedule()
```

## Error Handling

```python
from app_directory import (
    AppDirectoryError,   # Base
    ValidationError,     # 400/422
    AuthError,           # 401
    ForbiddenError,      # 403
    NotFoundError,       # 404
    ConflictError,       # 409
    RateLimitError,      # 429
    ServerError,         # 5xx
)

try:
    ad.get_app("nonexistent")
except NotFoundError as e:
    print(f"Not found: {e} (status={e.status_code})")
```

## Running Tests

```bash
# Local server
python test_sdk.py

# Staging
APP_DIRECTORY_URL=http://192.168.0.79:3003 python test_sdk.py -v
```
