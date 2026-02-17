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


if __name__ == "__main__":
    print(f"\n📱 App Directory Python SDK Tests")
    print(f"   Server: {BASE_URL}\n")
    unittest.main(verbosity=2)
