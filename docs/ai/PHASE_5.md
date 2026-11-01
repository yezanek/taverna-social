# Phase 5 — User Profiles, Moderation & Polish

> **Status:** complete. This file is the plan this phase was built from. It is kept as a
> record of why the code looks the way it does, and as a template for planning similar work.
> Where it disagrees with the code, `SPEC.md`, `DECISIONS.md` or [DESIGN.md](../DESIGN.md), those win.
>
> **Changed since:** Production config is `config/production.yaml`, copied from `config/production.example.yaml`. Rate limiting is keyed on the socket peer with `governor`, and `X-Forwarded-For` is trusted only from `TRUSTED_PROXIES` (ADR-026). DEPLOY.md is the current deployment guide.

**Goal:** Complete user profiles, moderation tools, and production-readiness hardening. This is the final phase before deployment.

**Prerequisites:** Phase 4 complete. Federation working. All tests passing.

---

## Tasks

### Task 5.1 — User profile pages

**GET /u/:username (HTML mode)**

Display:
- Avatar (if `avatar_url` set, render as `<img>`)
- Display name + @username@instance
- Bio (rendered markdown)
- Follower count, following count
- "Follow" / "Unfollow" button (HTMX, for logged-in users only)
- Tab bar: Posts | Comments
- Default tab: recent posts (paginated)

**GET /u/:username/posts** — paginated list of user's posts  
**GET /u/:username/comments** — paginated list of user's comments with post context

### Task 5.2 — Follow/unfollow (local users)

Add to users controller:

```
POST /u/:username/follow    → create follow row, enqueue AP Follow if remote
POST /u/:username/unfollow  → delete follow row, enqueue AP Undo{Follow} if remote
```

Both return HTMX fragments updating the follow button.

For local follows: `accepted = true` immediately (no AP roundtrip needed).
For remote follows: `accepted = false`, enqueue AP Follow, update to `true` when Accept received.

### Task 5.3 — Account settings

**GET /settings** — form with:
- Display name field
- Bio field (markdown)
- Avatar URL field (must be a valid HTTPS URL if provided)
- Email field (shows current, change requires password confirmation)

**POST /settings** — update the above fields  
**POST /settings/password** — change password (requires current password, new password, confirm)

After updating display_name or bio: enqueue `Update{Person}` activity to all followers.

### Task 5.4 — Moderation tools

**Community moderator actions** (must verify role = 'moderator' or is_admin):

```
POST /c/:slug/posts/:id/remove        → set is_removed=true, send AP Delete
POST /c/:slug/posts/:id/pin           → toggle is_pinned
POST /c/:slug/posts/:id/lock          → toggle is_locked (no new comments)
POST /c/:slug/posts/:id/comments/:cid/remove → remove comment

POST /c/:slug/bans                    → form: username, reason; insert ban row
POST /c/:slug/bans/:username/delete   → remove ban
(Both are driven from the ban list on /c/:slug/settings; see ADR-028.)
```

**Create community bans table** (new migration: `m007_bans.rs`):

```sql
CREATE TABLE community_bans (
  community_id  UUID NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
  user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  banned_by     UUID NOT NULL REFERENCES users(id),
  reason        TEXT,
  created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (community_id, user_id)
);
```

**Instance admin actions** (must verify `is_admin=true`):

```
POST /admin/users/:username/ban       → set users.is_banned=true
POST /admin/users/:username/unban     → set users.is_banned=false
POST /admin/communities/:slug/delete  → soft delete community
```

Add a minimal `/admin` dashboard page listing recent users and communities.

### Task 5.5 — Rate limiting

Add `tower-governor` to Cargo.toml. Apply rate limits:

```rust
// In app.rs router setup:
use tower_governor::{governor::GovernorConfigBuilder, GovernorLayer};

// Strict limit for federation inboxes (protect against spam)
let federation_governor = GovernorConfigBuilder::default()
    .per_second(5)
    .burst_size(20)
    .finish().unwrap();

// Normal limit for web routes
let web_governor = GovernorConfigBuilder::default()
    .per_second(30)
    .burst_size(100)
    .finish().unwrap();
```

Apply federation limit to all `/*/inbox` routes.
Apply web limit to `POST /register` and `POST /login`.

### Task 5.6 — Locked post enforcement

When `is_locked = true` on a post:
- Hide the comment form in the template
- Return 403 from the create_comment handler with message "Post is locked"

### Task 5.7 — Banned user enforcement

In `AuthUser` middleware: after loading user from JWT, check `is_banned`. If true, clear cookies and return 403.

In community posting: check `community_bans` table before allowing post or comment creation.

### Task 5.8 — Error pages

Create templates:
- `assets/views/errors/404.html` — "Page not found"
- `assets/views/errors/403.html` — "You don't have permission"
- `assets/views/errors/500.html` — "Something went wrong"

Register a custom error handler in Axum that renders these templates for the corresponding status codes.

### Task 5.9 — Production configuration

Update `config/production.toml`:

```toml
server:
  binding: 0.0.0.0
  port: 3000

database:
  min_connections: 5
  max_connections: 50

taverna:
  base_url: "${BASE_URL}"       # read from environment variable
  federation:
    enabled: true
    blocked_domains: []
    http_signature_expiry_seconds: 30
    user_agent: "Taverna/0.1.0 (+${BASE_URL})"

workers:
  deliver_activity:
    concurrency: 8
    max_attempts: 5
```

### Task 5.10 — Deployment documentation

Create `DEPLOY.md`:

```markdown
# Deployment Guide

## Requirements
- PostgreSQL 16 with extensions: uuid-ossp, ltree, pgmq
- Reverse proxy: Caddy or nginx (handles TLS)
- A domain name with TLS certificate

## PostgreSQL Setup
pgmq must be installed as a PostgreSQL extension:
  CREATE EXTENSION pgmq;  (requires superuser)

## Environment Variables
  DATABASE_URL=postgres://user:pass@localhost/taverna_production
  BASE_URL=https://your-domain.com
  JWT_SECRET=<openssl rand -hex 32>
  RUST_LOG=info

## Build
  cargo build --release
  ./target/release/taverna-cli start

## Database Setup
  ./target/release/taverna-cli db migrate

## Reverse Proxy (Caddy example)
  your-domain.com {
    reverse_proxy localhost:3000
  }

## First Run
  After deployment, create the first admin user via the registration page,
  then in psql:
    UPDATE users SET is_admin = true WHERE username = 'yourusername';
```

### Task 5.11 — Final test suite

```rust
// tests/moderation_test.rs
test_moderator_can_remove_post()
test_non_moderator_cannot_remove_post()
test_banned_user_cannot_post()
test_locked_post_rejects_comments()
test_admin_can_ban_user()

// tests/profile_test.rs
test_follow_local_user()
test_follow_sets_accepted_immediately_for_local()
test_unfollow_removes_row()
test_settings_update_display_name()
test_password_change_requires_current_password()

// tests/integration_test.rs
// End-to-end: register → login → create community → create post → vote → comment
test_full_happy_path()
```

---

## Deliverables Checklist

- [ ] User profile pages (HTML + AP JSON)
- [ ] Follow/unfollow with HTMX buttons
- [ ] Account settings (display name, bio, avatar, email, password)
- [ ] Community moderator actions (remove, pin, lock, ban)
- [ ] Admin actions (ban user, delete community)
- [ ] `m007_bans.rs` migration applied
- [ ] Rate limiting on inboxes and auth endpoints
- [ ] Locked post enforcement
- [ ] Banned user enforcement
- [ ] Error pages (404, 403, 500)
- [ ] `config/production.toml` complete
- [ ] `DEPLOY.md` written
- [ ] `cargo check` passes
- [ ] `cargo test` passes (all tests including new ones)
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes
- [ ] `cargo fmt` applied

---

## Definition of Done (Entire Project)

The project is complete when:

1. A user can register, log in, create a community, post, comment, and vote
2. A Lemmy instance can subscribe to a Taverna community and see posts
3. A Mastodon user can follow a Taverna user profile and see posts
4. A remote Lemmy user can post to a Taverna community (via Follow + Create)
5. All 5 phases' deliverable checklists are complete
6. `cargo test` passes with zero failures
7. `cargo clippy --all-targets --all-features -- -D warnings` produces zero warnings
8. The application starts cleanly in production mode

---

## Prompts

Prompts for redoing or extending this phase with an AI coding agent. They work with any
tool; see [INSTRUCTIONS.md](INSTRUCTIONS.md) for the workflow.

### Implement

```
Read AGENTS.md, docs/ai/SPEC.md, and docs/ai/PHASE_5.md.

Phases 1–4 are complete. Federation works.

Show me a plan before writing any code.

Implement Phase 5: user profiles, follow/unfollow, account settings, moderation
tools, rate limiting, banned user enforcement, error pages, production config,
and DEPLOY.md.

New migration m007_bans.rs — show me the migration before applying it.
Run cargo loco db migrate after I approve.

After completing:
  cargo fmt && cargo clippy --all-targets --all-features -- -D warnings && cargo test

All must pass with zero errors and zero warnings.
Verify the Definition of Done at the bottom of docs/ai/PHASE_5.md.
```

### Review

```
You are a QA engineer reviewing Phase 5 of the Taverna project.

Read AGENTS.md, docs/ai/SPEC.md, and docs/ai/PHASE_5.md.

Verify:

RATE LIMITING
- Open src/app.rs or wherever tower-governor is configured.
- Inbox routes must have strict limit (5 req/s, burst 20).
- POST /login and POST /register must have auth limit (10 req/s, burst 30).
- Missing limits: MEDIUM.

BANNED USER ENFORCEMENT
- Open src/initializers/auth_middleware.rs.
- After loading user from JWT, must check user.is_banned.
- If banned: clear cookies, return 403.
- Missing check: MEDIUM.

LOCKED POST ENFORCEMENT
- Comment form must be hidden in template when is_locked=true.
- create_comment handler must return 403 for locked posts.
- If either check is missing: MEDIUM.

COMMUNITY BAN TABLE
- Verify m007_bans.rs migration exists and is registered in migration/src/lib.rs.
- Run: cargo loco db migrate (should succeed with no errors).

ERROR PAGES
- assets/views/errors/404.html, assets/views/errors/403.html, assets/views/errors/500.html all exist.
- Custom error handler renders these for the right status codes.

DEPLOY.md EXISTS
- Contains: PostgreSQL extensions setup, environment variables, build command,
  migration command, Caddy example, admin setup, AGPL-3.0 license note.

PRODUCTION CONFIG
- config/production.yaml uses <%= get_env(name="BASE_URL") %> and <%= get_env(name="TAVERNA_DOMAIN") %> (env var interpolation).
- Not hardcoded URLs.

FINAL CHECKS
Run: cargo fmt -- check
Run: cargo clippy --all-targets --all-features -- -D warnings
Run: cargo test

All must produce zero errors and zero warnings.

Verify the Definition of Done checklist at the bottom of docs/ai/PHASE_5.md.

Report CRITICAL / MEDIUM / LOW.
```
