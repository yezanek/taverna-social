# Architecture Decision Log

This file records WHY specific decisions were made. If you are an AI agent reading this, do not reverse these decisions without explicit user instruction.

---

## ADR-001: PostgreSQL for Everything (No Redis)

**Decision:** Use PostgreSQL for the job queue (pgmq) and federation cache, not Redis.

**Reason:** Self-hosting is simpler with one service. pgmq provides durable queues with at-least-once delivery. The federation cache (federation_objects table) needs persistence across restarts anyway. Redis would add operational complexity for no meaningful performance gain at the expected scale.

**Do not add Redis.**

---

## ADR-002: No JavaScript Framework

**Decision:** HTMX + Tera templates only. No React, Vue, Svelte, or similar.

**Reason:** The project is Rust-first. A JS framework would require a Node.js build step, TypeScript compilation, and a separate development server. HTMX achieves the necessary interactivity (voting, infinite scroll, inline forms) with server-rendered HTML and minimal JavaScript. This keeps the entire application in one language ecosystem.

**Do not add a JavaScript build step or framework.**

---

## ADR-003: RSA-2048 for ActivityPub Keys

**Decision:** Use RSA-2048 keypairs for HTTP signatures, not Ed25519.

**Reason:** Lemmy and Mastodon both use RSA-SHA256 for HTTP signature verification (draft-cavage-http-signatures-12). Ed25519 is not supported by the major Fediverse implementations as of 2024. Interoperability is more important than key size modernity here.

**Do not switch to Ed25519 without verifying Lemmy and Mastodon support it.**

---

## ADR-004: Separate Keypairs for Users and Communities

**Decision:** Each community has its own RSA keypair, separate from any user keypair.

**Reason:** In Lemmy's federation model, the community Group actor signs the Announce activities it sends to followers. If communities shared a user's key, the key ID in the signature wouldn't match the community actor, breaking signature verification on remote servers.

**Each community must have its own keypair.**

---

## ADR-005: ltree for Comment Threading

**Decision:** Use PostgreSQL ltree extension for comment paths.

**Reason:** Adjacency list queries (parent_id recursion with CTEs) become expensive at depth. ltree allows fetching an entire thread subtree with a single index scan: `WHERE path <@ 'root_path'`. It also makes sorting by thread path trivial.

**Do not replace ltree with recursive CTEs or application-side threading.**

---

## ADR-006: Denormalized score on posts and comments

**Decision:** Maintain a `score` column on posts and comments that is recalculated after each vote.

**Reason:** Counting votes on every feed query would make the community feed slow (aggregate over votes table per post). Denormalization trades write complexity for read performance. The feed is read far more often than it is written.

**Always call recalculate_score after any vote change.**

---

## ADR-007: Content Negotiation (same URL, dual response)

**Decision:** `/u/:username` and `/c/:slug` serve both HTML (for browsers) and AP JSON-LD (for federation), using the Accept header to decide.

**Reason:** This is how Mastodon and Lemmy do it. Remote servers fetch the actor by its canonical URL. That URL must return AP JSON when requested with the right Accept header. Having separate `/ap/u/:username` URLs would break WebFinger discovery, which points to the canonical actor URL.

**Do not split HTML and AP JSON into separate URL paths.**

---

## ADR-008: Auto-accept all follows

**Decision:** When a Follow activity arrives, immediately send Accept{Follow}.

**Reason:** Taverna communities and profiles are public. Manual approval adds friction for legitimate federation. If a moderation need arises, banning the remote instance's domain is more effective than per-follow approval.

**Do not implement manual follow approval in v1.**

---

## ADR-009: Idempotent inbox processing

**Decision:** Receiving the same activity twice must be a no-op, not an error or duplicate insert.

**Reason:** The Fediverse is eventually consistent and unreliable. Remote servers retry deliveries. Network failures cause duplicate sends. Idempotency (checking ap_id before insert) is required for correctness.

**Always check ap_id existence before inserting any federated object.**

---

## ADR-012: Single-row instance_settings table

**Decision:** Store instance-wide configuration in a single-row table enforced
by `CHECK (id = true)` on a boolean primary key.

**Reason:** Avoids a settings key-value table (hard to type-check in Rust) and
an application-level singleton (hard to invalidate). A single strongly-typed row
with named columns is readable, safe, and queryable with SeaORM without special
handling. The CHECK constraint makes it impossible to insert a second row.

**Do not replace with a key-value store or environment variables for
runtime-configurable settings.** Environment variables require a restart to
change. Key-value tables lose type safety.

---

## ADR-013: Standing score floor at -100

**Decision:** `standing_score` is floored at -100 via `GREATEST(-100, score + delta)`
in the UPDATE statement, enforced at the database level.

**Reason:** Without a floor, a heavily downvoted or heavily moderated user
accumulates a score that can never be recovered from in practice, which is a
moderation abuse vector (mass-downvote campaigns). -100 is recoverable with
consistent positive contributions. The floor is enforced in SQL, not Rust, so it
applies regardless of which code path triggers the update.

**Do not enforce the floor only in Rust.** A future migration or admin tool might
update scores directly.

---

## ADR-014: Suspension vs. Ban

**Decision:** Two distinct concepts: `suspended_at TIMESTAMPTZ` (reversible,
affects eligibility) and `is_banned BOOL` (permanent, blocks all access).

**Reason:** Moderation needs nuance. A ban is a terminal action — the user
cannot log in. A suspension is a temporary or conditional restriction that
affects eligibility for trust-gated actions (like creating communities) but
does not prevent the user from participating otherwise. Keeping them as separate
columns avoids ambiguous states and makes the admin UI clearer.

**Do not conflate suspension and banning** by reusing `is_banned` for both.

---

## ADR-015: First-run detection via user count

**Decision:** Detect first-run state by counting local users in the database,
not via a config file flag or a "setup_complete" table.

**Reason:** A config file flag can get out of sync with reality (e.g. if the
database is wiped but the flag file is not). The user count is the ground truth.
If there are zero local users, setup has not been completed regardless of any
other state. The result is cached for 60 seconds per process to avoid a DB query
on every request.

**Do not use a setup_complete flag file or environment variable for this.**

---

## ADR-016: Config files use TOML, not YAML

**Decision:** Loco configuration files are `config/development.toml` and
`config/production.toml`, not `.yaml`.

**Reason:** TOML is Rust's native configuration format, used by Cargo itself.
It is unambiguous about types (no implicit string/number coercion), has no
indentation-sensitivity foot-guns, and is what Loco supports natively. YAML's
flexibility is a liability for config files that are read by tools and humans alike.

**Do not add YAML config files. If loco new generates .yaml files, rename them.**

---

## ADR-017: Remote federated content lives in dedicated tables

**Decision:** Content received from remote federated instances lives in
`remote_actors`, `remote_posts`, and `remote_comments` — never in `users`,
`posts`, or `comments`.

**Reason:** Local content is authoritative and permanent. Remote content is a
cache — it can go stale, it can be re-fetched, and it must be purgeable without
any risk to local data. Mixing them in the same table makes expiry impossible
without careful filtering, creates index bloat from mixed workloads, and makes
it easy for a bug in the expiry worker to accidentally delete local content.
Separation makes the invariant structural: the expiry worker operates only on
`remote_*` tables by definition.

**Do not insert remote content into the local posts, comments, or users tables.**

---

## ADR-018: Keyset (cursor) pagination everywhere, no OFFSET

**Decision:** All paginated queries use cursor-based (keyset) pagination.
`OFFSET N` is never used in feed or list queries.

**Reason:** `OFFSET N` requires the database to scan and discard N rows before
returning results. At page 100 with 25 items per page, this is 2,500 discarded
rows per query. On a busy community feed this is measurable latency. Keyset
pagination using `WHERE (score, published_at) < (cursor_score, cursor_time)`
is O(log N) regardless of page depth, because it uses the index directly.

**Do not introduce OFFSET-based pagination for any new endpoint.**

---

## ADR-019: Federation delivery is always async (never in request handlers)

**Decision:** `SendActivityTask::send` is only called from background worker
code (`src/workers/`). Request handlers enqueue to `outbound_activities` and
return immediately.

**Reason:** Remote HTTP requests are slow and unreliable. A community with
1,000 followers across 200 instances would require 200 outbound HTTP requests
per post. Doing this synchronously in a request handler would block the response
for 10–30+ seconds and exhaust the web server's connection pool under any real
load. The worker handles retries, backoff, and failure without affecting the
user-facing response time.

**No exceptions. Even for single-recipient activities (e.g. Accept{Follow}),
enqueue rather than send inline.**

---

## ADR-020: Remote actors in follows and votes; typed idempotency table

**Decision:** Phase 8 moves remote people out of `users` (ADR-017), so
`follows` gains `remote_follower_ap_id` / `remote_following_ap_id` and `votes`
gains `remote_actor_ap_id`, all referencing `remote_actors(ap_id)`.
`follower_id` and `votes.user_id` become nullable, with CHECK constraints that
exactly one side is set. Inbox idempotency (ADR-009) moves from the dropped
`federation_objects` table to `received_activities(ap_id, received_at)`, which
the daily expiry job trims after 30 days.

**Reason:** Without these columns, a remote Follow or Like has nowhere to
live once remote actors are not users. The expiry job never deletes a remote
actor that a follow or vote still references.

**Do not point follows or votes at placeholder rows in `users`.**

---

## ADR-021: Outbound delivery is dispatched by an in-process poller

**Decision:** Handlers call `enqueue_activity` (ADR-019). A background loop
started by `FederationJobsInitializer` claims due `outbound_activities` rows
every 5 seconds (`FOR UPDATE SKIP LOCKED` plus a 5-minute lease) and hands
each one to `DeliverActivityWorker`. The same initializer runs the remote
cache expiry once a day. Neither loop runs in the test environment.

**Reason:** Before Phase 8 nothing ever picked up rows from
`outbound_activities`, so no activity was actually delivered. pgmq is
installed but not used by the code, and Loco's scheduler needs a separate
process; an in-process loop keeps deployment to one binary plus PostgreSQL.
CI no longer builds pgmq for the test job, since no code or migration uses it.

---

## ADR-016 note (Phase 8)

Loco (0.16 and 1.x) reads `config/<env>.yaml`; the repository's config files are YAML
(`development.yaml`, `test.yaml`, and the `production.example.yaml` template).
ADR-016 describes an intent that the framework does not support.

---

## ADR-022: Session tokens carry a `kind`, and the refresh token is used

**Decision:** JWT claims include `kind: "access" | "refresh"`, and
`verify_token` checks it. The `refresh_session` middleware renews a missing
or expired access token from a valid refresh token: it swaps the new token
into the request's `Cookie` header (so `AuthUser` sees it) and sets it on
the response. `/logout` and `/static/` are skipped.

**Reason:** SPEC §3.4 specifies a 1h access token and a 7d refresh token,
but the refresh token was never read, so every session ended after an hour.
Both tokens also had identical claims, so a 7-day refresh token was accepted
as an access token.

---

## ADR-023: Outbound activities are built in `federation::outbound`

**Decision:** Controllers build Notes and activities with the helpers in
`src/federation/outbound.rs` and queue them with `outbound::send*`, which
log enqueue failures instead of failing the request. Notes carry HTML in
`content` and the markdown in `source`; posts set `attributedTo` to the
author and `audience` to the community. A Follow's id is deterministic
(`{actor}/follows/{target_id}`) so `Undo{Follow}` names the same object.
The Announce of a remote post wraps the original `Create` (SPEC §6.4).

**Reason:** The same JSON was hand-written in a dozen handlers and had
drifted: posts were attributed to the community (which our own inbox
rejects), `Undo{Follow}` referenced an id that was never sent, and the
Announce carried a bare Note.

---

## ADR-024: Framework idioms after the Loco 1.2 upgrade

**Decision:**
- `App::after_context` reads `TavernaConfig` and builds the single
  `FederationConfig`, and keeps both in Loco's `shared_store`. The router
  layers and `DeliverActivityWorker` take them from there, so the inbox
  middleware and outbound delivery share one HTTP client.
- Controllers use `?` on `DbErr` (Loco maps `Error::DB` to a 500 and logs
  the cause). Other internal errors go through `Error::wrap`, never a bare
  `Error::InternalServerError` that drops the cause.
- 401 means "not signed in". A signed-in user who may not do something, or
  a request with a bad CSRF token, gets a 403 (`controllers::errors`).
- Actor documents come from the crate's `Object::into_json` on `DbUser` /
  `DbCommunity`, wrapped in `FederationJson(WithContext::new(..))`, so the
  actor endpoint, `Update{Person}` and the crate's own fetches agree. All
  ActivityPub responses use `application/activity+json`.
- `TavernaActivity::verify` checks that an activity's id is on its actor's
  domain (`verify_domains_match`).

**Reason:** The hand-written JSON for actors and outboxes had drifted from
the trait implementations (outboxes sent raw markdown and attributed
community posts to the community), and blanket `InternalServerError`
mapping hid every database error from the logs.

## ADR-025: Taverna settings in Loco's `settings:` block; NodeInfo uses `ctx.cache`

**Decision:**
- `TavernaConfig` is deserialized from `ctx.config.settings` (the
  `settings:` block of `config/<env>.yaml`) in `App::after_context`, instead
  of being read from environment variables in code. The YAML pulls
  per-deployment values from the same environment variables as before
  (`BASE_URL`, `TAVERNA_DOMAIN`, `FEDERATION_ENABLED`, `BLOCKED_DOMAINS`,
  `HTTP_SIGNATURE_EXPIRY_SECONDS`, `RATE_LIMITING_ENABLED`) with `get_env`.
  The undocumented `DOMAIN` fallback for `TAVERNA_DOMAIN` is gone.
- NodeInfo caches its user and post counts in Loco's `ctx.cache`
  (`get_or_insert_with_expiry`, 5 minutes). Every config sets
  `cache: { kind: InMem }`, because Loco's default null cache would turn the
  caching off.

**Reason:** One file lists every setting and its default, and the
hand-rolled `static Mutex` cache duplicated what Loco already provides.


## ADR-026: Rate limits are keyed on the socket peer; proxies are opt-in

**Decision:** The rate limiter keys each client on the socket peer address
(`ConnectInfo<SocketAddr>`). `X-Forwarded-For` (or `X-Real-IP`) is read
only when the peer is listed in `settings.rate_limiting.trusted_proxies`
(`TRUSTED_PROXIES`, IPs or CIDRs), and then the header is walked from the
right past trusted hops to the first untrusted one.

**Reason:** Trusting the left-most `X-Forwarded-For` entry let any client
reached directly pick a fresh bucket per request, and requests without the
header all shared one `127.0.0.1` bucket.

## ADR-027: Handlers render through the `Views` extractor

**Decision:** HTML handlers take `views: crate::views::Views`, an extractor
that bundles the Tera view engine with the request's `CurrentTheme`, and
call `views.render(template, data)`. `Views` adds `theme` to every
template's context, so handlers no longer take `Extension<CurrentTheme>`.

**Reason:** SPEC §3.5 wants the theme available to every template; passing
it by hand through about 30 handlers was easy to forget and noisy.

## ADR-028: Community bans are managed from the settings page

**Decision:** `POST /c/{slug}/bans` takes `username` and an optional
`reason` as form fields, and `POST /c/{slug}/bans/{username}/delete` lifts a
ban. They replace `/c/{slug}/ban/{username}` and `/c/{slug}/unban/{username}`.
The community settings page lists current bans with Unban buttons and has a
form to add one; the feed links moderators to that page. Banning an
already banned user updates the reason instead of failing.

**Reason:** With the username in the path, a plain HTML form could not
target the old routes without JavaScript, so no page linked to them.
