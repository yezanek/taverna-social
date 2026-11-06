# Taverna — Architecture & Design

This document describes how Taverna is built: its parts, its data, how
requests and federation flow through it, and the design rules it follows.
It replaces the original `taverna_design.docx`, and is updated to match the
code as of version 0.0.8.

For exact requirements see [docs/ai/SPEC.md](ai/SPEC.md); for the reasoning
behind individual choices see the ADRs in
[docs/ai/DECISIONS.md](ai/DECISIONS.md); for every setting see
[CONFIGURATION.md](CONFIGURATION.md). Where this document and the code
disagree, the code wins and this document should be fixed.

---

## 1. Overview

### 1.1 What Taverna is

Taverna is a self-hostable, federated community platform modelled on
Facebook Groups and old Reddit. One instance hosts many communities, each
with its own members, moderators and post feed. Users also have personal
profiles with their own posts and followers.

Federation is the core design constraint. Every user, community, post and
comment is an ActivityPub actor or object; the web UI is a view over the
same data that Lemmy, Mastodon and other Fediverse servers consume.

### 1.2 Scope of v1

In scope:

- Communities: create, join, leave, post, comment, vote, moderate.
- Personal profiles with posts visible to followers (Mastodon-compatible).
- Federation: Lemmy-compatible communities, Mastodon-compatible profiles.
- Text and link posts only.
- Moderation: community moderators, instance admins, removal, community
  bans, global bans and suspensions.
- Local accounts only (username or email + password, JWT sessions in
  cookies), with optional email verification.
- Instance onboarding: a first-run setup wizard and tunable rules for who
  may create communities.

Out of scope for v1: media uploads, OAuth/SSO, full-text search, direct
messages, a Bluesky/AT Protocol bridge (see §11).

### 1.3 Stack

| Layer | Choice |
|---|---|
| Language | Rust (edition 2024) |
| Web framework | Loco 1.2 on Axum 0.8 |
| ORM | SeaORM 2.0 |
| Database | PostgreSQL 16 with `uuid-ossp` and `ltree` |
| Jobs and federation cache | PostgreSQL tables (no Redis, no pgmq) |
| Templates | Tera 2, server-rendered, plus HTMX |
| Federation | `activitypub_federation` 0.7 (LemmyNet) |
| Auth | HS256 JWTs (`jsonwebtoken`) in httpOnly cookies; argon2 passwords |
| Markdown | `pulldown-cmark` rendered, then sanitised with `ammonia` |
| Rate limiting | `governor`, keyed per client |

The `activitypub_federation` crate is AGPL-3.0, so Taverna is
AGPL-3.0-or-later.

---

## 2. Architecture

### 2.1 Topology

Taverna is one binary (`taverna-cli`, the Loco app) plus one PostgreSQL
database. PostgreSQL holds the application data, the outbound delivery
queue and the cache of remote federated content (ADR-001). Background work
runs inside the server process.

```
┌───────────────────────────────────────────────────────┐
│                   Browser (HTML + HTMX)               │
└──────────────────────────┬────────────────────────────┘
                           │ HTTP
┌──────────────────────────▼────────────────────────────┐
│               Taverna (Loco / Axum process)           │
│  ┌──────────────┐ ┌───────────────┐ ┌───────────────┐ │
│  │ HTML handlers│ │ ActivityPub   │ │ WebFinger,    │ │
│  │ (Tera views) │ │ actors, inbox │ │ NodeInfo      │ │
│  └──────────────┘ └───────────────┘ └───────────────┘ │
│  ┌──────────────────────────────────────────────────┐ │
│  │ In-process loops: delivery poller (every 5 s),   │ │
│  │ remote cache expiry (daily) → Loco workers       │ │
│  └──────────────────────────────────────────────────┘ │
└──────────────────────────┬────────────────────────────┘
                           │
┌──────────────────────────▼────────────────────────────┐
│                     PostgreSQL 16                     │
│   local data │ outbound_activities │ remote_* cache   │
└───────────────────────────────────────────────────────┘
                           ↕ signed ActivityPub HTTP
                Lemmy │ Mastodon │ other Fediverse servers
```

### 2.2 Code layout

Taverna follows Loco's conventions:

```
src/
├── app.rs              routes, middleware stack, workers, after_context
├── bin/main.rs         the taverna-cli entry point (Loco CLI)
├── common/             config.rs (TavernaConfig), jwt.rs, markdown.rs
├── controllers/        HTML handlers: auth, home, communities, posts, users,
│   │                   admin, setup, verify, theme; csrf.rs and errors.rs helpers
│   └── federation/     actors, inbox, notes, nodeinfo, webfinger
├── federation/         protocol.rs (AP types), traits.rs (Object/Actor impls),
│                       activities.rs (inbound handlers), outbound.rs (builders)
├── initializers/       view_engine, rate_limiter, federation_jobs, first_run,
│                       auth_middleware, theme_middleware, content_negotiation
├── mailers/            verification email
├── models/             domain logic, one file per table; _entities/ generated
├── views/              the Views render extractor (ADR-027)
└── workers/            deliver_activity, expire_remote_cache
migration/src/          m001_extensions … m010_performance_indexes
assets/views/           Tera templates: layouts, partials, icons, pages
assets/static/          theme.css, htmx.min.js, 404.html
config/                 development.yaml, test.yaml, production.example.yaml
tests/                  integration tests, one file per area
```

### 2.3 Startup and shared state

On boot, `App::after_context` reads the `settings:` block of the config file
into `TavernaConfig` and builds the `activitypub_federation` config once.
Both are kept in Loco's `ctx.shared_store` for the router and the workers
(ADR-025). NodeInfo counts are cached in `ctx.cache` (in-memory).

### 2.4 Middleware stack

Applied in `App::after_routes`, outermost last:

1. **Fallback 404** renders `errors/404.html`.
2. **Error pages**: turns 403/404/500 responses into themed HTML pages for
   browsers, leaves JSON and ActivityPub responses alone.
3. **`TavernaConfig` extension** for handlers.
4. **Session refresh**: when the 1-hour access token has expired, renews it
   from the 7-day refresh token (ADR-022).
5. **Theme**: reads the `taverna_theme` and `taverna_accent` cookies and
   exposes the current theme and accent colour.
6. **`FederationMiddleware`** from `activitypub_federation`.
7. **Content negotiation**: the same URL serves HTML or ActivityPub JSON
   depending on the `Accept` header (ADR-007).
8. **First-run guard**: while there are no local users, every web request
   is sent to `/setup`.

Rate limiting is added by its own initializer (§9).

### 2.5 Rendering

Handlers render through the `Views` extractor, which injects the theme,
accent colour and shared context into every template once (ADR-027). Templates extend
`layouts/base.html`; reusable pieces live in `partials/` (post card,
comment, vote buttons, follow button) and `icons/`. HTMX endpoints such as
voting return fragments only.

---

## 3. Request flows

### 3.1 Creating a post locally

1. `POST /c/{slug}/posts` with a CSRF token.
2. The handler checks the session, community membership, bans, suspension
   and the posting mode, then validates the input.
3. `models::posts` inserts the row; Markdown is rendered and sanitised.
4. `federation::outbound` builds a `Create{Page}` activity and calls
   `enqueue_activity`, which writes one `outbound_activities` row with the
   deduplicated follower inboxes. The handler never sends HTTP itself
   (ADR-019).
5. The response is a redirect, or an HTMX fragment for HTMX requests.
6. Within seconds the delivery poller hands the row to the
   `deliver_activity` worker, which signs and sends it through
   `activitypub_federation`.

### 3.2 Receiving federation

1. A remote server `POST`s to `/u/{username}/inbox` or `/c/{slug}/inbox`.
2. `activitypub_federation::receive_activity` verifies the HTTP signature
   and fetches the sending actor if needed.
3. The activity is checked against `received_activities` and dropped if it
   was already processed (ADR-009, ADR-020).
4. It is dispatched by type: `Follow` (auto-accepted, ADR-008), `Undo`,
   `Create`, `Update`, `Delete`, `Like`/`Dislike`, `Announce`.
5. Remote content is written only to the `remote_*` cache tables, never to
   the local `posts`, `comments` or `users` tables (ADR-017).
6. When a community receives a `Create`, the community actor sends an
   `Announce` of it to all its followers, as Lemmy does.

---

## 4. Data model

Migrations live in `migration/src/` (`m001` to `m010`); the generated
SeaORM entities are in `src/models/_entities/`. All primary keys are UUIDs
except where a natural ActivityPub id is the key, and all timestamps are
`TIMESTAMPTZ`. SPEC.md §4 lists every column.

### 4.1 Local content

| Table | Holds | Notes |
|---|---|---|
| `users` | Local accounts | username, optional email and password hash, profile, RSA keypair, `is_admin`, `is_banned`, `suspended_at`, `email_verified`, `standing_score`, appearance (`theme`, `accent_color`) |
| `communities` | Communities | slug, name, Markdown description, keypair, `is_nsfw`, `posting_restricted` (moderators only) |
| `posts` | Local posts | `community_id` is NULL for profile posts; denormalised `score` and `comment_count` (ADR-006); `is_deleted`, `is_removed`, `is_locked`, `is_pinned` |
| `comments` | Local comments | `parent_id` and an `ltree` `path` for threading (ADR-005) |
| `votes` | +1 / −1 votes | by a local user or a remote actor (`remote_actor_ap_id`) |
| `community_members` | Membership | `role` is `member` or `moderator` |
| `community_bans` | Per-community bans | who banned, optional reason (ADR-028) |
| `follows` | User and community follows | local or remote follower and target, the `Follow` activity id, `accepted` |
| `email_verifications` | Verification tokens | random token, expiry, `verified_at` |
| `instance_settings` | Single row of instance settings | name, open registrations, email verification, community creation rules (ADR-012) |

### 4.2 Federation

| Table | Holds | Notes |
|---|---|---|
| `outbound_activities` | Delivery queue | activity JSON, signing actor, `target_inboxes TEXT[]`, `state`, `attempts`, `next_attempt_at`, `last_error` |
| `received_activities` | Ids of processed inbound activities | idempotency; kept 30 days |
| `remote_actors` | Cached remote users and groups | keys, inboxes, `instance_domain`, raw JSON, `stale_after` (1 day) |
| `remote_posts` | Cached remote posts | linked to a local community when posted there; `stale_after` (7 days) |
| `remote_comments` | Cached remote comments | linked to a local post or a remote one |

### 4.3 The `community_feed` view

`community_feed` is a `UNION ALL` of local `posts` (joined to `users`) and
`remote_posts` (joined to `remote_actors`), with an `origin` column of
`local` or `remote`. Community feeds read from it, so a feed shows local and
federated posts together while the two stay in separate tables.

### 4.4 Pagination

Every list uses keyset (cursor) pagination on `(score, published_at)` or
`published_at`, passed as an opaque `?cursor=` parameter, never `OFFSET`
(ADR-018). Indexes for the hot paths are in `m010_performance_indexes`.

---

## 5. ActivityPub federation

### 5.1 What the crate does

`activitypub_federation` signs outbound requests, verifies inbound HTTP
signatures, fetches and caches remote actors, builds WebFinger responses
and guards against fetching private addresses. Taverna never hand-rolls any
of these and never calls `reqwest` directly in federation code.

Taverna implements the crate's `Object` and `Actor` traits for its users,
communities, posts and comments (`federation/traits.rs`), the protocol
types (`federation/protocol.rs`), the inbound activity handlers
(`federation/activities.rs`) and the outbound builders
(`federation/outbound.rs`, ADR-023).

### 5.2 Actors

| Local entity | Actor type | URL | Federates with |
|---|---|---|---|
| User | `Person` | `https://host/u/{username}` | Mastodon, Pleroma, Lemmy users |
| Community | `Group` | `https://host/c/{slug}` | Lemmy and other group software |

Users and communities each have their own RSA-2048 keypair (ADR-003,
ADR-004). A community signs the activities it sends, such as `Announce`;
users sign their own posts, comments and votes. Group actors carry Lemmy's
`postingRestrictedToMods`.

### 5.3 Activities

| Direction | Activity | Object | When |
|---|---|---|---|
| Out | `Create` | post / comment | a user posts or comments |
| Out | `Announce` | `Create` | a community relays a new post to its followers |
| Out | `Like` / `Dislike` | post / comment | a user votes |
| Out | `Undo` | vote / `Follow` | a vote is removed, a user unfollows |
| Out | `Update` | post / comment | an edit |
| Out | `Delete` | post / comment | the author deletes, or a moderator removes |
| Out | `Follow` / `Accept` | actor / `Follow` | following, and auto-accepting followers |
| In | the same set | | handled idempotently; remote content goes to `remote_*` |

Profile posts are addressed to the user's followers and cc the public
collection; community posts to the community's followers. Comments set
`inReplyTo` to the parent object.

### 5.4 Delivery

Handlers only enqueue (`enqueue_activity`). An in-process loop in
`initializers/federation_jobs.rs` picks due rows every 5 seconds and hands
them to the `deliver_activity` worker (ADR-021), which shares one HTTP
client. A failed delivery is retried after 10 s, 30 s, 2 min and 10 min;
after the fifth failed attempt the row is marked failed. Follower inboxes
are deduplicated so a server with a shared inbox gets one request.

### 5.5 Remote cache expiry

Once a day the `expire_remote_cache` worker deletes `remote_comments`,
`remote_posts` and unreferenced `remote_actors` past their `stale_after`,
and old `received_activities` rows. It never touches local tables.

### 5.6 Discovery

- WebFinger: `GET /.well-known/webfinger?resource=acct:name@host` resolves
  both users and communities.
- NodeInfo: `GET /.well-known/nodeinfo` links to `GET /nodeinfo/2.0`, which
  reports `software.name = "taverna"`, the version, the `activitypub`
  protocol and cached usage counts.
- Objects: `GET /posts/{id}` and `GET /comments/{id}` serve posts and
  comments as ActivityPub JSON.

---

## 6. Routes

`src/app.rs` is the source of truth. User and community URLs are shared
between HTML and ActivityPub through content negotiation.

### 6.1 Web

| Method | Path | Purpose |
|---|---|---|
| GET | `/` | Home feed |
| GET/POST | `/setup` | First-run wizard |
| GET/POST | `/register`, `/login`; POST `/logout` | Accounts |
| GET | `/verify-email`; POST `/resend-verification` | Email verification |
| POST | `/theme` | Set the theme cookie |
| POST | `/settings/appearance` | Save the account's default theme and accent colour |
| GET / POST | `/c`, `/c/new` | List and create communities |
| GET | `/c/{slug}` | Community feed |
| GET / POST | `/c/{slug}/new`, `/c/{slug}/posts` | New post |
| POST | `/c/{slug}/join`, `/c/{slug}/leave` | Membership |
| GET/POST | `/c/{slug}/settings` | Community settings and bans |
| POST | `/c/{slug}/bans`, `/c/{slug}/bans/{username}/delete` | Ban / unban |
| GET | `/c/{slug}/posts/{id}` | Post and comment thread |
| GET/POST | `…/posts/{id}/edit`; POST `…/vote`, `…/delete`, `…/remove`, `…/pin`, `…/lock` | Post actions |
| POST | `…/posts/{id}/comments`; `…/comments/{cid}/vote`, `/edit`, `/delete`, `/remove` | Comment actions |
| GET | `/u/{username}`, `/u/{username}/posts`, `/u/{username}/comments` | Profile |
| GET/POST | `/u/{username}/new`, `/u/{username}/posts` | Profile post |
| POST | `/u/{username}/follow`, `/u/{username}/unfollow` | Follow |
| GET/POST | `/settings`; POST `/settings/password` | Account settings |
| GET/POST | `/admin`, `/admin/settings`; POST `/admin/users/{username}/{ban,unban,suspend,unsuspend,force-verify}`, `/admin/communities/{slug}/delete` | Administration |

### 6.2 Federation

| Method | Path | Purpose |
|---|---|---|
| GET | `/.well-known/webfinger`, `/.well-known/nodeinfo`, `/nodeinfo/2.0` | Discovery |
| GET | `/u/{username}`, `/c/{slug}` with an AP `Accept` | Person / Group actor |
| POST | `/u/{username}/inbox`, `/c/{slug}/inbox` | Inboxes |
| GET | `/u/{username}/outbox`, `/followers`, `/following` | User collections |
| GET | `/c/{slug}/outbox`, `/c/{slug}/followers` | Community collections |
| GET | `/posts/{id}`, `/comments/{id}` | Objects |

---

## 7. Frontend

The UI is server-rendered HTML from Tera templates with HTMX for small
interactions (voting, follow buttons). There is no JavaScript framework
(ADR-002); every form works without JavaScript. `htmx.min.js` is served
locally from `assets/static/`.

Three themes, light, dark and sepia, are defined as `[data-theme]` blocks
of CSS custom properties in `assets/static/theme.css`. The theme is stored
in the `taverna_theme` cookie (one year, `SameSite=Lax`), set by
`POST /theme`, and written to `<html data-theme="…">` by the base layout.
Templates use `var(--…)` properties only, never hard-coded colours.

Logged-in users pick a default theme and an accent colour (any RGB colour
from a native colour picker) under Settings → Appearance. Both are stored
on the account (`users.theme`, `users.accent_color`, null meaning the
default) and copied into the `taverna_theme` and `taverna_accent` cookies
when saved and on every login, so the choice follows the account to a new
browser while page rendering still reads cookies only. From the one accent
colour, `src/common/accent.rs` derives the accent tokens (`--primary`,
`--primary-container`, `--on-primary`, `--primary-soft`, `--upvote`,
`--cover-a`) for all three themes and the base layout inlines them after
`theme.css`. `--primary` is pushed towards white (dark) or black
(light/sepia) until it has 4.5:1 contrast with the surface, so any colour
stays readable.

---

## 8. Accounts, moderation and onboarding

### 8.1 Roles

| Role | Scope | Can |
|---|---|---|
| User | Own content | post, comment, vote, follow, edit and delete own content |
| Moderator | One community | remove posts and comments, pin and lock posts, ban and unban users, edit community settings |
| Admin | Instance | everything moderators can, plus ban, suspend and force-verify users, delete communities, edit instance settings |

Suspension is temporary and reversible; a ban is permanent, globally or per
community (ADR-014). A moderator removal is sent to other servers as a
`Delete` from the community.

### 8.2 First run

While the instance has no local users, every web request is redirected to
`/setup`, where the operator names the instance and creates the first admin
(ADR-015). The check re-runs on submit, so the wizard cannot be replayed.

### 8.3 Email verification and community creation

Email verification sends a random token through Loco's mailer (or logs it
when no mailer is configured). Whether verification is required, and who may
create communities, is set in `instance_settings` from the admin dashboard
and read on every request. Eligibility combines account age, verified email,
a minimum standing score and disqualifications for removed content or a
current suspension; a user must pass every enabled check and is shown the
reasons they fail. The standing score moves with votes on the user's content
and is floored at −100 in the database update itself (ADR-013).

---

## 9. Security

- **Sessions:** a 1-hour access token and a 7-day refresh token, both HS256
  JWTs in httpOnly cookies, kept apart by a `kind` claim (ADR-022).
- **Passwords:** argon2.
- **CSRF:** every form uses a double-submit token plus an Origin/Referer
  check against `base_url`.
- **Federation:** HTTP signatures on every outbound inbox POST and verified
  on every inbound one; private and loopback addresses are never fetched.
- **Markdown:** rendered with `pulldown-cmark`, then sanitised with `ammonia`
  before display. Only sanitised HTML is marked `| safe` in templates.
- **Rate limiting:** per client, keyed on the socket peer. Inboxes allow
  5 requests/s (burst 20), `/login` and `/register` 10/s (burst 30), other
  routes 60/s (burst 600). `X-Forwarded-For` is trusted only from
  `TRUSTED_PROXIES` (ADR-026).
- **Errors:** handlers return `Result` and never `unwrap()` on request paths.

---

## 10. Configuration and deployment

Loco reads `config/<environment>.yaml`; Taverna's own settings live in its
`settings:` block (base URL, domain, federation, rate limiting) and are read
into `TavernaConfig`. Values come from environment variables through
`<%= get_env(...) %>`. See [CONFIGURATION.md](CONFIGURATION.md) for every
setting and [DEPLOY.md](../DEPLOY.md) for production setup.

A small deployment needs one VPS (2 vCPU, 2 GB RAM), PostgreSQL 16 on the
same host or a separate one, and a reverse proxy such as Caddy or nginx for
TLS. The server binary is a single `cargo build --release` artifact, but it
reads `config/` and `assets/` from its working directory, so those ship
alongside it.

---

## 11. Roadmap beyond v1

| Feature | Notes |
|---|---|
| Image and video uploads | S3-compatible storage, thumbnails |
| Full-text search | `pg_trgm` first, a search engine later |
| Bluesky / AT Protocol bridge | relay posts, pull back replies |
| OAuth (GitHub, Forgejo) | additional login providers |
| Private communities | invite-only, not federated |
| Notifications | in-app and email digest |
| Markdown preview | live preview in forms |
| Custom emoji | per-instance packs |
| Domain blocking | `BLOCKED_DOMAINS` is parsed but not yet enforced |
