# Reviewing Taverna with an AI agent

This file holds the review prompts that apply to the whole codebase rather
than to one phase. Each phase file (`PHASE_*.md`) has its own implementation
and review prompts at the end. See [INSTRUCTIONS.md](INSTRUCTIONS.md) for the
overall workflow.

The prompts work with any AI coding agent that can read files and run shell
commands in the repository. Run the reviewer in a fresh session, separate from
the one that wrote the code, so it judges the code rather than its own plan.

## Severity scale

Every prompt asks for findings graded the same way:

- **CRITICAL**: a blocker. Wrong behaviour, a security hole, broken federation,
  or a failing gate. Fix before merging.
- **MEDIUM**: should be fixed; a bug or a spec gap that is not yet harmful.
- **LOW**: style or tidiness; fix when convenient.

Hand the findings to the implementing agent with the "Fix a reported issue"
prompt in [INSTRUCTIONS.md](INSTRUCTIONS.md), then run the gate again.

Automated PR review (CodeRabbit) also runs on every pull request; these prompts
complement it with checks against the spec.

---

## Targeted follow-up prompts

Use these for specific concerns at any point:

**Security audit:**
```
Read docs/ai/SPEC.md Section 7. Audit the entire src/ directory for:
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
Review every .html file in assets/views/.
Any element with a hardcoded colour (hex, rgb, named colour other than inherit/transparent)
that is not inside a CSS var() call: report as MEDIUM.
```

---

## Final acceptance test

This is a holistic end-to-end review of the entire project against the full
specification. It is different from the per-phase QA prompts:

- Per-phase prompts check individual deliverables as they are built
- CodeRabbit checks each PR diff for inline issues and best practices
- **This prompt checks the finished product as a whole** — cross-cutting
  concerns, spec compliance end-to-end, and a binary go/no-go verdict

Run it before a release, or after a large change, once the per-phase review prompts pass.

---

### How to run it

Start a fresh session of your reviewing agent in the repo root, using the most
capable model you have, and paste the prompt below in full.

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
  cargo clippy --all-targets --all-features -- -D warnings
  cargo fmt --all -- --check

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
- assets/views/layouts/base.html has <html ... data-theme="{{ theme }}">
- base.html links theme.css: <link rel="stylesheet" href="/static/theme.css">
- base.html nav contains the theme switcher form (POST /theme, three buttons)
- No JavaScript is involved in theme switching

4c. Cookie
- POST /theme handler sets cookie: name=taverna_theme, Max-Age=31536000,
  SameSite=Lax, NOT HttpOnly
- Invalid values default to "light"
- Redirects to Referer or / if no Referer

4d. No hardcoded colours
- Search assets/views/ for hex values (#[0-9a-fA-F]{3,6}) and rgb() not in theme.css
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

8f. Error pages exist: assets/views/errors/404.html, 403.html, 500.html

────────────────────────────────────────────────────────────────────────────────
AREA 9 — Deployment readiness
────────────────────────────────────────────────────────────────────────────────

9a. DEPLOY.md exists and covers:
- PostgreSQL 16 setup with uuid-ossp, ltree, pgmq extensions
- Environment variables: DATABASE_URL, BASE_URL, TAVERNA_DOMAIN, JWT_SECRET
- Build command: cargo build --release
- Migration command: ./target/release/taverna-cli db migrate
- AGPL-3.0 license note (required by activitypub_federation dependency)

9b. config/production.yaml uses <%= get_env(name="BASE_URL") %> and <%= get_env(name="TAVERNA_DOMAIN") %>
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
  Hand the CRITICAL findings to the implementing agent with the "Fix a reported issue" prompt in docs/ai/INSTRUCTIONS.md.

────────────────────────────────────────────────────────────────────────────────

Begin with Area 1. If cargo build --release fails, stop and report FAIL immediately.
Otherwise work through all ten areas, then give the verdict.
```

---

## Targeted codebase audit

Run this at any point to scan for issues.

Use this when you have made changes and want a general health check, or when
you notice something feels wrong but can't pinpoint it. Unlike the per-phase
QA prompts (which check specific deliverables) and the Final Acceptance Test
(which assumes all phases are done), this prompt scans the whole codebase for
common issues regardless of what phase you are on.

### How to run it

Start a fresh session of your reviewing agent in the repo root and paste the
prompt below.

### The Prompt

```
You are performing a targeted audit of the Taverna codebase. This is a
general health scan — not tied to any specific phase or feature. Your job
is to find real problems in the existing code, not to verify planned features.

Read docs/ai/SPEC.md and AGENTS.md first. Then work through the following areas.
Report every finding with: file path, line number if possible, severity
(CRITICAL / MEDIUM / LOW), and a concrete description of the problem.

Do not report things that are correct. Only report actual issues.

────────────────────────────────────────────────────────────────────────────────
SCAN 1 — Compilation and lint baseline
────────────────────────────────────────────────────────────────────────────────

Run these and report any output:

  cargo check 2>&1
  cargo clippy --all-targets --all-features -- -D warnings 2>&1
  cargo fmt --all -- --check 2>&1

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
    grep -rn "| safe" assets/views/ --include="*.html" \
      | grep -v "rendered_\|markdown_\|body_html\|content_html\|safe_html"
    Any match that is not pre-sanitized server-side HTML: CRITICAL.

3e. JWT stored outside httpOnly cookie
    Search for "localStorage\|sessionStorage\|Authorization.*Bearer" in
    src/ and assets/views/. If JWT is stored client-side or sent as a header
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
    grep -rn "#[0-9a-fA-F]\{3,6\}\|rgb(\|rgba(" assets/views/ --include="*.html"
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

Hand the CRITICAL findings to the implementing agent with the "Fix a reported issue" prompt in docs/ai/INSTRUCTIONS.md.
```
