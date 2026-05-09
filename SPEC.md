# Taverna — Technical Specification

**Version:** 1.2 (added theme switcher: light / dark / sepia)
**Status:** Authoritative. All implementation decisions must align with this document.
**Conflicts:** If any other file contradicts this document, this document wins.

---

## 1. What Is Taverna?

Taverna is a self-hostable, federated community platform. Think Facebook Groups + old Reddit, open source and part of the Fediverse.

A single Taverna instance:
- Hosts **multiple communities** (like subreddits or Facebook Groups)
- Hosts **user profiles** that can post publicly and be followed
- Federates with **Lemmy** instances (community-to-community)
- Federates with **Mastodon** users (profile follows, replies)
- Is **text and links only** — no media uploads in v1

---

## 2. Technology Stack

| Layer | Technology | Notes |
|---|---|---|
| Language | Rust | Single binary output |
| Web framework | Loco (built on Axum) | Rails-like conventions |
| ORM | SeaORM | Async, works with Loco |
| Database | PostgreSQL 16 | All data, queue, federation cache |
| Job queue | pgmq (PostgreSQL extension) | No separate broker |
| Templates | Tera (Jinja2 syntax) | Built into Loco |
| Frontend | HTMX | No JS framework |
| Markdown | pulldown-cmark + ammonia | Parse + sanitize |
| ActivityPub | activitypub_federation 0.6 | LemmyNet's crate |
| Theme | CSS custom properties + cookie | Light / Dark / Sepia, no JS framework |

**There is no Redis. There is no separate message broker. PostgreSQL does everything.**
**There is no hand-rolled HTTP signature code.** The `activitypub_federation` crate handles all of that.

### 2.1 License Note

`activitypub_federation` is AGPL-3.0. Taverna must also be AGPL-3.0.

---

## 3. Functional Requirements

### 3.1 Communities
- Anyone can browse communities without an account
- Logged-in users can join and post in communities
- Each community has: slug (URL-safe), name, description (markdown), creation date
- Community moderators can: remove posts/comments, ban users from community, pin posts, lock posts
- Instance admins can: everything mods can + delete communities, ban users globally
- Communities federate as ActivityPub `Group` actors

### 3.2 Posts
- A post belongs to exactly one community OR is a profile post (community_id IS NULL)
- Post types: text (body only), link (url only), or both (url + body description)
- Posts have: title, optional body (markdown), optional URL, score, comment count
- Users can vote +1 or -1 on any post (not their own)
- Votes are changeable and removable
- Posts can be edited by author (sends AP Update activity)
- Posts can be deleted by author, mod, or admin (sends AP Delete activity)

### 3.3 Comments
- Threaded (nested) using PostgreSQL ltree for path tracking
- No depth limit at DB level; UI collapses beyond 6 levels
- Comments can be voted on (same rules as posts)
- Comments can be edited by author
- Comments can be deleted by author, mod, or admin

### 3.4 User Profiles
- Registration: username (alphanumeric + underscore, 3–30 chars), email, password
- Login: email + password → JWT (access 1h, refresh 7d in httpOnly cookie)
- Profile page shows: bio, avatar URL (external link only), recent posts, recent comments
- Users can follow other users and communities
- Profile posts (community_id IS NULL) federate as personal Notes to followers

### 3.5 Theme Switcher

Users can select one of three visual themes:

| Theme | Description |
|---|---|
| `light` | White background, dark text. Default for new visitors. |
| `dark` | Dark background (#1a1a2e), light text. |
| `sepia` | Warm off-white background (#f4ecd8), dark brown text. Easy on the eyes for reading. |

**Implementation rules:**
- Theme is stored in a cookie named `taverna_theme` (value: `light`, `dark`, or `sepia`)
- Cookie is set server-side by a POST handler — no JavaScript required for the switch itself
- The `<html>` element carries a `data-theme` attribute set from the cookie on every page render
- All colours are defined as CSS custom properties on `[data-theme="light"]`, `[data-theme="dark"]`, and `[data-theme="sepia"]` selectors in a single `assets/static/theme.css` file
- The theme switcher UI is a small form in the navigation bar with three buttons (☀ Light, ● Dark, ✦ Sepia)
- No JavaScript is required. The form POSTs to `/theme` and the server redirects back with the cookie set.
- If no cookie is present, default to `light`
- The cookie must be: httpOnly=false (CSS reads data-theme from HTML, not JS), SameSite=Lax, max-age=31536000 (1 year)

**CSS custom properties required** (minimum set — add more as needed):

```css
[data-theme="light"] {
  --bg-primary: #ffffff;
  --bg-secondary: #f6f7f8;
  --bg-card: #ffffff;
  --text-primary: #1a1a1a;
  --text-secondary: #666666;
  --text-muted: #999999;
  --border: #e0e0e0;
  --accent: #8b3a3a;        /* Taverna wine red */
  --accent-hover: #6d2e2e;
  --upvote: #e05a00;
  --downvote: #5a7eb5;
  --link: #3a5f8b;
  --link-hover: #2a4a6b;
  --nav-bg: #1a1a2e;
  --nav-text: #ffffff;
  --score-positive: #e05a00;
  --score-negative: #5a7eb5;
}

[data-theme="dark"] {
  --bg-primary: #1a1a2e;
  --bg-secondary: #16213e;
  --bg-card: #0f3460;
  --text-primary: #e0e0e0;
  --text-secondary: #a0a0b0;
  --text-muted: #6a6a7a;
  --border: #2a2a4e;
  --accent: #c0566a;
  --accent-hover: #e0687c;
  --upvote: #ff7b3a;
  --downvote: #7ab0e8;
  --link: #7ab0e8;
  --link-hover: #9ac5f5;
  --nav-bg: #0d0d1a;
  --nav-text: #e0e0e0;
  --score-positive: #ff7b3a;
  --score-negative: #7ab0e8;
}

[data-theme="sepia"] {
  --bg-primary: #f4ecd8;
  --bg-secondary: #ede0c4;
  --bg-card: #faf3e0;
  --text-primary: #3b2f1e;
  --text-secondary: #6b5740;
  --text-muted: #9b8070;
  --border: #d4c4a0;
  --accent: #8b5e3c;
  --accent-hover: #6d4830;
  --upvote: #b06020;
  --downvote: #4a6a8a;
  --link: #6b4226;
  --link-hover: #4d2e18;
  --nav-bg: #3b2f1e;
  --nav-text: #f4ecd8;
  --score-positive: #b06020;
  --score-negative: #4a6a8a;
}
```

**Route:**
```
POST /theme   → set taverna_theme cookie, redirect to Referer (or / if no Referer)
```

**Template integration** (in `views/layouts/base.html`):
```html
<!DOCTYPE html>
<html lang="en" data-theme="{{ theme }}">
<head>
  <link rel="stylesheet" href="/static/theme.css">
  ...
</head>
<body>
  <nav>
    ...
    <form method="POST" action="/theme" style="display:inline">
      <input type="hidden" name="_method" value="POST">
      <button type="submit" name="theme" value="light"  title="Light mode">☀</button>
      <button type="submit" name="theme" value="dark"   title="Dark mode">●</button>
      <button type="submit" name="theme" value="sepia"  title="Sepia mode">✦</button>
    </form>
  </nav>
  {% block content %}{% endblock %}
</body>
</html>
```

The `theme` variable is injected into every template context by a Tera global or by every handler reading the cookie. Prefer a Tera global function or a middleware that injects `theme` into every context automatically — do not manually add it in each handler.

---

## 4. Data Model

### 4.1 PostgreSQL Extensions Required

```sql
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS ltree;
CREATE EXTENSION IF NOT EXISTS pgmq;
```

### 4.2 Table: users

```sql
CREATE TABLE users (
  id                UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
  username          TEXT UNIQUE NOT NULL,
  email             TEXT UNIQUE,
  password_hash     TEXT,
  display_name      TEXT,
  bio               TEXT,
  avatar_url        TEXT,
  ap_id             TEXT UNIQUE NOT NULL,
  public_key        TEXT NOT NULL DEFAULT '',
  private_key       TEXT,
  is_local          BOOL NOT NULL DEFAULT true,
  is_admin          BOOL NOT NULL DEFAULT false,
  is_banned         BOOL NOT NULL DEFAULT false,
  inbox_url         TEXT NOT NULL DEFAULT '',
  shared_inbox_url  TEXT,
  followers_url     TEXT NOT NULL DEFAULT '',
  last_fetched_at   TIMESTAMPTZ,
  created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_users_username ON users(username);
CREATE INDEX idx_users_ap_id ON users(ap_id);
```

### 4.3 Table: communities

```sql
CREATE TABLE communities (
  id                  UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
  slug                TEXT UNIQUE NOT NULL,
  name                TEXT NOT NULL,
  description         TEXT,
  ap_id               TEXT UNIQUE NOT NULL,
  public_key          TEXT NOT NULL DEFAULT '',
  private_key         TEXT,
  inbox_url           TEXT NOT NULL DEFAULT '',
  followers_url       TEXT NOT NULL DEFAULT '',
  is_local            BOOL NOT NULL DEFAULT true,
  is_nsfw             BOOL NOT NULL DEFAULT false,
  posting_restricted  BOOL NOT NULL DEFAULT false,
  last_fetched_at     TIMESTAMPTZ,
  created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

### 4.4 Table: posts

```sql
CREATE TABLE posts (
  id            UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
  ap_id         TEXT UNIQUE NOT NULL,
  community_id  UUID REFERENCES communities(id) ON DELETE CASCADE,
  author_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  title         TEXT NOT NULL,
  body          TEXT,
  url           TEXT,
  score         INT NOT NULL DEFAULT 0,
  comment_count INT NOT NULL DEFAULT 0,
  is_deleted    BOOL NOT NULL DEFAULT false,
  is_removed    BOOL NOT NULL DEFAULT false,
  is_locked     BOOL NOT NULL DEFAULT false,
  is_pinned     BOOL NOT NULL DEFAULT false,
  published_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  CONSTRAINT post_has_content CHECK (body IS NOT NULL OR url IS NOT NULL)
);
CREATE INDEX idx_posts_community_id ON posts(community_id);
CREATE INDEX idx_posts_author_id ON posts(author_id);
CREATE INDEX idx_posts_published_at ON posts(published_at DESC);
CREATE INDEX idx_posts_score ON posts(score DESC);
```

### 4.5 Table: comments

```sql
CREATE TABLE comments (
  id            UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
  ap_id         TEXT UNIQUE NOT NULL,
  post_id       UUID NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
  author_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  parent_id     UUID REFERENCES comments(id) ON DELETE CASCADE,
  path          LTREE NOT NULL,
  body          TEXT NOT NULL,
  score         INT NOT NULL DEFAULT 0,
  is_deleted    BOOL NOT NULL DEFAULT false,
  is_removed    BOOL NOT NULL DEFAULT false,
  published_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_comments_post_id ON comments(post_id);
CREATE INDEX idx_comments_path ON comments USING GIST(path);
```

ltree path: prefix UUID segment with `c`. Top-level: `c{post_8}.c{comment_8}`. Reply: `c{post_8}.c{parent_8}.c{comment_8}`.

### 4.6 Table: votes

```sql
CREATE TABLE votes (
  id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
  user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  post_id     UUID REFERENCES posts(id) ON DELETE CASCADE,
  comment_id  UUID REFERENCES comments(id) ON DELETE CASCADE,
  value       SMALLINT NOT NULL CHECK (value IN (1, -1)),
  created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  CONSTRAINT vote_target CHECK ((post_id IS NULL) != (comment_id IS NULL)),
  CONSTRAINT vote_unique_post    UNIQUE (user_id, post_id),
  CONSTRAINT vote_unique_comment UNIQUE (user_id, comment_id)
);
```

### 4.7 Table: community_members

```sql
CREATE TABLE community_members (
  community_id  UUID NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
  user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  role          TEXT NOT NULL DEFAULT 'member' CHECK (role IN ('member', 'moderator')),
  joined_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (community_id, user_id)
);
```

### 4.8 Table: follows

```sql
CREATE TABLE follows (
  id            UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
  follower_id   UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  following_id  UUID REFERENCES users(id) ON DELETE CASCADE,
  community_id  UUID REFERENCES communities(id) ON DELETE CASCADE,
  ap_id         TEXT,
  accepted      BOOL NOT NULL DEFAULT false,
  created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  CONSTRAINT follow_target CHECK ((following_id IS NULL) != (community_id IS NULL)),
  CONSTRAINT follow_unique_user      UNIQUE (follower_id, following_id),
  CONSTRAINT follow_unique_community UNIQUE (follower_id, community_id)
);
```

### 4.9 Table: outbound_activities

```sql
CREATE TABLE outbound_activities (
  id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
  activity_json   JSONB NOT NULL,
  target_inboxes  TEXT[] NOT NULL,
  actor_ap_id     TEXT NOT NULL,
  state           TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending','sent','failed')),
  attempts        INT NOT NULL DEFAULT 0,
  next_attempt_at TIMESTAMPTZ DEFAULT NOW(),
  last_error      TEXT,
  created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_outbound_pending ON outbound_activities(next_attempt_at)
  WHERE state = 'pending';
```

---

## 5. URL Structure

### 5.1 Web Routes

```
GET  /                           Home feed
GET  /login                      Login form
POST /login                      Authenticate
GET  /register                   Register form
POST /register                   Create account
POST /logout                     Clear session
POST /theme                      Set theme cookie, redirect back

GET  /c/                         Browse communities
GET  /c/:slug                    Community feed
GET  /c/:slug/new                New post form (auth required)
POST /c/:slug/posts              Create post
GET  /c/:slug/posts/:id          Post + comments
POST /c/:slug/posts/:id/vote     Vote on post (HTMX)
POST /c/:slug/posts/:id/delete   Delete post
POST /c/:slug/posts/:id/comments Create comment
POST /c/:slug/posts/:id/comments/:cid/vote  Vote on comment (HTMX)
POST /c/:slug/join               Join community
POST /c/:slug/leave              Leave community
GET  /c/:slug/settings           Community settings (mod only)
POST /c/:slug/settings           Update community settings

GET  /u/:username                User profile
GET  /u/:username/posts          User post history
GET  /u/:username/comments       User comment history

GET  /settings                   Account settings
POST /settings                   Update account
POST /settings/password          Change password
```

### 5.2 Federation Routes

```
GET  /.well-known/webfinger
GET  /.well-known/nodeinfo
GET  /nodeinfo/2.0
GET  /u/:username                (content negotiated)
POST /u/:username/inbox
GET  /u/:username/outbox
GET  /u/:username/followers
GET  /u/:username/following
GET  /c/:slug                    (content negotiated)
POST /c/:slug/inbox
GET  /c/:slug/outbox
GET  /c/:slug/followers
GET  /posts/:id
GET  /comments/:id
```

---

## 6. ActivityPub — Using activitypub_federation Crate

### 6.1 What the Crate Handles (Do Not Reimplement)
- HTTP signature signing and verification
- Remote actor fetching and caching (`ObjectId::dereference`)
- WebFinger response building (`build_webfinger_response`, `extract_webfinger_name`)
- Activity sending with retry (`SendActivityTask::send`)
- SSRF protection on outbound fetches
- `FederationMiddleware` Axum layer
- `FederationJson` response type and `FEDERATION_CONTENT_TYPE` constant

### 6.2 Traits to Implement
- `Object` — on DbUser, DbCommunity, DbPost, DbComment
- `Actor` — on DbUser, DbCommunity
- `ActivityHandler` — on every activity struct

### 6.3 FederationConfig Setup
```rust
let federation_config = FederationConfig::builder()
    .domain(&config.taverna.domain)
    .app_data(db_pool.clone())
    .debug(cfg!(debug_assertions))
    .build().await?;
```

### 6.4 The Announce Pattern (Critical — Lemmy Compat)
When community inbox receives `Create{Note}`:
1. Save the Note locally
2. Community Group actor sends `Announce{Create{Note}}` to ALL community followers via `SendActivityTask::send`

### 6.5 Supported Activities
See PHASE_4.md for full implementation details and `ActivityHandler` implementations.

### 6.6 NodeInfo
```json
{"version":"2.0","software":{"name":"taverna","version":"0.1.0"},
 "protocols":["activitypub"],"usage":{"users":{"total":0},"localPosts":0},
 "openRegistrations":true}
```

---

## 7. Security Requirements

| Requirement | Implementation |
|---|---|
| Password hashing | argon2id via `argon2` crate |
| Session tokens | JWT, 1h access + 7d refresh in httpOnly cookie |
| HTTP signature verification | activitypub_federation crate |
| SSRF protection | activitypub_federation crate |
| Rate limiting | tower-governor on inbox and auth endpoints |
| Markdown safety | pulldown-cmark → ammonia |
| SQL injection | SeaORM parameterized queries only |

---

## 8. Configuration

```yaml
# config/production.yaml
server:
  binding: 0.0.0.0
  port: 3000
database:
  min_connections: 5
  max_connections: 50
taverna:
  domain: "${TAVERNA_DOMAIN}"
  base_url: "${BASE_URL}"
  federation:
    enabled: true
    blocked_domains: []
workers:
  deliver_activity:
    concurrency: 8
```

---

## 9. Out of Scope for v1

Do NOT build: media uploads, OAuth/SSO, Bluesky bridge, full-text search, direct messages, push notifications, defederation admin UI.
