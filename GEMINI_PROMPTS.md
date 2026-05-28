# How to Use gemini-cli as QA

This file describes how to run gemini-cli as a quality-assurance agent after
opencode implements each phase.

opencode writes the code. gemini-cli reviews, verifies, and flags issues.

---

## Role Division

| Tool | Role |
|---|---|
| opencode | Implements each phase — writes Rust code, templates, migrations |
| **gemini-cli** | **QA — reviews output, runs tests, catches issues, requests fixes** |

---

## Installing gemini-cli

```bash
# Node.js (if not already installed)
sudo apt install -y nodejs npm

# gemini-cli
npm install -g @google/gemini-cli
gemini auth
```

Or via Linuxbrew if you use it:
```bash
brew install node
npm install -g @google/gemini-cli
gemini auth
```

Use the most capable available model for review work:
```bash
gemini --model gemini-2.5-pro
```

---

## QA Workflow: After Each opencode Phase

### Step 1 — Compile gate (you run this manually)

```bash
cargo check
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

If any fail, go back to opencode first:
```
cargo test failed with:
[paste output]
Fix these failures without deleting any tests.
```

### Step 2 — Start gemini-cli in the repo root (`taverna-social/`)

```bash
gemini --model gemini-2.5-pro
```

### Step 3 — Run the phase QA prompt

Copy the prompt for your completed phase from the sections below and paste it into gemini-cli.

### Step 4 — If gemini-cli finds issues

Copy the output and take it to opencode:
```
gemini-cli found the following issues after Phase [N]:

[paste gemini-cli output]

Fix every CRITICAL issue. For MEDIUM issues, fix them too unless I say otherwise.
Show me a plan before making changes.
```

### Step 5 — Re-run the compile gate after fixes

```bash
cargo check && cargo test && cargo clippy -- -D warnings
```

Only proceed to the next phase when all gates pass and no CRITICAL issues remain.

---

## Phase 1 QA Prompt

```
You are a QA engineer reviewing Phase 1 of the Taverna project.

Read AGENTS.md, SPEC.md, and PHASE_1.md.

Then verify:

DELIVERABLES — check every item in the PHASE_1.md deliverables checklist exists.

DATABASE SCHEMA — open each migration file and verify:
- users and communities tables have last_fetched_at TIMESTAMPTZ (required by activitypub_federation crate)
- All primary keys are UUID, not INTEGER
- All timestamps are TIMESTAMPTZ (not TIMESTAMP)
- All indexes from SPEC.md Sections 4.2–4.9 are present
- ltree extension is created in m001_extensions.rs
- outbound_activities uses TEXT[] for target_inboxes (not a separate table per inbox)

ENTITIES — verify src/models/_entities/ is populated with generated SeaORM files.

MIGRATIONS — verify migration/src/lib.rs registers all 6 migrations in order.

Run: cargo check
Run: cargo test

Report issues as CRITICAL (blocker), MEDIUM (should fix), or LOW (style).
```

---

## Phase 2 QA Prompt

```
You are a QA engineer reviewing Phase 2 of the Taverna project.

Read AGENTS.md, SPEC.md, and PHASE_2.md.

Verify:

KEYPAIR GENERATION
- Open src/models/users.rs. The generate_keypair() method must call
  generate_actor_keypair() from the activitypub_federation crate.
- Search for "use rsa::" in src/. If found: CRITICAL — wrong crate.

PASSWORD HASHING
- Find where passwords are hashed. Must use argon2id via the argon2 crate.
- If bcrypt, md5, sha256, or plaintext: CRITICAL.

LOGIN ERROR MESSAGES
- Open src/controllers/auth.rs, POST /login handler.
- The error response must NOT say "email not found" or "wrong password".
- Acceptable: "Invalid credentials" or similar. MEDIUM if it reveals which field failed.

JWT
- Access token expires in 1 hour. Refresh token expires in 7 days.
- Both stored in httpOnly cookies. If stored elsewhere: CRITICAL.

FEDERATION SETUP
- Open src/app.rs. FederationConfig must be built and FederationMiddleware applied.
- If missing: CRITICAL (Phase 4 will fail at runtime).

LOCAL USER FLAGS
- New local users must have: is_local=true, last_fetched_at=None.

Run: cargo check
Run: cargo test

Report CRITICAL / MEDIUM / LOW.
```

---

## Phase 3 QA Prompt

```
You are a QA engineer reviewing Phase 3 of the Taverna project.

Read AGENTS.md, SPEC.md Section 3.5, and PHASE_3.md.

Verify:

THEME — CSS FILE
- Open assets/static/theme.css.
- Must contain three blocks: [data-theme="light"], [data-theme="dark"], [data-theme="sepia"].
- Each must define at minimum these properties:
  --bg-primary, --bg-secondary, --bg-card, --text-primary, --text-secondary,
  --text-muted, --border, --accent, --accent-hover, --upvote, --downvote,
  --link, --link-hover, --nav-bg, --nav-text, --score-positive, --score-negative
- Missing blocks or properties: CRITICAL.

THEME — BASE TEMPLATE
- Open views/layouts/base.html.
- <html> tag must be: <html lang="en" data-theme="{{ theme }}">
- theme.css must be linked: <link rel="stylesheet" href="/static/theme.css">
- Nav must contain the theme switcher form (POST to /theme, three buttons).
- If data-theme is hardcoded (e.g. data-theme="light"): CRITICAL.

THEME — HANDLER
- Open src/controllers/theme.rs.
- POST /theme must set cookie: name=taverna_theme, SameSite=Lax, Max-Age=31536000.
- Cookie must NOT be HttpOnly.
- Invalid theme values must default to "light".
- Must redirect to Referer or / if no Referer.

THEME — MIDDLEWARE
- Open src/initializers/theme_middleware.rs.
- Must read taverna_theme cookie and inject CurrentTheme into request extensions.
- Must be in the middleware stack before all handlers.

HARDCODED COLOURS
- Search views/ for hex colours (#[0-9a-fA-F]{3,6}) and rgb(...) values.
- Any found in .html files (not in theme.css): MEDIUM.

HTMX VOTE ENDPOINTS
- Vote handlers must return HTML fragments only (no full page, no theme injection).
- Check by looking for HX-Request header detection in vote handlers.

LTREE PATHS
- Open src/models/comments.rs, the create() method.
- Top-level path: format!("{}.{}", post_seg, comment_seg)
- Reply path: format!("{}.{}", parent.path, comment_seg)
- Each segment must be prefixed with "c" (ltree labels must start with a letter).

SCORE RECALCULATION
- After every vote in src/models/votes.rs, recalculate_score must be called.
- If it's missing: CRITICAL (scores will be stale).

Run: cargo check
Run: cargo test (including theme tests)

Report CRITICAL / MEDIUM / LOW.
```

---

## Phase 4 QA Prompt

```
You are a QA engineer reviewing Phase 4 of the Taverna project.

Read AGENTS.md, SPEC.md Section 6, and PHASE_4.md.

Verify:

NO HAND-ROLLED SIGNATURES
- Search src/ for: use rsa::, use sha2::, use base64:: in federation context.
- If found: CRITICAL — the activitypub_federation crate handles this.

INBOX HANDLERS USE receive_activity
- Open src/controllers/federation/inbox.rs.
- Both user_inbox and community_inbox must call receive_activity::<...>() from the crate.
- If they verify signatures manually: CRITICAL.

OUTBOUND USES SendActivityTask
- Search src/ for reqwest::Client and reqwest::post in federation code.
- If raw reqwest is used to send to remote inboxes: CRITICAL.

WEBFINGER USES CRATE HELPERS
- Open src/controllers/federation/webfinger.rs.
- Must call build_webfinger_response() and extract_webfinger_name().
- If it manually builds the JRD JSON: MEDIUM.

verify_domains_match IN EVERY Object::verify
- Open each Object trait implementation (person.rs, group.rs, note.rs).
- Each verify() must call verify_domains_match(json.id.inner(), expected_domain)?.
- Missing calls: CRITICAL (enables cross-domain actor spoofing).

ANNOUNCE PATTERN
- Find Create's ActivityHandler::receive implementation.
- When the inbox owner is a Community, after saving the post, it must:
  (a) Build an Announce activity with the community as actor
  (b) Call SendActivityTask::send with the community as signing actor
  (c) Send to the community's follower inboxes
- If either (a) or (b) is missing: CRITICAL (Lemmy federation broken).

IDEMPOTENCY IN ALL receive() METHODS
- Check every ActivityHandler::receive.
- Each must check if the object exists by ap_id before inserting.
- Missing checks: CRITICAL (duplicate data on delivery retries).

FederationMiddleware APPLIED
- Open src/app.rs.
- The AP route group must have .layer(FederationMiddleware::new(...)).
- If missing: CRITICAL (runtime panics on Data<T> extraction).

CONTENT NEGOTIATION USES FEDERATION_CONTENT_TYPE
- Search for hardcoded "application/activity+json" string.
- Should use the FEDERATION_CONTENT_TYPE constant from the crate.
- If hardcoded: LOW.

NodeInfo CORRECTNESS
- Open src/controllers/federation/nodeinfo.rs.
- /nodeinfo/2.0 must return: "name": "taverna" and "protocols": ["activitypub"].
- If name is wrong or protocols is missing: MEDIUM.

Run: cargo check
Run: cargo test

Then manually test:
  cargo loco start &
  curl "http://localhost:3000/.well-known/webfinger?resource=acct:admin@localhost"
  curl -H "Accept: application/activity+json" "http://localhost:3000/u/admin"
  curl "http://localhost:3000/nodeinfo/2.0"

Report CRITICAL / MEDIUM / LOW.
```

---

## Phase 5 QA Prompt

```
You are a QA engineer reviewing Phase 5 of the Taverna project.

Read AGENTS.md, SPEC.md, and PHASE_5.md.

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
- views/errors/404.html, views/errors/403.html, views/errors/500.html all exist.
- Custom error handler renders these for the right status codes.

DEPLOY.md EXISTS
- Contains: PostgreSQL extensions setup, environment variables, build command,
  migration command, Caddy example, admin setup, AGPL-3.0 license note.

PRODUCTION CONFIG
- config/production.yaml uses ${BASE_URL} and ${TAVERNA_DOMAIN} (env var interpolation).
- Not hardcoded URLs.

FINAL CHECKS
Run: cargo fmt -- check
Run: cargo clippy -- -D warnings
Run: cargo test

All must produce zero errors and zero warnings.

Verify the Definition of Done checklist at the bottom of PHASE_5.md.

Report CRITICAL / MEDIUM / LOW.
```

---

## Targeted Follow-Up Prompts

Use these for specific concerns at any point:

**Security audit:**
```
Read SPEC.md Section 7. Audit the entire src/ directory for:
1. Any .unwrap() in non-test production code
2. Any SQL string interpolation (not parameterized)
3. Any user content rendered without ammonia sanitization
4. Any | safe Tera filter on user-supplied content (not pre-sanitized markdown)
5. Any federation HTTP fetch not going through the activitypub_federation client

Report findings as CRITICAL / MEDIUM / LOW.
```

**Completeness check:**
```
Search the entire src/ directory for: todo!(), unimplemented!(), panic!(),
and functions whose body is just Ok(()) or a stub return value.
List every occurrence with file and line number. These are all CRITICAL.
```

**Theme consistency:**
```
Review every .html file in views/.
Any element with a hardcoded colour (hex, rgb, named colour other than inherit/transparent)
that is not inside a CSS var() call: report as MEDIUM.
```


---

## Phase 6 QA Prompt

```
You are a QA engineer reviewing Phase 6 of the Taverna project.

Read AGENTS.md, SPEC.md Sections 3.5–3.7 and 4.11–4.13, and PHASE_6.md.

Verify:

FIRST-RUN SETUP
- Open src/initializers/first_run.rs. is_first_run() queries users table
  for local users and returns true when count is 0.
- FirstRunGuard middleware must NOT redirect /setup*, /static/*, or /verify-email*.
  If it does: CRITICAL (setup page can't load its own CSS or JS).
- POST /setup must call is_first_run() — if false, return 403.
  If it trusts only a session variable or GET-time check: CRITICAL.
- The admin user created by setup must have is_admin=true. If not: CRITICAL.
- After setup, navigating to /setup must return 403, not the wizard form.

EMAIL VERIFICATION
- Tokens must be cryptographically random (uuid v4 or equivalent).
  If tokens are sequential integers or predictable: CRITICAL.
- Tokens expire after 24 hours. If no expiry check: CRITICAL.
- Expired tokens must return a clear error, not a 500.
- Verifying the same token twice must be a no-op (idempotent).
- Verification emails must NOT be sent in tests (check for mock mailer config).

STANDING SCORE
- open src/models/users.rs or votes.rs. update_standing must use an atomic
  UPDATE with GREATEST(-100, standing_score + $delta).
  If it reads then writes (two queries): MEDIUM (race condition under load).
- VoteModel::cast_vote must call update_standing for the content author,
  not the voter. If it updates the voter: CRITICAL.
- VoteAction::Removed case must reverse the delta (not just skip the update).
- soft_remove on posts and comments must deduct standing from the author.

ELIGIBILITY CHECKER
- All enabled gates must ALL pass — one failure must block creation.
- Reasons list must be accurate: only list the gates that actually failed.
- GET /c/new must show reasons when user is ineligible (not a silent 403).
- POST /c/ must return 403 with reasons when user is ineligible.

ADMIN SETTINGS
- instance_settings table has exactly one row (enforced by CHECK (id = true)).
- POST /admin/settings must require is_admin=true. If a regular user can POST:
  CRITICAL.
- Settings take effect immediately on next request (no caching beyond the
  request lifetime).

USER-FACING ELIGIBILITY
- /settings page shows eligibility status for the logged-in user.
- Reasons shown match what check_community_creation_eligibility() returns.

Run: cargo test
Report any failing tests as CRITICAL.

Check these specific tests exist (MEDIUM if missing):
- test_setup_wizard_redirects_when_not_first_run
- test_setup_disabled_after_first_user_created
- test_account_age_gate
- test_standing_score_gate
- test_removed_content_disqualifies
- test_upvote_increases_standing
- test_vote_removal_reverses_delta
- test_expired_token_rejected

Report CRITICAL / MEDIUM / LOW.
```

---

## Final Acceptance Test (run once, after all five phases are complete)

This is a holistic end-to-end review of the entire project against the full
specification. It is different from the per-phase QA prompts:

- Per-phase prompts check individual deliverables as they are built
- CodeRabbit checks each PR diff for inline issues and best practices
- **This prompt checks the finished product as a whole** — cross-cutting
  concerns, spec compliance end-to-end, and a binary go/no-go verdict

Run this once, after Phase 5 is complete and all phase QA prompts have passed.

---

### How to run it

Start gemini-cli from the repo root with the most capable model available:

```bash
cd taverna-social
gemini --model gemini-2.5-pro
```

Then paste the prompt below in full.

---

### The Prompt

```
You are a senior software engineer performing a final acceptance test of Taverna,
a federated community platform built in Rust. Your job is to determine whether
the project is ready to deploy and federate with real Lemmy and Mastodon instances.

This is not a per-file review. You are assessing the whole system.

Read these files completely before starting:
1. SPEC.md — the authoritative specification
2. AGENTS.md — the coding standards
3. PHASE_5.md — the Definition of Done

Then work through the following ten areas in order. For each area, read the
relevant source files, run the relevant commands, and report findings.
At the end, give a single verdict: PASS, PASS WITH CONDITIONS, or FAIL.

────────────────────────────────────────────────────────────────────────────────
AREA 1 — Build integrity
────────────────────────────────────────────────────────────────────────────────

Run each of these and report the result:

  cargo build --release --locked
  cargo test --all-features
  cargo clippy -- -D warnings
  cargo fmt --check

All four must succeed with zero errors and zero warnings.
If any fail: FAIL — do not continue to other areas.

────────────────────────────────────────────────────────────────────────────────
AREA 2 — Specification completeness
────────────────────────────────────────────────────────────────────────────────

Cross-reference SPEC.md against the implementation. Check:

Routes (SPEC.md Section 5.1 and 5.2):
- Every web route listed in SPEC.md is registered in src/app.rs or router setup
- Every federation route is registered and covered by FederationMiddleware
- POST /theme exists and sets taverna_theme cookie

Data model (SPEC.md Section 4):
- Every table in Section 4 exists in migration/src/
- last_fetched_at is present on users and communities tables
- All indexes listed in each table definition are present in migrations
- outbound_activities uses TEXT[] for target_inboxes

Functional requirements (SPEC.md Section 3):
- Communities: create, join, leave, post, moderate
- Posts: text type, link type, vote +1/-1, edit, delete
- Comments: threaded via ltree, vote, edit, delete
- Profiles: registration, login, follow/unfollow, profile posts
- Theme: light, dark, sepia — POST /theme — data-theme on <html>

Report any route, table, column, or feature that is in SPEC.md but missing
from the implementation as CRITICAL.

────────────────────────────────────────────────────────────────────────────────
AREA 3 — ActivityPub federation correctness
────────────────────────────────────────────────────────────────────────────────

This is the most critical area — federation bugs are invisible until a real
Lemmy or Mastodon instance tries to connect.

Check the following:

3a. WebFinger (SPEC.md Section 6.2)
- Handler uses extract_webfinger_name() from activitypub_federation crate
- Handler uses build_webfinger_response() from activitypub_federation crate
- Returns correct AP actor URL for both users (/u/:username) and communities (/c/:slug)
- Returns 404 for unknown resources (not a 500)

3b. Actor JSON-LD (SPEC.md Sections 6.2 and 6.3)
- Person actor has: id, type, preferredUsername, inbox, outbox, followers,
  following, publicKey (with id, owner, publicKeyPem), url, manuallyApprovesFollowers
- Group actor has: id, type, preferredUsername, inbox, outbox, followers,
  publicKey, attributedTo, postingRestrictedToMods, sensitive
- Both actors include the full @context array (activitystreams + security/v1)

3c. Content negotiation (SPEC.md Section 6)
- /u/:username and /c/:slug return AP JSON when Accept contains
  application/activity+json (uses FEDERATION_CONTENT_TYPE constant, not hardcoded)
- Same URLs return HTML for browser Accept headers

3d. HTTP signatures
- Inbox handlers use receive_activity() from activitypub_federation crate
  (NOT manual signature verification)
- Outbound activities use SendActivityTask::send() (NOT raw reqwest)
- No use of rsa:: or sha2:: crates in src/federation/ or src/controllers/federation/

3e. The Announce pattern (SPEC.md Section 6.4 — critical for Lemmy)
- Find the ActivityHandler::receive implementation for Create activity
- When the inbox owner is a Community, after saving the post locally,
  the code MUST build an Announce activity and call SendActivityTask::send
  with the community as the signing actor and all follower inboxes as targets
- If this is missing: CRITICAL — Lemmy federation is broken

3f. Idempotency
- Every ActivityHandler::receive checks if the object's ap_id already exists
  in the database before inserting
- No receive() implementation inserts without checking first

3g. Object::verify
- Every Object trait implementation calls verify_domains_match() in its
  verify() method
- Open each file in src/federation/types/ and confirm this

3h. Follow/Accept cycle
- When Follow activity is received: follow row inserted, Accept sent back
  via SendActivityTask::send, follow marked accepted=true
- When UndoFollow received: follow row deleted

3i. NodeInfo (SPEC.md Section 6.6)
- /nodeinfo/2.0 returns software.name = "taverna"
- /nodeinfo/2.0 returns protocols = ["activitypub"]
- /.well-known/nodeinfo links to /nodeinfo/2.0

────────────────────────────────────────────────────────────────────────────────
AREA 4 — Theme system
────────────────────────────────────────────────────────────────────────────────

4a. CSS file
- assets/static/theme.css exists
- Contains [data-theme="light"], [data-theme="dark"], [data-theme="sepia"] blocks
- Each block defines at minimum: --bg-primary, --bg-secondary, --bg-card,
  --text-primary, --text-secondary, --text-muted, --border, --accent,
  --upvote, --downvote, --link, --nav-bg, --nav-text, --score-positive,
  --score-negative

4b. Template integration
- views/layouts/base.html has <html ... data-theme="{{ theme }}">
- base.html links theme.css: <link rel="stylesheet" href="/static/theme.css">
- base.html nav contains the theme switcher form (POST /theme, three buttons)
- No JavaScript is involved in theme switching

4c. Cookie
- POST /theme handler sets cookie: name=taverna_theme, Max-Age=31536000,
  SameSite=Lax, NOT HttpOnly
- Invalid values default to "light"
- Redirects to Referer or / if no Referer

4d. No hardcoded colours
- Search views/ for hex values (#[0-9a-fA-F]{3,6}) and rgb() not in theme.css
- Any found in .html files: report as MEDIUM

────────────────────────────────────────────────────────────────────────────────
AREA 5 — Security (SPEC.md Section 7)
────────────────────────────────────────────────────────────────────────────────

5a. Passwords: argon2id via argon2 crate — not bcrypt, not sha256, not plaintext

5b. JWT: access token 1h, refresh 7d, both in httpOnly cookies
- Login does not distinguish between wrong email and wrong password in error message

5c. Banned user: AuthUser middleware checks is_banned after loading user from JWT

5d. Markdown: all user content goes through pulldown-cmark then ammonia::clean()
- Search for | safe in templates — only pre-sanitized markdown variables allowed

5e. SQL: no raw string interpolation into queries — SeaORM parameterized only

5f. SSRF: no raw reqwest calls for federation — activitypub_federation client only

5g. Rate limiting: tower-governor applied to /*/inbox routes and POST /login,
    POST /register

5h. Authorization:
- POST /c/:slug/settings: requires moderator or admin
- POST /c/:slug/posts/:id/remove: requires moderator or admin
- GET /admin, POST /admin/*: requires is_admin = true
- POST /c/:slug/posts/:id/delete: requires author OR moderator OR admin

────────────────────────────────────────────────────────────────────────────────
AREA 6 — Code quality
────────────────────────────────────────────────────────────────────────────────

6a. No unwrap() in production paths (outside #[cfg(test)] blocks)
- Search: grep -rn "\.unwrap()" src/ | grep -v "#\[cfg(test)\]\|mod tests\|// unwrap-ok"

6b. No todo!/unimplemented!/panic! in production code
- Search: grep -rn "todo!\|unimplemented!\|panic!(" src/ | grep -v "test\|// panic-ok"

6c. No dbg!() macros anywhere in src/
- Search: grep -rn "dbg!(" src/

6d. All public functions in models/ and controllers/ have /// doc comments

6e. All database migrations have both up() and down() methods

6f. Cargo.lock is committed

────────────────────────────────────────────────────────────────────────────────
AREA 7 — Cross-cutting integration
────────────────────────────────────────────────────────────────────────────────

These are issues that only appear when all phases are complete together:

7a. Theme threads through all pages
- Every handler that renders a full HTML page extracts CurrentTheme and passes
  it to render_with_theme (or equivalent)
- HTMX fragment endpoints do NOT inject theme (they return partial HTML only)

7b. Federation hooks are in place for all local actions
Check that after each local action, the corresponding AP activity is enqueued:
  - Create post in community → Create{Note} + community Announce{Create}
  - Create profile post → Create{Note} to user followers
  - Create comment → Create{Note}
  - Upvote → Like{Note}; downvote → Dislike{Note}; remove vote → Undo
  - Delete post → Delete{Note}
  - Edit post → Update{Note}
  - Follow user → Follow{Person}; follow community → Follow{Group}
  - Unfollow → Undo{Follow}
Any missing hook: CRITICAL if it's Create/Delete/Follow; MEDIUM otherwise

7c. score recalculation
- VoteModel::cast_vote always calls recalculate_score after every vote change
  (post and comment)

7d. ltree paths
- CommentModel::create builds path correctly:
  top-level: c{post_8}.c{comment_8}
  reply: {parent.path}.c{comment_8}
  Each segment starts with "c" (ltree label requirement)

7e. Outbound activities gated on federation config
- SendActivityTask::send is only called when config.taverna.federation.enabled == true

────────────────────────────────────────────────────────────────────────────────
AREA 8 — Moderation completeness
────────────────────────────────────────────────────────────────────────────────

8a. community_bans table exists (m007_bans migration) and is registered

8b. Locked post: comment form hidden in template when is_locked=true;
    create_comment handler returns 403 for locked posts

8c. Banned community member: community_bans checked before allowing post/comment

8d. Instance-banned user: is_banned checked in AuthUser middleware

8e. Admin dashboard exists at GET /admin

8f. Error pages exist: views/errors/404.html, 403.html, 500.html

────────────────────────────────────────────────────────────────────────────────
AREA 9 — Deployment readiness
────────────────────────────────────────────────────────────────────────────────

9a. DEPLOY.md exists and covers:
- PostgreSQL 16 setup with uuid-ossp, ltree, pgmq extensions
- Environment variables: DATABASE_URL, BASE_URL, TAVERNA_DOMAIN, JWT_SECRET
- Build command: cargo build --release
- Migration command: ./target/release/taverna-cli db migrate
- AGPL-3.0 license note (required by activitypub_federation dependency)

9b. config/production.yaml uses ${BASE_URL} and ${TAVERNA_DOMAIN}
    (environment variable interpolation, not hardcoded values)

9c. cargo deny check licenses passes (run it and report result)

9d. cargo audit reports no known vulnerabilities (run it and report result)

────────────────────────────────────────────────────────────────────────────────
AREA 10 — Definition of Done (PHASE_5.md)
────────────────────────────────────────────────────────────────────────────────

Read the Definition of Done checklist from PHASE_5.md.
Verify each item explicitly. Report any that are not satisfied.

────────────────────────────────────────────────────────────────────────────────
VERDICT
────────────────────────────────────────────────────────────────────────────────

After completing all ten areas, give one of three verdicts:

PASS
  All CRITICAL checks passed. Zero or only LOW issues remain.
  The project is ready to deploy and federate.

PASS WITH CONDITIONS
  No CRITICAL issues. One or more MEDIUM issues remain.
  List the conditions that must be fixed before going live.
  The project can be deployed to a staging environment for testing.

FAIL
  One or more CRITICAL issues found.
  List every CRITICAL issue with file and line number where possible.
  The project must not be deployed until these are resolved.
  Take the CRITICAL issues to opencode using /project:fix-issue.

────────────────────────────────────────────────────────────────────────────────

Begin with Area 1. If cargo build --release fails, stop and report FAIL immediately.
Otherwise work through all ten areas, then give the verdict.
```

---

## Targeted Codebase Audit (run at any point to scan for issues)

Use this when you have made changes and want a general health check, or when
you notice something feels wrong but can't pinpoint it. Unlike the per-phase
QA prompts (which check specific deliverables) and the Final Acceptance Test
(which assumes all phases are done), this prompt scans the whole codebase for
common issues regardless of what phase you are on.

### How to run it

```bash
cd taverna-social
gemini --model gemini-2.5-pro
```

Paste the prompt below.

---

### The Prompt

```
You are performing a targeted audit of the Taverna codebase. This is a
general health scan — not tied to any specific phase or feature. Your job
is to find real problems in the existing code, not to verify planned features.

Read SPEC.md and AGENTS.md first. Then work through the following areas.
Report every finding with: file path, line number if possible, severity
(CRITICAL / MEDIUM / LOW), and a concrete description of the problem.

Do not report things that are correct. Only report actual issues.

────────────────────────────────────────────────────────────────────────────────
SCAN 1 — Compilation and lint baseline
────────────────────────────────────────────────────────────────────────────────

Run these and report any output:

  cargo check 2>&1
  cargo clippy -- -D warnings 2>&1
  cargo fmt --check 2>&1

If cargo check fails: stop and report all errors as CRITICAL.
Clippy warnings: report each as MEDIUM.
Format issues: report as LOW (auto-fixable with cargo fmt).

────────────────────────────────────────────────────────────────────────────────
SCAN 2 — Stub and placeholder code
────────────────────────────────────────────────────────────────────────────────

Run these searches and report every match:

  grep -rn "todo!\|unimplemented!\|panic!(" src/ --include="*.rs" \
    | grep -v "#\[cfg(test)\]\|mod tests\|// panic-ok"

  grep -rn "\.unwrap()" src/ --include="*.rs" \
    | grep -v "#\[cfg(test)\]\|mod tests\|// unwrap-ok"

  grep -rn "dbg!(" src/ --include="*.rs"

  # Functions whose entire body is a stub return
  grep -rn "Ok(())\|todo!()\|unimplemented!()" src/ --include="*.rs"

For every unwrap() found outside tests: report as CRITICAL.
For every todo!/unimplemented!: report as CRITICAL.
For every dbg!(): report as MEDIUM.

────────────────────────────────────────────────────────────────────────────────
SCAN 3 — Security patterns
────────────────────────────────────────────────────────────────────────────────

3a. SQL injection risk
    Search for format!() calls that contain SQL keywords:
      grep -rn "format!.*SELECT\|format!.*INSERT\|format!.*UPDATE\|format!.*DELETE" \
        src/ --include="*.rs"
    Any match: CRITICAL.

3b. Hardcoded secrets
    grep -rn -i "password\s*=\s*\"[^{]\|secret\s*=\s*\"[^{]\|api_key\s*=\s*\"[^{]" \
      src/ config/ --include="*.rs" --include="*.yaml" \
      | grep -v "test\|example\|placeholder\|your-secret\|changeme"
    Any match: CRITICAL.

3c. Markdown not sanitized
    Find all places markdown is rendered. Verify each calls ammonia::clean()
    after pulldown_cmark parsing. If any render path skips ammonia: CRITICAL.

3d. Tera | safe on user content
    grep -rn "| safe" views/ --include="*.html" \
      | grep -v "rendered_\|markdown_\|body_html\|content_html\|safe_html"
    Any match that is not pre-sanitized server-side HTML: CRITICAL.

3e. JWT stored outside httpOnly cookie
    Search for "localStorage\|sessionStorage\|Authorization.*Bearer" in
    src/ and views/. If JWT is stored client-side or sent as a header
    from JavaScript: CRITICAL.

3f. Password hashing algorithm
    grep -rn "bcrypt\|md5\|sha256.*password\|sha1.*password" src/ --include="*.rs"
    Any match: CRITICAL. Must use argon2id.

────────────────────────────────────────────────────────────────────────────────
SCAN 4 — Federation correctness
────────────────────────────────────────────────────────────────────────────────

4a. Raw reqwest in federation code
    grep -rn "reqwest::" src/federation/ src/controllers/federation/ \
      --include="*.rs" 2>/dev/null
    Any match: CRITICAL. Must use SendActivityTask::send() or ObjectId::dereference().

4b. Manual signature code
    grep -rn "use rsa::\|use sha2::\|HmacSha\|RsaPrivateKey" \
      src/federation/ src/controllers/federation/ --include="*.rs" 2>/dev/null
    Any match: CRITICAL. activitypub_federation handles signing.

4c. verify_domains_match missing
    Open each file in src/federation/types/ that implements the Object trait.
    Each verify() method must call verify_domains_match().
    Missing calls: CRITICAL.

4d. Idempotency gaps
    For each ActivityHandler::receive implementation, verify there is a
    database lookup by ap_id before any INSERT. If any receive() inserts
    without checking: CRITICAL.

4e. Announce pattern present
    Find Create activity's ActivityHandler::receive. When inbox owner is a
    Community, after saving the post, must call SendActivityTask::send with
    an Announce activity signed by the community actor.
    Missing: CRITICAL.

────────────────────────────────────────────────────────────────────────────────
SCAN 5 — Data integrity
────────────────────────────────────────────────────────────────────────────────

5a. Score recalculation
    Every vote operation (cast_vote in votes.rs) must call recalculate_score.
    Read votes.rs and confirm recalculate_score is called for both post and
    comment votes. Missing: CRITICAL.

5b. ltree path correctness
    Open src/models/comments.rs, create() method.
    - Top-level path: format!("{}.{}", post_seg, comment_seg)
    - Reply path: format!("{}.{}", parent.path, comment_seg)
    - Each segment must start with a letter (prefix "c" required for ltree)
    Wrong format: CRITICAL (threaded comments will break silently).

5c. post_has_content constraint respected
    Find post creation in src/models/posts.rs.
    At least one of body or url must be non-null before inserting.
    If validation is missing: MEDIUM (DB constraint will catch it, but with
    a poor error message).

5d. Denormalized counts
    Find comment creation. comment_count on the parent post must be
    incremented. Find comment deletion. comment_count must be decremented.
    Missing: MEDIUM.

────────────────────────────────────────────────────────────────────────────────
SCAN 6 — Theme consistency
────────────────────────────────────────────────────────────────────────────────

6a. Hardcoded colours in templates
    grep -rn "#[0-9a-fA-F]\{3,6\}\|rgb(\|rgba(" views/ --include="*.html"
    Any found outside theme.css: MEDIUM.

6b. Theme not injected in a handler
    List all handlers that render a full HTML page (not HTMX fragments).
    Each must extract CurrentTheme and pass it to render_with_theme.
    grep -rn "tera.render\b" src/ --include="*.rs" \
      | grep -v "render_with_theme\|fragment\|htmx"
    Any plain tera.render() call for a full page: MEDIUM (page ignores theme).

6c. HTMX endpoints leaking full pages
    Vote endpoints and reply-form endpoints must return HTML fragments only.
    Check that handlers for /vote and /reply-form check HX-Request header
    and return partial HTML. If they return full pages: MEDIUM.

────────────────────────────────────────────────────────────────────────────────
SCAN 7 — Dead code and unused imports
────────────────────────────────────────────────────────────────────────────────

    cargo check 2>&1 | grep "warning.*unused\|warning.*dead_code\|warning.*never used"

Report each as LOW. These are not urgent but indicate incomplete wiring.

────────────────────────────────────────────────────────────────────────────────
SCAN 8 — Test coverage gaps
────────────────────────────────────────────────────────────────────────────────

Run: cargo test 2>&1
Report any failing tests as CRITICAL.

Then check for these specific tests — if any are missing, report as MEDIUM:
- A test that verifies the ltree path format for top-level comments
- A test that verifies the ltree path format for nested replies
- A test that verifies duplicate AP activity is handled idempotently
- A test that verifies POST /theme sets the correct cookie
- A test that verifies score recalculates after a vote

────────────────────────────────────────────────────────────────────────────────
REPORT FORMAT
────────────────────────────────────────────────────────────────────────────────

Group findings by severity:

CRITICAL — must fix before this code runs in production
  [file:line] Description of the problem and why it matters.

MEDIUM — should fix; causes bugs or degrades correctness
  [file:line] Description.

LOW — clean up when convenient
  [file:line] Description.

End with a one-line summary:
  "X critical, Y medium, Z low issues found."

If zero issues: "No issues found. Codebase is clean."

Take any CRITICAL findings to opencode with /project:fix-issue.
```
