# App Directory — Design Document

> See also: [Shared Design Principles](../docs/design-principles.md)

## Overview

A public directory of applications/services. Open for browsing, lightweight submission process with optional anonymous listings.

## Auth Model: Open Read, Token-on-Create Write

The directory is **public by nature** — the whole point is discoverability. Reading never requires auth. Submitting is low-friction with two paths.

### Access Rules

| Operation | Auth Required | Rationale |
|-----------|--------------|-----------|
| Browse / search directory | ❌ No | Public directory |
| View app details | ❌ No | Public directory |
| View categories / tags | ❌ No | Public directory |
| Health check status | ❌ No | Public info |
| Submit an app (anonymous) | ❌ No | Returns an edit token |
| Submit an app (with key) | 🔑 Optional API key | Manage multiple listings |
| Edit/update listing | 🔑 Edit token or API key | Proves ownership |
| Delete listing | 🔑 Edit token or API key | Proves ownership |
| Admin operations | 🔑 Admin key | Moderation, featuring apps |

### Two Submission Paths

#### Path 1: Anonymous One-Off Submission
1. Submit app details (name, URL, description, tags)
2. Response includes:
   ```json
   {
     "app_id": "uuid",
     "edit_token": "ad_uuid",
     "listing_url": "/apps/{app_id}",
     "edit_url": "/apps/{app_id}/edit?token={edit_token}"
   }
   ```
3. Save the `edit_token` — it's the only way to modify/delete this listing
4. Lose the token → can't edit anymore (can request admin help)

**Best for:** Quick one-off listings, agents that just want to register something.

#### Path 2: API Key for Multiple Listings
1. `POST /keys` → generates an API key (no signup, one click)
   ```json
   {
     "api_key": "ad_uuid",
     "key_id": "uuid"
   }
   ```
2. Use this key for all submissions → all listings tied to this key
3. Can list/manage all your submissions via the key

**Best for:** Agents or humans managing multiple app listings.

### Key Differences from Current Implementation

Currently, ALL operations require a global API key, and there's an admin key auto-generated on first run. The new model:
- Removes auth from all read operations
- Makes submission auth optional (anonymous allowed)
- Keeps admin key for moderation only
- Replaces global API key system with per-resource tokens

## User Flows

### AI Agent Flow (Anonymous)
1. `POST /apps` with `{ name: "My Tool", url: "https://...", description: "...", tags: ["utility"] }`
2. Gets back `app_id`, `edit_token`, `listing_url`
3. Saves `edit_token` for future updates
4. Done — listing is live immediately

### AI Agent Flow (With Key)
1. `POST /keys` → gets `api_key` (one time)
2. `POST /apps` with `Authorization: Bearer {api_key}` and app details
3. All future submissions use the same key
4. `GET /apps/mine` with key → list all their submissions

### Human Flow (Browse)
1. Open the directory → see all listed apps
2. Search, filter by category/tag
3. Click an app → see details, health status, links

### Human Flow (Submit)
1. Click "Submit App"
2. Fill in the form (name, URL, description, tags)
3. Click submit → listing created immediately
4. Shown a confirmation: "Your listing is live! Bookmark this edit link: [edit_url]"
5. No signup, no email, no verification

## Moderation

### V1: Light Touch
- All submissions go **live immediately** (no approval queue)
- Each listing has a "Report" button → flags for admin review
- Admin key can: feature apps, remove listings, edit anything
- Spam is not a v1 concern — address if/when it becomes real

### Future Considerations
- Optional approval queue for flagged domains
- Community voting/rating
- Verified publisher badges

## Health Checks

The directory can periodically check if listed apps are online:
- Store last health check result and timestamp
- Show status indicator on listing (🟢 online, 🔴 offline, ⚪ unchecked)
- Health checks run server-side on a schedule — no auth needed
- Apps can opt out of health checks

## API Changes Needed (from current state)

1. **Remove auth from ALL read endpoints** (list, search, get, categories, tags, health)
2. **Make submission work without auth** — return edit token on creation
3. **Add anonymous submission flow** with edit tokens
4. **Add optional API key generation** (`POST /keys`, no signup)
5. **Add `/apps/mine` endpoint** for key holders to list their submissions
6. **Keep admin key** but only for moderation operations
7. **Add `token` query param support** for edit URLs in browsers
8. **Remove requirement for auth on webhooks endpoint** (if public notifications)

## Rate Limiting

- **Read:** No rate limit (or very generous — 1000 req/min per IP)
- **Submissions:** IP-based limit (e.g., 10 submissions/hour per IP)
- **Key generation:** IP-based limit (e.g., 5 keys/hour per IP)
