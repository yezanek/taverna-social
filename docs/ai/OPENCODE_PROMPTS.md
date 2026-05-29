# How to Use opencode to Implement Taverna

This file tells you exactly what to say to opencode to implement each phase.
For the QA workflow using gemini-cli, see GEMINI_PROMPTS.md.

---

## Role Division

| Tool | Role |
|---|---|
| **opencode** | **Implements each phase — writes Rust code, templates, migrations** |
| gemini-cli | QA — reviews output, runs tests, catches issues, requests fixes |

---

## Before You Start

```bash
# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Loco and SeaORM CLIs
cargo install loco-cli sea-orm-cli

# PostgreSQL 16 and pgmq — see INSTRUCTIONS.md Prerequisites section
# Databases must exist before Phase 1:
createdb taverna_development && createdb taverna_test
```

---

## Key Dependency Note

`activitypub_federation = "0.6"` (AGPL-3.0) handles all HTTP signatures, actor
fetching, activity sending, and WebFinger. Do NOT implement these manually.
Taverna must be AGPL-3.0 as a result.

---

## Installing and Configuring opencode

```bash
curl -fsSL https://opencode.ai/install | bash
```

Configure your AI provider. opencode works with Anthropic, OpenAI, Google, and others.
Recommended for implementation work: Claude Sonnet or Opus (strong at Rust).

```bash
# Edit ~/.config/opencode/config.json
{
  "provider": "anthropic",
  "model": "claude-sonnet-4-5",
  "apiKey": "YOUR_ANTHROPIC_API_KEY"
}
```

Or to use Gemini for both tools:
```json
{
  "provider": "google",
  "model": "gemini-2.5-pro",
  "apiKey": "YOUR_GOOGLE_API_KEY"
}
```

Start opencode in your project directory:
```bash
opencode       # interactive mode
# or for a single prompt:
opencode -p "your prompt here"
```

---

## Plan Mode and Build Mode

opencode has two modes. **Always use Plan mode first for significant tasks.**

In Plan mode opencode shows you what it intends to do before touching any files.
Review the plan, then type `yes` or `build` to execute.

For small fixes (a single line, a typo) you can skip Plan mode.
For anything that creates or significantly modifies files, use Plan mode.

---

## Phase 1 — Scaffold & Database

Run from the **repo root** (`taverna-social/`), with docs/ai/ files visible:

```
Read AGENTS.md, docs/ai/SPEC.md, and docs/ai/PHASE_1.md from the docs/ai/ directory.

Follow every instruction in PHASE_1.md exactly. Do not skip any task.

Before writing any code, show me a plan of every file you will create or modify,
then wait for my approval.

Important: include last_fetched_at TIMESTAMPTZ on the users and communities tables —
the activitypub_federation crate requires this field for the Object trait's
last_refreshed_at() method.

Verify the docs/ai/PHASE_1.md deliverables checklist when done.

Note: `loco new` will try to create a subdirectory. After scaffolding, move all
generated files up to the repo root (Cargo.toml, src/, config/, migration/, assets/)
and delete the empty subdirectory before committing.
```

Gate before Phase 2:
```bash
cargo check
cargo loco db migrate
cargo test
```

---

## Phase 2 — Authentication & User Model

```
Read AGENTS.md, docs/ai/SPEC.md, and docs/ai/PHASE_2.md.

Phase 1 is complete. The database has been migrated and SeaORM entities
are in src/models/_entities/.

Show me a plan before writing any code.

Implement everything in PHASE_2.md.

Critical: use generate_actor_keypair() from the activitypub_federation crate for
keypair generation — do NOT use the rsa crate directly. Set up FederationConfig
and FederationMiddleware in app.rs as in Task 2.6.

Verify the Phase 2 deliverables checklist when done.
```

Gate:
```bash
cargo check && cargo test
```

---

## Phase 3 — Communities, Posts & Theme Switcher

```
Read AGENTS.md, docs/ai/SPEC.md, and docs/ai/PHASE_3.md.

Phases 1 and 2 are complete. Auth works.

Show me a plan before writing any code.

Implement everything in PHASE_3.md. Key things:

Theme switcher (Tasks 3.5–3.9):
- Theme middleware reads taverna_theme cookie, injects CurrentTheme extension
- POST /theme sets cookie: Max-Age=31536000, SameSite=Lax, NOT httpOnly
- assets/static/theme.css must define all three [data-theme] blocks with the
  exact CSS custom properties from SPEC.md Section 3.5
- base.html must have data-theme="{{ theme }}" on the <html> element
- All templates use var(--bg-primary), var(--text-primary) etc — no hardcoded colours
- HTMX vote endpoints return fragments only — do not inject theme into fragments

Other:
- ltree path logic for comments (Task 3.3) — get this exactly right
- Always call recalculate_score after any vote change

Verify the Phase 3 deliverables checklist when done.
```

Gate:
```bash
cargo check && cargo test
# Manual:
cargo loco start &
curl -X POST http://localhost:3000/theme -d "theme=dark" -v
# Must see Set-Cookie: taverna_theme=dark in response
```

---

## Phase 4 — ActivityPub Federation

```
Read AGENTS.md, docs/ai/SPEC.md, and docs/ai/PHASE_4.md.

Phases 1–3 are complete. Local features and the theme switcher work.

Show me a plan before writing any code.

Implement ActivityPub federation using the activitypub_federation crate (version 0.6,
axum feature). The crate handles HTTP signatures, actor fetching, and activity sending.

Do NOT implement these manually:
- Use receive_activity() in inbox handlers (signatures auto-verified)
- Use SendActivityTask::send() for all outbound activities
- Use build_webfinger_response() and extract_webfinger_name() for WebFinger
- Use FederationJson and FEDERATION_CONTENT_TYPE for content negotiation
- Use ObjectId::dereference() for actor resolution

Critical:
1. Announce pattern (Task 4.4) is mandatory for Lemmy compatibility
2. All ActivityHandler::receive implementations must be idempotent (check ap_id)
3. Call verify_domains_match() in every Object::verify implementation
4. Apply FederationMiddleware to all AP route groups

Implement Task 4.1 (AP struct types) first and show me the result before continuing.

Verify the Phase 4 deliverables checklist when done.
```

Gate:
```bash
cargo check && cargo test
cargo loco start &
curl "http://localhost:3000/.well-known/webfinger?resource=acct:admin@localhost"
curl -H "Accept: application/activity+json" "http://localhost:3000/u/admin"
curl "http://localhost:3000/nodeinfo/2.0"
```

---

## Phase 5 — Profiles, Moderation & Polish

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
  cargo fmt && cargo clippy -- -D warnings && cargo test

All must pass with zero errors and zero warnings.
Verify the Definition of Done at the bottom of docs/ai/PHASE_5.md.
```

---

## Recovery Prompts

**If opencode skips Plan mode and starts writing files immediately:**
```
Stop. Before making any more changes, show me a complete plan:
- Every file you will create
- Every file you will modify and what will change
Wait for my approval before proceeding.
```

**If it hand-rolls HTTP signatures:**
```
Do not implement HTTP signatures manually. The activitypub_federation crate handles this.
- Outbound: use SendActivityTask::send() — signs automatically
- Inbound: use receive_activity() — verifies automatically
Remove any manual signing code and replace with the crate's API.
```

**If it uses rsa/sha2/base64 directly:**
```
Remove rsa, sha2, and base64 from federation code. Use generate_actor_keypair()
for key generation. The crate manages signing internally.
```

**If the Announce pattern is missing:**
```
The Announce pattern is missing from Create::receive. When a community inbox
receives Create{Note}: (1) save the post locally, (2) community Group actor sends
Announce{Create{Note}} to ALL followers via SendActivityTask::send.
Without step 2, remote Lemmy followers never see posts. Add this now.
```

**If the theme doesn't apply:**
```
Check: (1) base.html has data-theme="{{ theme }}" on <html>,
(2) theme middleware is in the middleware stack before handlers,
(3) render_with_theme injects theme into every Tera context,
(4) theme.css is at assets/static/theme.css and linked from base.html.
```

**If it hardcodes colours:**
```
Do not hardcode colours in HTML or templates. All colours must use CSS custom
properties: var(--bg-primary), var(--text-primary), var(--accent), etc.
as defined in theme.css. Remove hardcoded hex values.
```

**If it leaves todo!() or unimplemented!():**
```
The file [filename] contains placeholder code (todo!/unimplemented!).
All code must be real, working implementations. Complete it now.
```

---

## Phase 6 — Admin Onboarding & Community Creation Eligibility

```
Read AGENTS.md, docs/ai/SPEC.md (Sections 3.5, 3.6, 3.7, and 4.11–4.13), and docs/ai/PHASE_6.md.

Phases 1–5 are complete and the application federates correctly.

Show me a plan before writing any code.

Implement everything in PHASE_6.md in task order:

Task 6.1 — First-run setup wizard
  Critical: FirstRunGuard must NOT block /static/* or /setup* paths.
  POST /setup must re-check is_first_run() — do not trust GET-time state.
  Cache the is_first_run() result for max 60 seconds, bust it after setup.

Task 6.2 — Email verification
  Use Loco's built-in mailer. Use a mock mailer in test config.
  Token must be a cryptographically random string (use uuid or rand crate),
  not a predictable value.

Task 6.3 — Standing score
  standing_score is updated atomically: UPDATE users SET standing_score =
  GREATEST(-100, standing_score + $delta) WHERE id = $id
  The floor of -100 is enforced at the DB UPDATE level, not in Rust.

Task 6.4 — Eligibility checker
  All checks are additive — a user must pass ALL enabled checks.
  Return human-readable reasons for each failed check.
  Call this from both GET /c/new (show reasons in form) and POST /c/ (return 403).

Task 6.5 — Admin eligibility settings dashboard
  All settings read from DB on each request — no application restart needed.

Tasks 6.6, 6.7, 6.8 — Admin user management, user-facing eligibility, tests.

After completing all tasks:
  cargo fmt && cargo clippy -- -D warnings && cargo test

Verify the Phase 6 deliverables checklist when done.
```
