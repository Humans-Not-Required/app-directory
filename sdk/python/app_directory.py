#!/usr/bin/env python3
"""
app_directory — Python SDK for HNR App Directory

Zero-dependency client library for the App Directory API.
Works with Python 3.8+ using only the standard library.

Quick start:
    from app_directory import AppDirectory

    ad = AppDirectory("http://localhost:3003")

    # Browse apps
    apps = ad.list_apps()
    for app in apps["apps"]:
        print(f"{app['name']} — {app['short_description']}")

    # Search
    results = ad.search("monitoring")

    # Submit an app (no auth required)
    submitted = ad.submit(
        name="My Service",
        short_description="A useful tool",
        description="Full description...",
        author_name="Alice",
    )
    print(f"Edit token: {submitted['edit_token']}")  # Save this!

    # Admin operations (require admin API key)
    admin = AppDirectory("http://localhost:3003", api_key="ad_...")
    admin.approve(app_id, note="Looks good")

Full docs: GET /api/v1/llms.txt or /.well-known/skills/app-directory/SKILL.md
"""

from __future__ import annotations

import json
import os
import urllib.error
import urllib.parse
import urllib.request
from typing import (
    Any,
    Dict,
    List,
    Optional,
    Union,
)


__version__ = "1.0.0"


# ---------------------------------------------------------------------------
# Exceptions
# ---------------------------------------------------------------------------


class AppDirectoryError(Exception):
    """Base exception for App Directory API errors."""

    def __init__(self, message: str, status_code: int = 0, body: Any = None):
        super().__init__(message)
        self.status_code = status_code
        self.body = body


class NotFoundError(AppDirectoryError):
    """Resource not found (404)."""
    pass


class AuthError(AppDirectoryError):
    """API key required or invalid (401)."""
    pass


class ForbiddenError(AppDirectoryError):
    """Insufficient permissions (403)."""
    pass


class ConflictError(AppDirectoryError):
    """Conflict — e.g. already approved, already deprecated (409)."""
    pass


class ValidationError(AppDirectoryError):
    """Invalid request parameters (400/422)."""
    pass


class RateLimitError(AppDirectoryError):
    """Rate limit exceeded (429)."""

    def __init__(self, message: str, status_code: int = 429, body: Any = None):
        super().__init__(message, status_code, body)


class ServerError(AppDirectoryError):
    """Internal server error (5xx)."""
    pass


# ---------------------------------------------------------------------------
# Client
# ---------------------------------------------------------------------------


class AppDirectory:
    """Client for the HNR App Directory API.

    Args:
        base_url: Service URL (default: ``$APP_DIRECTORY_URL`` or ``http://localhost:3003``).
        api_key: Admin API key for privileged operations (optional).
        edit_token: Default edit token for app updates (optional).
        timeout: HTTP timeout in seconds (default 30).
    """

    def __init__(
        self,
        base_url: Optional[str] = None,
        *,
        api_key: Optional[str] = None,
        edit_token: Optional[str] = None,
        timeout: int = 30,
    ):
        self.base_url = (
            base_url or os.environ.get("APP_DIRECTORY_URL") or "http://localhost:3003"
        ).rstrip("/")
        self.api_key = api_key or os.environ.get("APP_DIRECTORY_KEY")
        self.edit_token = edit_token
        self.timeout = timeout

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------

    def _request(
        self,
        method: str,
        path: str,
        *,
        json_body: Any = None,
        headers: Optional[Dict[str, str]] = None,
        query: Optional[Dict[str, Any]] = None,
        auth: bool = False,
        use_edit_token: Optional[str] = None,
    ) -> Any:
        url = f"{self.base_url}{path}"
        if query:
            filtered = {k: str(v) for k, v in query.items() if v is not None}
            if filtered:
                url += "?" + urllib.parse.urlencode(filtered)

        hdrs = dict(headers or {})
        body: Optional[bytes] = None

        if json_body is not None:
            body = json.dumps(json_body).encode()
            hdrs.setdefault("Content-Type", "application/json")

        # Auth
        if auth and self.api_key:
            hdrs["Authorization"] = f"Bearer {self.api_key}"
        elif use_edit_token:
            hdrs["X-Edit-Token"] = use_edit_token
        elif self.edit_token and method in ("PATCH", "DELETE"):
            hdrs["X-Edit-Token"] = self.edit_token

        req = urllib.request.Request(url, data=body, headers=hdrs, method=method)

        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as resp:
                ct = resp.headers.get("Content-Type", "")
                raw = resp.read()
                if "json" in ct:
                    return json.loads(raw)
                return raw
        except urllib.error.HTTPError as exc:
            self._raise_for_status(exc)

    def _raise_for_status(self, exc: urllib.error.HTTPError) -> None:
        status = exc.code
        try:
            body = json.loads(exc.read())
        except Exception:
            body = None

        msg = ""
        if isinstance(body, dict):
            msg = body.get("error", "") or body.get("message", "")
        if not msg:
            msg = f"HTTP {status}"

        if status == 401:
            raise AuthError(msg, status, body)
        if status == 403:
            raise ForbiddenError(msg, status, body)
        if status == 404:
            raise NotFoundError(msg, status, body)
        if status == 409:
            raise ConflictError(msg, status, body)
        if status == 429:
            raise RateLimitError(msg, status, body)
        if status in (400, 422):
            raise ValidationError(msg, status, body)
        if status >= 500:
            raise ServerError(msg, status, body)
        raise AppDirectoryError(msg, status, body)

    # ------------------------------------------------------------------
    # Health
    # ------------------------------------------------------------------

    def health(self) -> Dict[str, Any]:
        """``GET /api/v1/health`` — service health check."""
        return self._request("GET", "/api/v1/health")

    def is_healthy(self) -> bool:
        """Return ``True`` if the service is reachable and healthy."""
        try:
            h = self.health()
            return h.get("status") == "ok"
        except Exception:
            return False

    # ------------------------------------------------------------------
    # Apps — Browse & Search
    # ------------------------------------------------------------------

    def list_apps(
        self,
        *,
        category: Optional[str] = None,
        protocol: Optional[str] = None,
        status: Optional[str] = None,
        featured: Optional[bool] = None,
        verified: Optional[bool] = None,
        health: Optional[str] = None,
        sort: Optional[str] = None,
        page: Optional[int] = None,
        per_page: Optional[int] = None,
    ) -> Dict[str, Any]:
        """``GET /api/v1/apps`` — list apps with optional filters.

        Args:
            category: Filter by category (e.g. ``"data"``, ``"infrastructure"``).
            protocol: Filter by protocol (e.g. ``"rest"``, ``"grpc"``).
            status: Filter by status (``"pending"``, ``"approved"``, ``"rejected"``, ``"deprecated"``, ``"all"``).
            featured: Filter by featured badge.
            verified: Filter by verified badge.
            health: Filter by health status.
            sort: Sort order (``"name"``, ``"newest"``, ``"oldest"``, ``"rating"``).
            page: Page number (1-based).
            per_page: Results per page (default 20, max 100).

        Returns:
            Dict with ``apps`` list, ``total``, ``page``, ``per_page``.
        """
        query: Dict[str, Any] = {
            "category": category,
            "protocol": protocol,
            "status": status,
            "sort": sort,
            "page": page,
            "per_page": per_page,
        }
        if featured is not None:
            query["featured"] = str(featured).lower()
        if verified is not None:
            query["verified"] = str(verified).lower()
        if health is not None:
            query["health"] = health
        return self._request("GET", "/api/v1/apps", query=query)

    def get_app(self, id_or_slug: str) -> Dict[str, Any]:
        """``GET /api/v1/apps/{id_or_slug}`` — get app by ID or slug.

        Args:
            id_or_slug: App UUID or URL slug.

        Returns:
            Full app object.
        """
        return self._request("GET", f"/api/v1/apps/{id_or_slug}")

    def search(
        self,
        query: str,
        *,
        category: Optional[str] = None,
        protocol: Optional[str] = None,
        page: Optional[int] = None,
        per_page: Optional[int] = None,
    ) -> Dict[str, Any]:
        """``GET /api/v1/apps/search`` — full-text search across apps.

        Args:
            query: Search string.
            category: Filter by category.
            protocol: Filter by protocol.
            page: Page number.
            per_page: Results per page.

        Returns:
            Dict with ``apps`` list, ``total``, ``query``.
        """
        return self._request("GET", "/api/v1/apps/search", query={
            "q": query,
            "category": category,
            "protocol": protocol,
            "page": page,
            "per_page": per_page,
        })

    def my_apps(self) -> Dict[str, Any]:
        """``GET /api/v1/apps/mine`` — list apps submitted by the current API key.

        Requires API key auth.
        """
        return self._request("GET", "/api/v1/apps/mine", auth=True)

    def pending(
        self,
        *,
        page: Optional[int] = None,
        per_page: Optional[int] = None,
    ) -> Dict[str, Any]:
        """``GET /api/v1/apps/pending`` — list pending apps (admin only).

        Args:
            page: Page number.
            per_page: Results per page.
        """
        return self._request("GET", "/api/v1/apps/pending", auth=True, query={
            "page": page,
            "per_page": per_page,
        })

    # ------------------------------------------------------------------
    # Apps — Submit & Manage
    # ------------------------------------------------------------------

    def submit(
        self,
        name: str,
        short_description: str,
        description: str,
        author_name: str,
        *,
        homepage_url: Optional[str] = None,
        api_url: Optional[str] = None,
        api_spec_url: Optional[str] = None,
        protocol: Optional[str] = None,
        category: Optional[str] = None,
        tags: Optional[List[str]] = None,
        logo_url: Optional[str] = None,
        author_url: Optional[str] = None,
    ) -> Dict[str, Any]:
        """``POST /api/v1/apps`` — submit a new app. No auth required.

        Returns a dict with the app data and an ``edit_token`` — save it!

        Args:
            name: App name.
            short_description: Brief tagline.
            description: Full description.
            author_name: Author/organization name.
            homepage_url: Project homepage.
            api_url: API base URL.
            api_spec_url: OpenAPI/GraphQL spec URL.
            protocol: ``"rest"``, ``"graphql"``, ``"grpc"``, ``"mcp"``, ``"a2a"``, ``"websocket"``, ``"other"``.
            category: ``"communication"``, ``"data"``, ``"developer-tools"``, ``"finance"``, ``"media"``, etc.
            tags: List of tag strings.
            logo_url: Logo image URL.
            author_url: Author website.
        """
        body: Dict[str, Any] = {
            "name": name,
            "short_description": short_description,
            "description": description,
            "author_name": author_name,
        }
        if homepage_url is not None:
            body["homepage_url"] = homepage_url
        if api_url is not None:
            body["api_url"] = api_url
        if api_spec_url is not None:
            body["api_spec_url"] = api_spec_url
        if protocol is not None:
            body["protocol"] = protocol
        if category is not None:
            body["category"] = category
        if tags is not None:
            body["tags"] = tags
        if logo_url is not None:
            body["logo_url"] = logo_url
        if author_url is not None:
            body["author_url"] = author_url
        result = self._request("POST", "/api/v1/apps", json_body=body)
        # Normalize: API returns 'app_id', add 'id' alias for convenience
        if isinstance(result, dict) and "app_id" in result and "id" not in result:
            result["id"] = result["app_id"]
        return result

    def update_app(
        self,
        app_id: str,
        *,
        edit_token: Optional[str] = None,
        **fields: Any,
    ) -> Dict[str, Any]:
        """``PATCH /api/v1/apps/{id}`` — update app fields.

        Auth: edit token, owner API key, or admin API key.

        Args:
            app_id: App UUID.
            edit_token: Override edit token for this call.
            **fields: Fields to update (name, description, tags, etc.).
        """
        return self._request(
            "PATCH",
            f"/api/v1/apps/{app_id}",
            json_body=fields,
            auth=bool(self.api_key),
            use_edit_token=edit_token,
        )

    def delete_app(
        self,
        app_id: str,
        *,
        edit_token: Optional[str] = None,
    ) -> Dict[str, Any]:
        """``DELETE /api/v1/apps/{id}`` — delete an app and its reviews.

        Auth: edit token, owner API key, or admin API key.
        """
        return self._request(
            "DELETE",
            f"/api/v1/apps/{app_id}",
            auth=bool(self.api_key),
            use_edit_token=edit_token,
        )

    # ------------------------------------------------------------------
    # Admin — Approve / Reject / Deprecate
    # ------------------------------------------------------------------

    def approve(self, app_id: str, *, note: Optional[str] = None) -> Dict[str, Any]:
        """``POST /api/v1/apps/{id}/approve`` — approve a pending/rejected app (admin).

        Args:
            app_id: App UUID.
            note: Optional approval note.
        """
        body: Dict[str, Any] = {}
        if note is not None:
            body["note"] = note
        return self._request("POST", f"/api/v1/apps/{app_id}/approve", json_body=body, auth=True)

    def reject(self, app_id: str, reason: str) -> Dict[str, Any]:
        """``POST /api/v1/apps/{id}/reject`` — reject an app (admin).

        Args:
            app_id: App UUID.
            reason: Rejection reason (required, non-empty).
        """
        return self._request(
            "POST", f"/api/v1/apps/{app_id}/reject",
            json_body={"reason": reason}, auth=True,
        )

    def deprecate(
        self,
        app_id: str,
        reason: str,
        *,
        replacement_app_id: Optional[str] = None,
        sunset_at: Optional[str] = None,
    ) -> Dict[str, Any]:
        """``POST /api/v1/apps/{id}/deprecate`` — deprecate an app (admin).

        Args:
            app_id: App UUID.
            reason: Deprecation reason (required).
            replacement_app_id: Suggested replacement app ID.
            sunset_at: ISO-8601 date when app stops working.
        """
        body: Dict[str, Any] = {"reason": reason}
        if replacement_app_id is not None:
            body["replacement_app_id"] = replacement_app_id
        if sunset_at is not None:
            body["sunset_at"] = sunset_at
        return self._request(
            "POST", f"/api/v1/apps/{app_id}/deprecate",
            json_body=body, auth=True,
        )

    def undeprecate(self, app_id: str) -> Dict[str, Any]:
        """``POST /api/v1/apps/{id}/undeprecate`` — restore deprecated app (admin)."""
        return self._request(
            "POST", f"/api/v1/apps/{app_id}/undeprecate", auth=True,
            json_body={},
        )

    # ------------------------------------------------------------------
    # Reviews
    # ------------------------------------------------------------------

    def submit_review(
        self,
        app_id: str,
        rating: int,
        *,
        title: Optional[str] = None,
        body: Optional[str] = None,
        reviewer_name: Optional[str] = None,
    ) -> Dict[str, Any]:
        """``POST /api/v1/apps/{id}/reviews`` — submit or update a review.

        Authenticated reviews are upserted (same API key updates existing).
        Anonymous reviews always create new entries.

        Args:
            app_id: App UUID.
            rating: 1–5 stars.
            title: Review title.
            body: Review text.
            reviewer_name: Display name for the reviewer (defaults to "anonymous").
        """
        payload: Dict[str, Any] = {"rating": rating}
        if title is not None:
            payload["title"] = title
        if reviewer_name is not None:
            payload["reviewer_name"] = reviewer_name
        if body is not None:
            payload["body"] = body
        return self._request("POST", f"/api/v1/apps/{app_id}/reviews", json_body=payload)

    def list_reviews(
        self,
        app_id: str,
        *,
        page: Optional[int] = None,
        per_page: Optional[int] = None,
    ) -> Dict[str, Any]:
        """``GET /api/v1/apps/{id}/reviews`` — list reviews for an app.

        Args:
            app_id: App UUID.
            page: Page number.
            per_page: Results per page.
        """
        return self._request("GET", f"/api/v1/apps/{app_id}/reviews", query={
            "page": page,
            "per_page": per_page,
        })

    # ------------------------------------------------------------------
    # Categories
    # ------------------------------------------------------------------

    def categories(self) -> Dict[str, Any]:
        """``GET /api/v1/categories`` — list categories with app counts."""
        return self._request("GET", "/api/v1/categories")

    # ------------------------------------------------------------------
    # Stats
    # ------------------------------------------------------------------

    def app_stats(self, app_id: str) -> Dict[str, Any]:
        """``GET /api/v1/apps/{id}/stats`` — view counts and unique viewers.

        Args:
            app_id: App UUID or slug.
        """
        return self._request("GET", f"/api/v1/apps/{app_id}/stats")

    def trending(
        self,
        *,
        days: Optional[int] = None,
        limit: Optional[int] = None,
    ) -> List[Dict[str, Any]]:
        """``GET /api/v1/apps/trending`` — trending apps by recent views.

        Args:
            days: Time period (1–90, default 7).
            limit: Max results (1–50, default 10).
        """
        return self._request("GET", "/api/v1/apps/trending", query={
            "days": days,
            "limit": limit,
        })

    # ------------------------------------------------------------------
    # Health Checks
    # ------------------------------------------------------------------

    def health_check(self, app_id: str) -> Dict[str, Any]:
        """``POST /api/v1/apps/{id}/health-check`` — trigger a health check.

        Args:
            app_id: App UUID.
        """
        return self._request("POST", f"/api/v1/apps/{app_id}/health-check", json_body={}, auth=True)

    def health_check_batch(self, app_ids: Optional[List[str]] = None) -> Dict[str, Any]:
        """``POST /api/v1/apps/health-check/batch`` — batch health check.

        Args:
            app_ids: Optional list of app IDs (checks all if omitted).
        """
        body: Dict[str, Any] = {}
        if app_ids is not None:
            body["app_ids"] = app_ids
        return self._request("POST", "/api/v1/apps/health-check/batch", json_body=body, auth=True)

    def health_history(
        self,
        app_id: str,
        *,
        page: Optional[int] = None,
        per_page: Optional[int] = None,
    ) -> Dict[str, Any]:
        """``GET /api/v1/apps/{id}/health`` — health check history.

        Args:
            app_id: App UUID.
            page: Page number.
            per_page: Results per page.
        """
        return self._request("GET", f"/api/v1/apps/{app_id}/health", query={
            "page": page,
            "per_page": per_page,
        })

    def health_summary(self) -> Dict[str, Any]:
        """``GET /api/v1/apps/health/summary`` — overall health status summary."""
        return self._request("GET", "/api/v1/apps/health/summary")

    def health_schedule(self) -> Dict[str, Any]:
        """``GET /api/v1/health-check/schedule`` — health check scheduler info (admin)."""
        return self._request("GET", "/api/v1/health-check/schedule", auth=True)

    # ------------------------------------------------------------------
    # Webhooks
    # ------------------------------------------------------------------

    def create_webhook(
        self,
        url: str,
        *,
        events: Optional[List[str]] = None,
        secret: Optional[str] = None,
    ) -> Dict[str, Any]:
        """``POST /api/v1/webhooks`` — register a webhook (admin).

        Args:
            url: Webhook destination URL.
            events: Event types to subscribe to (default: all).
            secret: HMAC-SHA256 signing secret.
        """
        body: Dict[str, Any] = {"url": url}
        if events is not None:
            body["events"] = events
        if secret is not None:
            body["secret"] = secret
        return self._request("POST", "/api/v1/webhooks", json_body=body, auth=True)

    def list_webhooks(self) -> Any:
        """``GET /api/v1/webhooks`` — list registered webhooks (admin)."""
        return self._request("GET", "/api/v1/webhooks", auth=True)

    def delete_webhook(self, webhook_id: str) -> Dict[str, Any]:
        """``DELETE /api/v1/webhooks/{id}`` — delete a webhook (admin)."""
        return self._request("DELETE", f"/api/v1/webhooks/{webhook_id}", auth=True)

    # ------------------------------------------------------------------
    # API Keys (Admin)
    # ------------------------------------------------------------------

    def list_keys(self) -> Any:
        """``GET /api/v1/keys`` — list API keys (admin)."""
        return self._request("GET", "/api/v1/keys", auth=True)

    def create_key(
        self,
        name: str,
        *,
        is_admin: Optional[bool] = None,
        rate_limit: Optional[int] = None,
    ) -> Dict[str, Any]:
        """``POST /api/v1/keys`` — create an API key (admin).

        Args:
            name: Key name.
            is_admin: Whether this is an admin key.
            rate_limit: Custom rate limit.
        """
        body: Dict[str, Any] = {"name": name}
        if is_admin is not None:
            body["is_admin"] = is_admin
        if rate_limit is not None:
            body["rate_limit"] = rate_limit
        return self._request("POST", "/api/v1/keys", json_body=body, auth=True)

    def revoke_key(self, key_id: str) -> Dict[str, Any]:
        """``DELETE /api/v1/keys/{id}`` — revoke an API key (admin)."""
        return self._request("DELETE", f"/api/v1/keys/{key_id}", auth=True)

    # ------------------------------------------------------------------
    # Discovery
    # ------------------------------------------------------------------

    def llms_txt(self) -> str:
        """``GET /api/v1/llms.txt`` — AI-readable service documentation."""
        data = self._request("GET", "/api/v1/llms.txt")
        return data.decode() if isinstance(data, bytes) else str(data)

    def openapi(self) -> Dict[str, Any]:
        """``GET /api/v1/openapi.json`` — OpenAPI 3.0 specification."""
        return self._request("GET", "/api/v1/openapi.json")

    def skills(self) -> Dict[str, Any]:
        """``GET /.well-known/skills/index.json`` — Cloudflare RFC skill discovery."""
        return self._request("GET", "/.well-known/skills/index.json")

    def skill_md(self) -> str:
        """``GET /.well-known/skills/app-directory/SKILL.md`` — agent integration guide."""
        data = self._request("GET", "/.well-known/skills/app-directory/SKILL.md")
        return data.decode() if isinstance(data, bytes) else str(data)

    def llms_txt_root(self) -> str:
        """``GET /llms.txt`` — root-level AI-readable API summary."""
        data = self._request("GET", "/llms.txt")
        return data.decode() if isinstance(data, bytes) else str(data)

    def skill_md_v1(self) -> str:
        """``GET /api/v1/skills/SKILL.md`` — API-level skill discovery."""
        data = self._request("GET", "/api/v1/skills/SKILL.md")
        return data.decode() if isinstance(data, bytes) else str(data)

    # ------------------------------------------------------------------
    # Convenience
    # ------------------------------------------------------------------

    def find_by_name(self, name: str) -> Optional[Dict[str, Any]]:
        """Search for an app by exact name. Returns the first match or None."""
        results = self.search(name)
        for app in results.get("apps", []):
            if app["name"].lower() == name.lower():
                return app
        return None

    def __repr__(self) -> str:
        return f"AppDirectory(base_url={self.base_url!r})"
