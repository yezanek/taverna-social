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

### Step 2 — Start gemini-cli in the taverna/ directory

```bash
cd taverna
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
