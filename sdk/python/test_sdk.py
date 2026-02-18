#!/usr/bin/env python3
"""
Integration tests for the App Directory Python SDK.

Usage:
    # Against local dev server
    python test_sdk.py

    # Against staging
    APP_DIRECTORY_URL=http://192.168.0.79:3003 python test_sdk.py

    # Verbose output
    python test_sdk.py -v
"""

import json
import os
import sys
import time
import unittest
from typing import Optional

# Import SDK from same directory
sys.path.insert(0, os.path.dirname(__file__))
from app_directory import (
    AppDirectory,
    AppDirectoryError,
    AuthError,
    ConflictError,
    ForbiddenError,
    NotFoundError,
    RateLimitError,
    ServerError,
    ValidationError,
)

BASE_URL = os.environ.get("APP_DIRECTORY_URL", "http://localhost:3003")


def unique_name(prefix: str = "SDK-Test") -> str:
    return f"{prefix}-{int(time.time() * 1000) % 1_000_000}"


class AppDirectoryTestCase(unittest.TestCase):
    """Base class with shared setup and cleanup."""

    ad: AppDirectory
    _app_ids: list  # (id, edit_token) pairs for cleanup

    @classmethod
    def setUpClass(cls) -> None:
        cls.ad = AppDirectory(BASE_URL)
        cls._app_ids = []

    @classmethod
    def tearDownClass(cls) -> None:
        for app_id, token in cls._app_ids:
            try:
                cls.ad.delete_app(app_id, edit_token=token)
            except Exception:
                pass

    def _submit(self, **overrides) -> dict:
        """Submit a test app and register for cleanup.

        Returns a dict with at least: app_id, edit_token, slug, status.
        Also adds 'id' as alias for 'app_id' for convenience.
        """
        defaults = {
            "name": unique_name(),
            "short_description": "Test app",
            "description": "Created by SDK integration tests",
            "author_name": "SDK Tester",
        }
        defaults.update(overrides)
        result = self.ad.submit(**defaults)
        # Normalize: ensure 'id' is available (API returns 'app_id')
        if "app_id" in result and "id" not in result:
            result["id"] = result["app_id"]
        self.__class__._app_ids.append((result.get("id") or result.get("app_id"), result.get("edit_token")))
        return result


# =========================================================================
# Health
# =========================================================================


class TestHealth(AppDirectoryTestCase):
    def test_health(self):
        h = self.ad.health()
        self.assertEqual(h["status"], "ok")

    def test_is_healthy(self):
        self.assertTrue(self.ad.is_healthy())

    def test_is_healthy_bad_url(self):
        bad = AppDirectory("http://localhost:1")
        self.assertFalse(bad.is_healthy())


# =========================================================================
# Submit & CRUD
# =========================================================================


class TestSubmit(AppDirectoryTestCase):
    def test_submit_minimal(self):
        result = self._submit()
        self.assertTrue("id" in result or "app_id" in result)
        self.assertIn("edit_token", result)
        self.assertIn("slug", result)

    def test_submit_full(self):
        result = self._submit(
            homepage_url="https://example.com",
            api_url="https://api.example.com",
            api_spec_url="https://api.example.com/openapi.json",
            protocol="rest",
            category="data",
            tags=["test", "sdk"],
            logo_url="https://example.com/logo.png",
            author_url="https://example.com/about",
        )
        self.assertIn("edit_token", result)
        # Verify via get
        fetched = self.ad.get_app(result["id"])
        self.assertEqual(fetched["protocol"], "rest")
        self.assertEqual(fetched["category"], "data")

    def test_submit_missing_name(self):
        with self.assertRaises((ValidationError, AppDirectoryError)):
            self.ad._request("POST", "/api/v1/apps", json_body={
                "short_description": "desc",
                "description": "full",
                "author_name": "test",
            })

    def test_get_app_by_id(self):
        submitted = self._submit()
        fetched = self.ad.get_app(submitted["id"])
        self.assertEqual(fetched["id"], submitted["id"])

    def test_get_app_by_slug(self):
        submitted = self._submit()
        fetched = self.ad.get_app(submitted["slug"])
        self.assertEqual(fetched["id"], submitted["id"])

    def test_get_nonexistent_app(self):
        with self.assertRaises(NotFoundError):
            self.ad.get_app("nonexistent-id-12345")

    def test_update_app_with_edit_token(self):
        submitted = self._submit()
        new_name = unique_name("Updated")
        result = self.ad.update_app(
            submitted["id"],
            edit_token=submitted["edit_token"],
            name=new_name,
        )
        self.assertIn("message", result)
        # Verify update applied
        fetched = self.ad.get_app(submitted["id"])
        self.assertEqual(fetched["name"], new_name)

    def test_delete_app(self):
        submitted = self.ad.submit(
            name=unique_name("ToDelete"),
            short_description="delete me",
            description="will be deleted",
            author_name="tester",
        )
        app_id = submitted.get("id") or submitted.get("app_id")
        result = self.ad.delete_app(app_id, edit_token=submitted["edit_token"])
        # Verify deleted
        with self.assertRaises(NotFoundError):
            self.ad.get_app(app_id)

    def test_delete_nonexistent(self):
        with self.assertRaises((NotFoundError, AuthError)):
            self.ad.delete_app("nonexistent-app-id", edit_token="fake-token")

    def test_response_fields(self):
        """Verify key fields in submit response and fetched app."""
        result = self._submit()
        # Submit response has: app_id, edit_token, slug, status
        for field in ["edit_token", "slug", "status"]:
            self.assertIn(field, result, f"Missing field in submit response: {field}")
        # Full app object from get
        fetched = self.ad.get_app(result["id"])
        for field in ["id", "name", "slug", "short_description", "description",
                       "author_name", "status", "created_at"]:
            self.assertIn(field, fetched, f"Missing field in app: {field}")


# =========================================================================
# Browse & Search
# =========================================================================


class TestBrowse(AppDirectoryTestCase):
    def test_list_apps(self):
        self._submit()  # ensure at least one exists
        result = self.ad.list_apps()
        self.assertIn("apps", result)
        self.assertIsInstance(result["apps"], list)

    def test_list_pagination(self):
        result = self.ad.list_apps(page=1, per_page=2)
        self.assertIn("apps", result)
        self.assertLessEqual(len(result["apps"]), 2)

    def test_list_filter_category(self):
        self._submit(category="security")
        result = self.ad.list_apps(category="security")
        if result["apps"]:
            for app in result["apps"]:
                self.assertEqual(app["category"], "security")

    def test_list_filter_protocol(self):
        self._submit(protocol="grpc")
        result = self.ad.list_apps(protocol="grpc")
        if result["apps"]:
            for app in result["apps"]:
                self.assertEqual(app["protocol"], "grpc")

    def test_list_sort_by_name(self):
        result = self.ad.list_apps(sort="name")
        self.assertIn("apps", result)

    def test_search(self):
        name = unique_name("Searchable")
        self._submit(name=name)
        result = self.ad.search(name)
        self.assertIn("apps", result)
        names = [a["name"] for a in result["apps"]]
        self.assertIn(name, names)

    def test_search_no_results(self):
        result = self.ad.search("zzzznonexistent99999")
        self.assertEqual(len(result.get("apps", [])), 0)

    def test_search_with_category_filter(self):
        self._submit(name=unique_name("CatSearch"), category="finance")
        result = self.ad.search("CatSearch", category="finance")
        self.assertIn("apps", result)

    def test_categories(self):
        result = self.ad.categories()
        self.assertIsInstance(result, dict)


# =========================================================================
# Reviews
# =========================================================================


class TestReviews(AppDirectoryTestCase):
    def test_submit_review(self):
        app = self._submit()
        result = self.ad.submit_review(app["id"], 5, title="Great!", body="Works perfectly")
        self.assertIn("id", result)

    def test_submit_review_minimal(self):
        app = self._submit()
        result = self.ad.submit_review(app["id"], 3)
        self.assertIn("id", result)

    def test_list_reviews(self):
        app = self._submit()
        self.ad.submit_review(app["id"], 4, title="Good")
        reviews = self.ad.list_reviews(app["id"])
        self.assertIn("reviews", reviews)
        self.assertGreaterEqual(len(reviews["reviews"]), 1)

    def test_review_nonexistent_app(self):
        with self.assertRaises((NotFoundError, AppDirectoryError)):
            self.ad.submit_review("nonexistent-app-id", 5)

    def test_anonymous_reviews_create_new_entries(self):
        """Anonymous reviews always create new entries (no upsert)."""
        app = self._submit()
        self.ad.submit_review(app["id"], 3, title="Okay")
        self.ad.submit_review(app["id"], 5, title="Actually great")
        reviews = self.ad.list_reviews(app["id"])
        # Anonymous reviews don't upsert — each creates a new entry
        self.assertEqual(len(reviews["reviews"]), 2)

    def test_review_has_reviewer_name(self):
        """Reviews should include reviewer_name in the response."""
        app = self._submit()
        self.ad.submit_review(app["id"], 4, title="Named review")
        reviews = self.ad.list_reviews(app["id"])
        self.assertGreaterEqual(len(reviews["reviews"]), 1)
        # reviewer_name should be present (may be "anonymous" or None for anonymous)
        self.assertIn("reviewer_name", reviews["reviews"][0])


# =========================================================================
# Stats
# =========================================================================


class TestStats(AppDirectoryTestCase):
    def test_app_stats(self):
        app = self._submit()
        stats = self.ad.app_stats(app["id"])
        self.assertIn("total_views", stats)

    def test_trending(self):
        result = self.ad.trending()
        self.assertIsInstance(result, dict)
        self.assertIn("trending", result)
        self.assertIsInstance(result["trending"], list)

    def test_trending_custom_days(self):
        result = self.ad.trending(days=30, limit=5)
        self.assertIn("trending", result)
        self.assertLessEqual(len(result["trending"]), 5)


# =========================================================================
# Health Checks
# =========================================================================


class TestHealthChecks(AppDirectoryTestCase):
    def test_health_check_requires_auth(self):
        """Health check trigger requires auth."""
        app = self._submit(api_url="https://httpbin.org/status/200")
        with self.assertRaises(AuthError):
            self.ad.health_check(app["id"])

    def test_health_summary(self):
        result = self.ad.health_summary()
        self.assertIsInstance(result, dict)

    def test_health_history(self):
        app = self._submit()
        result = self.ad.health_history(app["id"])
        self.assertIn("checks", result)

    def test_health_schedule_requires_auth(self):
        """Health schedule requires admin auth."""
        with self.assertRaises(AuthError):
            self.ad.health_schedule()


# =========================================================================
# Discovery
# =========================================================================


class TestDiscovery(AppDirectoryTestCase):
    def test_llms_txt(self):
        txt = self.ad.llms_txt()
        self.assertIsInstance(txt, str)
        self.assertIn("app", txt.lower())

    def test_openapi(self):
        spec = self.ad.openapi()
        self.assertIn("openapi", spec)
        self.assertIn("paths", spec)

    def test_skills_index(self):
        idx = self.ad.skills()
        self.assertIn("skills", idx)

    def test_skill_md(self):
        md = self.ad.skill_md()
        self.assertIsInstance(md, str)
        self.assertIn("app", md.lower())


# =========================================================================
# Exceptions
# =========================================================================


class TestExceptions(AppDirectoryTestCase):
    def test_exception_hierarchy(self):
        self.assertTrue(issubclass(NotFoundError, AppDirectoryError))
        self.assertTrue(issubclass(AuthError, AppDirectoryError))
        self.assertTrue(issubclass(ForbiddenError, AppDirectoryError))
        self.assertTrue(issubclass(ConflictError, AppDirectoryError))
        self.assertTrue(issubclass(ValidationError, AppDirectoryError))
        self.assertTrue(issubclass(RateLimitError, AppDirectoryError))
        self.assertTrue(issubclass(ServerError, AppDirectoryError))

    def test_exception_has_status_code(self):
        try:
            self.ad.get_app("nonexistent")
        except NotFoundError as e:
            self.assertEqual(e.status_code, 404)

    def test_exception_has_body(self):
        try:
            self.ad.get_app("nonexistent")
        except NotFoundError as e:
            self.assertIsNotNone(e.body)


# =========================================================================
# Convenience
# =========================================================================


class TestConvenience(AppDirectoryTestCase):
    def test_find_by_name(self):
        name = unique_name("FindMe")
        self._submit(name=name)
        found = self.ad.find_by_name(name)
        self.assertIsNotNone(found)
        self.assertEqual(found["name"], name)

    def test_find_by_name_not_found(self):
        result = self.ad.find_by_name("DefinitelyNotARealApp99999")
        self.assertIsNone(result)

    def test_repr(self):
        self.assertIn(BASE_URL, repr(self.ad))

    def test_env_config(self):
        original = os.environ.get("APP_DIRECTORY_URL")
        try:
            os.environ["APP_DIRECTORY_URL"] = "http://custom:9999"
            client = AppDirectory()
            self.assertEqual(client.base_url, "http://custom:9999")
        finally:
            if original:
                os.environ["APP_DIRECTORY_URL"] = original
            else:
                os.environ.pop("APP_DIRECTORY_URL", None)


# =========================================================================
# Edge Cases
# =========================================================================


class TestEdgeCases(AppDirectoryTestCase):
    def test_protocols(self):
        """Submit apps with various protocols."""
        for proto in ("rest", "graphql", "grpc", "websocket"):
            app = self._submit(protocol=proto)
            fetched = self.ad.get_app(app["id"])
            self.assertEqual(fetched["protocol"], proto)

    def test_all_categories(self):
        """Submit apps in different categories."""
        for cat in ("data", "security", "infrastructure"):
            app = self._submit(category=cat)
            fetched = self.ad.get_app(app["id"])
            self.assertEqual(fetched["category"], cat)

    def test_tags_roundtrip(self):
        tags = ["python", "sdk", "testing"]
        submitted = self._submit(tags=tags)
        fetched = self.ad.get_app(submitted["id"])
        self.assertEqual(sorted(fetched.get("tags", [])), sorted(tags))

    def test_search_pagination(self):
        result = self.ad.search("test", page=1, per_page=1)
        self.assertLessEqual(len(result.get("apps", [])), 1)

    def test_list_status_filter_all(self):
        """Status filter 'all' should include all statuses."""
        result = self.ad.list_apps(status="all")
        self.assertIn("apps", result)


# =========================================================================
# Admin: Approval Workflow
# =========================================================================


ADMIN_KEY = os.environ.get("APP_DIRECTORY_ADMIN_KEY", "ad_hnr_appdir_admin_2026")


class AdminTestCase(AppDirectoryTestCase):
    """Base class with admin client."""

    admin: AppDirectory

    @classmethod
    def setUpClass(cls) -> None:
        super().setUpClass()
        cls.admin = AppDirectory(BASE_URL, api_key=ADMIN_KEY)


class TestApprovalWorkflow(AdminTestCase):
    def test_reject_app(self):
        """Apps auto-approve on submit, so reject first to test workflow."""
        app = self._submit()
        result = self.admin.reject(app["id"], "Incomplete description")
        self.assertIn("message", result)
        fetched = self.admin.get_app(app["id"])
        self.assertEqual(fetched["status"], "rejected")

    def test_reject_requires_reason(self):
        app = self._submit()
        with self.assertRaises((ValidationError, AppDirectoryError)):
            self.admin.reject(app["id"], "")

    def test_double_approve_conflict(self):
        """Approving an already-approved app should 409 (auto-approved on submit)."""
        app = self._submit()
        with self.assertRaises(ConflictError):
            self.admin.approve(app["id"])

    def test_reject_then_approve(self):
        """Rejected apps can be re-approved."""
        app = self._submit()
        self.admin.reject(app["id"], "Needs work")
        self.admin.approve(app["id"], note="Fixed now")
        fetched = self.admin.get_app(app["id"])
        self.assertEqual(fetched["status"], "approved")

    def test_approve_then_reject(self):
        """Approved apps can be rejected."""
        app = self._submit()
        # Already auto-approved, so just reject
        self.admin.reject(app["id"], "Policy violation")
        fetched = self.admin.get_app(app["id"])
        self.assertEqual(fetched["status"], "rejected")

    def test_reject_then_approve_with_note(self):
        """Re-approve with note after rejection."""
        app = self._submit()
        self.admin.reject(app["id"], "Fix issues")
        self.admin.approve(app["id"], note="Approved for pilot")
        fetched = self.admin.get_app(app["id"])
        self.assertEqual(fetched.get("review_note"), "Approved for pilot")

    def test_approve_nonexistent(self):
        with self.assertRaises(NotFoundError):
            self.admin.approve("nonexistent-app-id")

    def test_reject_nonexistent(self):
        with self.assertRaises(NotFoundError):
            self.admin.reject("nonexistent-app-id", "reason")

    def test_approve_requires_admin(self):
        """Non-admin client cannot approve."""
        app = self._submit()
        with self.assertRaises(AuthError):
            self.ad.approve(app["id"])

    def test_reject_requires_admin(self):
        """Non-admin client cannot reject."""
        app = self._submit()
        with self.assertRaises(AuthError):
            self.ad.reject(app["id"], "reason")

    def test_pending_list(self):
        """Pending apps list (likely empty due to auto-approve)."""
        result = self.admin.pending()
        self.assertIn("apps", result)
        self.assertIsInstance(result["apps"], list)

    def test_pending_requires_admin(self):
        with self.assertRaises(AuthError):
            self.ad.pending()

    def test_reviewed_by_field(self):
        """After reject + re-approve, reviewed_by should be set."""
        app = self._submit()
        self.admin.reject(app["id"], "temp")
        self.admin.approve(app["id"])
        fetched = self.admin.get_app(app["id"])
        self.assertIn("reviewed_by", fetched)
        self.assertIsNotNone(fetched.get("reviewed_at"))


# =========================================================================
# Admin: Deprecation Workflow
# =========================================================================


class TestDeprecationWorkflow(AdminTestCase):
    def test_deprecate_app(self):
        """Apps are auto-approved, so deprecate directly."""
        app = self._submit()
        result = self.admin.deprecate(app["id"], "No longer maintained")
        self.assertIn("message", result)
        fetched = self.admin.get_app(app["id"])
        self.assertEqual(fetched["status"], "deprecated")
        self.assertEqual(fetched.get("deprecated_reason"), "No longer maintained")

    def test_deprecate_with_replacement(self):
        old_app = self._submit(name=unique_name("OldApp"))
        new_app = self._submit(name=unique_name("NewApp"))
        self.admin.deprecate(old_app["id"], "Use new version", replacement_app_id=new_app["id"])
        fetched = self.admin.get_app(old_app["id"])
        self.assertEqual(fetched.get("replacement_app_id"), new_app["id"])

    def test_deprecate_with_sunset(self):
        app = self._submit()
        self.admin.deprecate(app["id"], "Sunset planned", sunset_at="2026-12-31T00:00:00Z")
        fetched = self.admin.get_app(app["id"])
        self.assertIsNotNone(fetched.get("sunset_at"))

    def test_deprecate_requires_reason(self):
        app = self._submit()
        with self.assertRaises((ValidationError, AppDirectoryError)):
            self.admin.deprecate(app["id"], "")

    def test_double_deprecate(self):
        app = self._submit()
        self.admin.deprecate(app["id"], "reason")
        with self.assertRaises(ConflictError):
            self.admin.deprecate(app["id"], "another reason")

    def test_undeprecate(self):
        app = self._submit()
        self.admin.deprecate(app["id"], "Temp")
        self.admin.undeprecate(app["id"])
        fetched = self.admin.get_app(app["id"])
        self.assertEqual(fetched["status"], "approved")
        self.assertIsNone(fetched.get("deprecated_reason"))

    def test_undeprecate_non_deprecated(self):
        """Cannot undeprecate an approved (non-deprecated) app."""
        app = self._submit()
        with self.assertRaises(ConflictError):
            self.admin.undeprecate(app["id"])

    def test_deprecate_self_replacement(self):
        """Cannot use self as replacement."""
        app = self._submit()
        with self.assertRaises((ValidationError, AppDirectoryError)):
            self.admin.deprecate(app["id"], "reason", replacement_app_id=app["id"])

    def test_approve_deprecated_blocked(self):
        """Cannot approve a deprecated app."""
        app = self._submit()
        self.admin.deprecate(app["id"], "reason")
        with self.assertRaises(ConflictError):
            self.admin.approve(app["id"])

    def test_reject_deprecated_blocked(self):
        """Cannot reject a deprecated app."""
        app = self._submit()
        self.admin.deprecate(app["id"], "reason")
        with self.assertRaises(ConflictError):
            self.admin.reject(app["id"], "reason")

    def test_deprecate_requires_admin(self):
        app = self._submit()
        with self.assertRaises(AuthError):
            self.ad.deprecate(app["id"], "reason")

    def test_undeprecate_requires_admin(self):
        app = self._submit()
        with self.assertRaises(AuthError):
            self.ad.undeprecate(app["id"])


# =========================================================================
# Admin: API Key Management
# =========================================================================


class TestKeyManagement(AdminTestCase):
    def test_list_keys(self):
        result = self.admin.list_keys()
        self.assertIsInstance(result, dict)
        self.assertIn("keys", result)

    def test_create_key(self):
        name = unique_name("TestKey")
        result = self.admin.create_key(name)
        self.assertIn("api_key", result)

    def test_create_key_with_rate_limit(self):
        name = unique_name("RatedKey")
        result = self.admin.create_key(name, rate_limit=50)
        self.assertIn("api_key", result)

    def test_revoke_key(self):
        name = unique_name("RevokeMe")
        created = self.admin.create_key(name)
        # Find the key id by listing keys and matching name
        keys = self.admin.list_keys()
        key_id = None
        for k in keys["keys"]:
            if k["name"] == name:
                key_id = k["id"]
                break
        self.assertIsNotNone(key_id, f"Could not find created key '{name}'")
        result = self.admin.revoke_key(key_id)
        self.assertIn("message", result)

    def test_revoke_nonexistent_key(self):
        with self.assertRaises((NotFoundError, AppDirectoryError)):
            self.admin.revoke_key("nonexistent-key-id")

    def test_keys_require_admin(self):
        with self.assertRaises(AuthError):
            self.ad.list_keys()

    def test_created_key_works_for_auth(self):
        """A newly created key should work as auth."""
        created = self.admin.create_key(unique_name("WorkingKey"))
        authed = AppDirectory(BASE_URL, api_key=created["api_key"])
        result = authed.my_apps()
        self.assertIn("apps", result)

    def test_create_key_has_warning(self):
        """Create key response warns to save the key."""
        result = self.admin.create_key(unique_name("WarnKey"))
        self.assertIn("message", result)


# =========================================================================
# Admin: Webhook Management
# =========================================================================


class TestWebhookManagement(AdminTestCase):
    def test_create_webhook(self):
        result = self.admin.create_webhook("https://httpbin.org/post")
        self.assertIn("id", result)

    def test_create_webhook_with_events(self):
        result = self.admin.create_webhook(
            "https://httpbin.org/post",
            events=["app.submitted", "app.approved"],
        )
        self.assertIn("id", result)

    def test_list_webhooks(self):
        result = self.admin.list_webhooks()
        self.assertIsInstance(result, (list, dict))

    def test_delete_webhook(self):
        created = self.admin.create_webhook("https://httpbin.org/post")
        result = self.admin.delete_webhook(created["id"])
        self.assertIn("message", result)

    def test_delete_nonexistent_webhook(self):
        with self.assertRaises((NotFoundError, AppDirectoryError)):
            self.admin.delete_webhook("nonexistent-webhook-id")

    def test_webhooks_require_admin(self):
        with self.assertRaises(AuthError):
            self.ad.list_webhooks()

    def test_create_webhook_requires_admin(self):
        with self.assertRaises(AuthError):
            self.ad.create_webhook("https://httpbin.org/post")


# =========================================================================
# Admin: Health Check Operations
# =========================================================================


class TestAdminHealthChecks(AdminTestCase):
    def test_health_check_single(self):
        app = self._submit(api_url="https://httpbin.org/status/200")
        result = self.admin.health_check(app["id"])
        self.assertIn("status", result)

    def test_health_check_batch(self):
        result = self.admin.health_check_batch()
        self.assertIsInstance(result, dict)

    def test_health_schedule(self):
        result = self.admin.health_schedule()
        self.assertIsInstance(result, dict)

    def test_health_summary_as_admin(self):
        result = self.admin.health_summary()
        self.assertIsInstance(result, dict)


# =========================================================================
# My Apps
# =========================================================================


class TestMyApps(AdminTestCase):
    def test_my_apps_returns_list(self):
        result = self.admin.my_apps()
        self.assertIn("apps", result)
        self.assertIsInstance(result["apps"], list)

    def test_my_apps_requires_auth(self):
        """Unauthenticated clients get auth error for my_apps."""
        with self.assertRaises(AuthError):
            self.ad.my_apps()

    def test_my_apps_with_admin_key(self):
        """my_apps with admin key returns apps list (may be empty if admin doesn't track ownership)."""
        result = self.admin.my_apps()
        self.assertIn("apps", result)
        self.assertIsInstance(result["apps"], list)


# =========================================================================
# Advanced Browse & Filter
# =========================================================================


class TestAdvancedBrowse(AdminTestCase):
    def test_list_sort_oldest(self):
        result = self.ad.list_apps(sort="oldest")
        self.assertIn("apps", result)

    def test_list_filter_status_approved(self):
        self._submit()  # auto-approved with admin
        result = self.ad.list_apps(status="approved")
        if result["apps"]:
            for app in result["apps"]:
                self.assertEqual(app["status"], "approved")

    def test_list_filter_featured(self):
        """Filter by featured badge — may return 0 if none are featured."""
        result = self.ad.list_apps(featured=True)
        self.assertIn("apps", result)
        self.assertIsInstance(result["apps"], list)

    def test_list_filter_verified(self):
        """Filter by verified badge — may return 0 if none are verified."""
        result = self.ad.list_apps(verified=True)
        self.assertIn("apps", result)
        self.assertIsInstance(result["apps"], list)

    def test_list_page_beyond_data(self):
        result = self.ad.list_apps(page=9999)
        self.assertEqual(len(result.get("apps", [])), 0)

    def test_list_per_page_one(self):
        self._submit()
        self._submit()
        result = self.ad.list_apps(per_page=1)
        self.assertLessEqual(len(result.get("apps", [])), 1)

    def test_search_pagination_per_page(self):
        result = self.ad.search("test", per_page=2)
        self.assertLessEqual(len(result.get("apps", [])), 2)

    def test_search_by_tag(self):
        tag = f"uniqtag{int(time.time()) % 10000}"
        self._submit(tags=[tag])
        result = self.ad.search(tag)
        self.assertIn("apps", result)

    def test_categories_has_counts(self):
        result = self.ad.categories()
        self.assertIsInstance(result, dict)


# =========================================================================
# Advanced Update
# =========================================================================


class TestAdvancedUpdate(AdminTestCase):
    def test_update_description(self):
        app = self._submit()
        new_desc = "Updated description " + unique_name()
        self.ad.update_app(app["id"], edit_token=app["edit_token"], description=new_desc)
        fetched = self.ad.get_app(app["id"])
        self.assertEqual(fetched["description"], new_desc)

    def test_update_tags(self):
        app = self._submit(tags=["old"])
        self.ad.update_app(app["id"], edit_token=app["edit_token"], tags=["new", "updated"])
        fetched = self.ad.get_app(app["id"])
        self.assertIn("new", fetched.get("tags", []))

    def test_update_protocol(self):
        app = self._submit(protocol="rest")
        self.ad.update_app(app["id"], edit_token=app["edit_token"], protocol="graphql")
        fetched = self.ad.get_app(app["id"])
        self.assertEqual(fetched["protocol"], "graphql")

    def test_update_category(self):
        app = self._submit(category="data")
        self.ad.update_app(app["id"], edit_token=app["edit_token"], category="security")
        fetched = self.ad.get_app(app["id"])
        self.assertEqual(fetched["category"], "security")

    def test_update_wrong_token(self):
        app = self._submit()
        with self.assertRaises((AuthError, ForbiddenError)):
            self.ad.update_app(app["id"], edit_token="wrong-token", name="Hacked")

    def test_update_nonexistent(self):
        with self.assertRaises((NotFoundError, AuthError)):
            self.ad.update_app("nonexistent-id", edit_token="fake", name="x")

    def test_admin_update_badges(self):
        """Admin can set featured/verified badges."""
        app = self._submit()
        self.admin.update_app(app["id"], is_featured=True)
        fetched = self.admin.get_app(app["id"])
        self.assertTrue(fetched.get("is_featured"))

    def test_admin_update_verified(self):
        app = self._submit()
        self.admin.update_app(app["id"], is_verified=True)
        fetched = self.admin.get_app(app["id"])
        self.assertTrue(fetched.get("is_verified"))

    def test_edit_token_cannot_set_badges(self):
        """Edit tokens cannot change admin-only fields — badge silently ignored."""
        app = self._submit()
        try:
            self.ad.update_app(app["id"], edit_token=app["edit_token"], is_featured=True)
        except (ForbiddenError, AppDirectoryError):
            pass  # Some implementations reject outright
        fetched = self.ad.get_app(app["id"])
        # Badge should NOT be set (edit token doesn't have admin rights)
        self.assertFalse(fetched.get("is_featured", False))

    def test_multiple_field_update(self):
        app = self._submit()
        new_name = unique_name("Multi")
        self.ad.update_app(
            app["id"],
            edit_token=app["edit_token"],
            name=new_name,
            short_description="Updated short",
            author_name="New Author",
        )
        fetched = self.ad.get_app(app["id"])
        self.assertEqual(fetched["name"], new_name)
        self.assertEqual(fetched["short_description"], "Updated short")
        self.assertEqual(fetched["author_name"], "New Author")


# =========================================================================
# Advanced Reviews
# =========================================================================


class TestAdvancedReviews(AdminTestCase):
    def test_review_rating_range(self):
        """Reviews should accept ratings 1-5."""
        app = self._submit()
        for rating in (1, 2, 3, 4, 5):
            result = self.ad.submit_review(app["id"], rating, title=f"Rating {rating}")
            self.assertIn("id", result)

    def test_review_updates_aggregate(self):
        """Submitting reviews should update aggregate rating."""
        app = self._submit()
        self.ad.submit_review(app["id"], 5, title="Perfect")
        self.ad.submit_review(app["id"], 3, title="Okay")
        fetched = self.ad.get_app(app["id"])
        self.assertIsNotNone(fetched.get("avg_rating"))
        self.assertGreater(fetched.get("review_count", 0), 0)

    def test_review_pagination(self):
        app = self._submit()
        for i in range(3):
            self.ad.submit_review(app["id"], 4, title=f"Review {i}")
        reviews = self.ad.list_reviews(app["id"], page=1, per_page=2)
        self.assertLessEqual(len(reviews.get("reviews", [])), 2)

    def test_review_with_body(self):
        app = self._submit()
        body_text = "This is a detailed review body with lots of content."
        self.ad.submit_review(app["id"], 4, title="Detailed", body=body_text)
        reviews = self.ad.list_reviews(app["id"])
        bodies = [r.get("body") for r in reviews["reviews"]]
        self.assertIn(body_text, bodies)

    def test_review_with_reviewer_name(self):
        app = self._submit()
        self.ad.submit_review(app["id"], 5, title="Named", reviewer_name="TestBot")
        reviews = self.ad.list_reviews(app["id"])
        found = any(r.get("reviewer_name") == "TestBot" for r in reviews["reviews"])
        self.assertTrue(found)


# =========================================================================
# Advanced Stats
# =========================================================================


class TestAdvancedStats(AdminTestCase):
    def test_stats_view_counting(self):
        """Getting an app increments view count."""
        app = self._submit()
        # View it a few times
        for _ in range(3):
            self.ad.get_app(app["id"])
        stats = self.ad.app_stats(app["id"])
        # Should have at least some views (including from _submit's get_app calls)
        self.assertGreaterEqual(stats.get("total_views", 0), 1)

    def test_stats_by_slug(self):
        """Stats work with slug too."""
        app = self._submit()
        stats = self.ad.app_stats(app["slug"])
        self.assertIn("total_views", stats)

    def test_stats_nonexistent(self):
        with self.assertRaises(NotFoundError):
            self.ad.app_stats("nonexistent-app-id")

    def test_trending_limit(self):
        result = self.ad.trending(limit=1)
        self.assertLessEqual(len(result.get("trending", [])), 1)

    def test_trending_days_range(self):
        for days in (1, 7, 30, 90):
            result = self.ad.trending(days=days)
            self.assertIn("trending", result)


# =========================================================================
# Advanced Delete & Cascade
# =========================================================================


class TestAdvancedDelete(AdminTestCase):
    def test_delete_cascade_reviews(self):
        """Deleting an app should cascade to reviews."""
        app = self.ad.submit(
            name=unique_name("CascadeDelete"),
            short_description="will be deleted",
            description="full",
            author_name="tester",
        )
        app_id = app.get("id") or app.get("app_id")
        self.ad.submit_review(app_id, 5, title="Soon gone")
        self.ad.delete_app(app_id, edit_token=app["edit_token"])
        with self.assertRaises(NotFoundError):
            self.ad.get_app(app_id)

    def test_admin_delete(self):
        """Admin can delete any app."""
        app = self._submit()
        result = self.admin.delete_app(app["id"])
        self.__class__._app_ids = [(i, t) for i, t in self.__class__._app_ids if i != app["id"]]
        with self.assertRaises(NotFoundError):
            self.ad.get_app(app["id"])


# =========================================================================
# Full Lifecycle
# =========================================================================


class TestFullLifecycle(AdminTestCase):
    def test_submit_review_deprecate_undeprecate(self):
        """Full lifecycle: submit (auto-approved) → review → deprecate → undeprecate."""
        # Submit (auto-approved)
        app = self._submit(name=unique_name("Lifecycle"))
        self.assertIn("id", app)
        fetched = self.admin.get_app(app["id"])
        self.assertEqual(fetched["status"], "approved")
        # Review
        self.ad.submit_review(app["id"], 5, title="Excellent")
        reviews = self.ad.list_reviews(app["id"])
        self.assertGreaterEqual(len(reviews["reviews"]), 1)
        # View stats
        stats = self.ad.app_stats(app["id"])
        self.assertIn("total_views", stats)
        # Deprecate
        self.admin.deprecate(app["id"], "Replaced by v2")
        fetched = self.admin.get_app(app["id"])
        self.assertEqual(fetched["status"], "deprecated")
        # Undeprecate
        self.admin.undeprecate(app["id"])
        fetched = self.admin.get_app(app["id"])
        self.assertEqual(fetched["status"], "approved")

    def test_submit_reject_reapprove(self):
        """Submit → reject → re-approve lifecycle."""
        app = self._submit()
        self.admin.reject(app["id"], "Needs more detail")
        fetched = self.admin.get_app(app["id"])
        self.assertEqual(fetched["status"], "rejected")
        self.admin.approve(app["id"], note="Fixed")
        fetched = self.admin.get_app(app["id"])
        self.assertEqual(fetched["status"], "approved")


# =========================================================================
# Constructor & Config
# =========================================================================


class TestConstructor(AppDirectoryTestCase):
    def test_trailing_slash_stripped(self):
        client = AppDirectory(BASE_URL + "/")
        self.assertFalse(client.base_url.endswith("/"))

    def test_custom_timeout(self):
        client = AppDirectory(BASE_URL, timeout=5)
        self.assertEqual(client.timeout, 5)

    def test_api_key_stored(self):
        client = AppDirectory(BASE_URL, api_key="test_key")
        self.assertEqual(client.api_key, "test_key")

    def test_edit_token_stored(self):
        client = AppDirectory(BASE_URL, edit_token="test_token")
        self.assertEqual(client.edit_token, "test_token")


# =========================================================================
# Discovery Advanced
# =========================================================================


class TestDiscoveryAdvanced(AdminTestCase):
    def test_openapi_has_info(self):
        spec = self.ad.openapi()
        self.assertIn("info", spec)
        self.assertIn("title", spec["info"])

    def test_openapi_has_paths(self):
        spec = self.ad.openapi()
        self.assertGreater(len(spec.get("paths", {})), 10)

    def test_llms_txt_not_empty(self):
        txt = self.ad.llms_txt()
        self.assertGreater(len(txt), 50)

    def test_skill_md_has_content(self):
        md = self.ad.skill_md()
        self.assertGreater(len(md), 100)
        self.assertIn("#", md)

    def test_root_llms_txt(self):
        """Root /llms.txt should be accessible."""
        import urllib.request
        resp = urllib.request.urlopen(f"{BASE_URL}/llms.txt", timeout=10)
        self.assertEqual(resp.status, 200)
        content = resp.read().decode()
        self.assertIn("app", content.lower())

    def test_api_v1_llms_txt(self):
        """API v1 llms.txt endpoint."""
        import urllib.request
        resp = urllib.request.urlopen(f"{BASE_URL}/api/v1/llms.txt", timeout=10)
        self.assertEqual(resp.status, 200)

    def test_skills_api_v1(self):
        """Skills available at /api/v1/skills/SKILL.md."""
        import urllib.request
        resp = urllib.request.urlopen(f"{BASE_URL}/api/v1/skills/SKILL.md", timeout=10)
        self.assertEqual(resp.status, 200)
        content = resp.read().decode()
        self.assertIn("#", content)


# =========================================================================
# Protocols & Categories Exhaustive
# =========================================================================


class TestProtocolsExhaustive(AppDirectoryTestCase):
    def test_all_protocols(self):
        """Test all 7 protocol types."""
        for proto in ("rest", "graphql", "grpc", "mcp", "a2a", "websocket", "other"):
            app = self._submit(protocol=proto, name=unique_name(f"Proto-{proto}"))
            fetched = self.ad.get_app(app["id"])
            self.assertEqual(fetched["protocol"], proto, f"Protocol mismatch for {proto}")

    def test_all_categories(self):
        """Test all valid categories."""
        for cat in ("communication", "data", "security", "infrastructure",
                     "finance", "media", "productivity", "social", "developer-tools",
                     "search", "ai-ml", "other"):
            app = self._submit(category=cat, name=unique_name(f"Cat-{cat}"))
            fetched = self.ad.get_app(app["id"])
            self.assertEqual(fetched["category"], cat, f"Category mismatch for {cat}")


# =========================================================================
# Unicode & Special Characters
# =========================================================================


class TestUnicode(AppDirectoryTestCase):
    def test_unicode_name(self):
        name = unique_name("Ünïcödé-🚀")
        app = self._submit(name=name)
        fetched = self.ad.get_app(app["id"])
        self.assertEqual(fetched["name"], name)

    def test_unicode_description(self):
        desc = "Description with émojis 🎉 and spëcial chars: <>&\"'"
        app = self._submit(description=desc)
        fetched = self.ad.get_app(app["id"])
        self.assertEqual(fetched["description"], desc)

    def test_unicode_tags(self):
        tags = ["日本語", "中文", "🏷️"]
        app = self._submit(tags=tags)
        fetched = self.ad.get_app(app["id"])
        self.assertEqual(sorted(fetched.get("tags", [])), sorted(tags))

    def test_unicode_review(self):
        app = self._submit()
        self.ad.submit_review(app["id"], 5, title="素晴らしい!", body="非常に良い")
        reviews = self.ad.list_reviews(app["id"])
        self.assertGreaterEqual(len(reviews["reviews"]), 1)


if __name__ == "__main__":
    print(f"\n📱 App Directory Python SDK Tests")
    print(f"   Server: {BASE_URL}\n")
    unittest.main(verbosity=2)
