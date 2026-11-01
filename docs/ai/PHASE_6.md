# Phase 6 — Admin Onboarding & Community Creation Eligibility

> **Status:** complete. This file is the plan this phase was built from. It is kept as a
> record of why the code looks the way it does, and as a template for planning similar work.
> Where it disagrees with the code, `SPEC.md`, `DECISIONS.md` or [DESIGN.md](../DESIGN.md), those win.
>
> **Changed since:** Instance settings are edited from `/admin/settings`. Eligibility disqualifies only currently suspended accounts.

**Goal:** First-run admin setup so a new instance has a configured administrator
from the start. A tunable eligibility system that controls which users can create
communities, based on account age, email verification, and standing score.

**Prerequisites:** Phases 1–5 complete. The application deploys and federates.

---

## Background

Two problems this phase solves:

**Problem 1 — Admin bootstrap.**
After `cargo loco start` there is no admin user. The current workaround is a
raw SQL UPDATE. This phase replaces that with a guided first-run flow.

**Problem 2 — Community creation spam.**
Open community creation invites abuse. This phase adds a configurable eligibility
gate so the instance admin can require users to meet criteria before creating a
community — without hardcoding any specific policy.

---

## New database migration: m008_eligibility.rs

```sql
-- Instance-level configuration for community creation eligibility.
-- Only one row exists (instance-wide settings).
CREATE TABLE instance_settings (
  id                          BOOL PRIMARY KEY DEFAULT true
                                CHECK (id = true),  -- enforces single row
  community_creation_open     BOOL NOT NULL DEFAULT true,
    -- true  = any registered user can create
    -- false = must pass all enabled eligibility checks

  -- Account age gate
  require_min_account_age     BOOL NOT NULL DEFAULT false,
  min_account_age_days        INT  NOT NULL DEFAULT 30,

  -- Email verification gate
  require_email_verified      BOOL NOT NULL DEFAULT false,

  -- Standing score gate (karma-like)
  require_min_standing        BOOL NOT NULL DEFAULT false,
  min_standing_score          INT  NOT NULL DEFAULT 0,
    -- standing_score = upvotes_received - downvotes_received
    -- on all posts and comments authored by the user

  -- Disqualification flags (if true, user is ineligible regardless of score)
  disqualify_if_content_removed  BOOL NOT NULL DEFAULT false,
    -- any post or comment with is_removed=true disqualifies
  disqualify_if_suspended        BOOL NOT NULL DEFAULT false,
    -- any past suspension (is_banned history) disqualifies

  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Seed default: open creation, no gates enabled
INSERT INTO instance_settings DEFAULT VALUES;

-- Track email verification tokens
CREATE TABLE email_verifications (
  id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
  user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  token       TEXT NOT NULL UNIQUE,
  expires_at  TIMESTAMPTZ NOT NULL DEFAULT NOW() + INTERVAL '24 hours',
  verified_at TIMESTAMPTZ,
  created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_email_verif_token ON email_verifications(token);
CREATE INDEX idx_email_verif_user  ON email_verifications(user_id);
```

Also add to the `users` table (new migration column):

```sql
ALTER TABLE users ADD COLUMN email_verified   BOOL NOT NULL DEFAULT false;
ALTER TABLE users ADD COLUMN standing_score   INT  NOT NULL DEFAULT 0;
ALTER TABLE users ADD COLUMN suspended_at     TIMESTAMPTZ;   -- NULL = not currently suspended
```

---

## Tasks

### Task 6.1 — First-run admin setup

**What it is:** When the application starts and there are zero local users in
the database, redirect all web traffic to a setup wizard at `/setup`. Once
setup is complete, the route is disabled permanently.

**Implementation:**

Add a startup check in `src/initializers/first_run.rs`:

```rust
pub async fn is_first_run(db: &DatabaseConnection) -> Result<bool> {
    let count = Users::find()
        .filter(users::Column::IsLocal.eq(true))
        .count(db)
        .await?;
    Ok(count == 0)
}
```

Add an Axum middleware layer `FirstRunGuard` that:
1. Calls `is_first_run()` (cached for 60 seconds — not every request)
2. If true and the request path is not `/setup*` or `/static/*`: redirect to `/setup`
3. If false: pass through

**Routes to add:**
```
GET  /setup        → setup wizard form (instance name, admin username, email, password)
POST /setup        → validate inputs, create admin user, mark setup complete, redirect /
```

**Setup form fields:**
- Instance name (stored in instance_settings as a new `instance_name TEXT` column)
- Admin username (3–30 chars, alphanumeric + underscore)
- Admin email
- Admin password + confirm
- Checkbox: "Enable open registrations" (sets a new `open_registrations BOOL` column)

**POST /setup logic:**
1. Check `is_first_run()` — if false, return 403 (setup already done)
2. Validate all fields
3. Call `UserModel::create_local()` with `is_admin = true`
4. Redirect to `/` with a flash message: "Welcome! You are logged in as admin."

**Template:** `assets/views/setup/wizard.html`
- Minimal, standalone page (no nav bar, no theme switcher — setup happens before preferences)
- Extends a plain layout, not base.html
- Clear heading: "Set up your Taverna instance"

---

### Task 6.2 — Email verification

Add email verification so the `require_email_verified` gate works.

**Flow:**
1. On registration, send a verification email with a unique token link
2. `GET /verify-email?token=...` — validates token, sets `email_verified = true`
3. Tokens expire after 24 hours

**Routes:**
```
GET  /verify-email         → verify token, redirect to / with success message
POST /resend-verification  → resend verification email (auth required)
```

**Loco mailer** (`src/mailers/verification.rs`):
```rust
pub async fn send_verification_email(
    mailer: &dyn Mailer,
    to: &str,
    username: &str,
    token: &str,
    base_url: &str,
) -> Result<()>
```

Email template (`src/mailers/templates/verification.html`):
- Subject: "Verify your Taverna account"
- Body: plain HTML with the verification link
- Link format: `{base_url}/verify-email?token={token}`

---

### Task 6.3 — Standing score maintenance

`standing_score` is a denormalized column on `users` that is recalculated
whenever the user's content is voted on or moderated.

**When to update:**

| Event | Change to standing_score |
|---|---|
| Upvote received on user's post/comment | +1 |
| Downvote received on user's post/comment | -1 |
| Vote removed from user's post/comment | reverse the original delta |
| User's post/comment removed by a mod (`is_removed=true`) | -5 (configurable) |

Add `UserModel::update_standing(db, user_id, delta: i32)` — a simple atomic
`UPDATE users SET standing_score = standing_score + $1 WHERE id = $2`.

Call this from:
- `VoteModel::cast_vote` — after recording the vote, update the content author's standing
- `PostModel::soft_remove` / `CommentModel::soft_remove` — deduct standing

---

### Task 6.4 — Eligibility checker

```rust
// src/models/eligibility.rs

pub struct EligibilityResult {
    pub eligible: bool,
    pub reasons: Vec<String>,   // human-readable reasons if not eligible
}

pub async fn check_community_creation_eligibility(
    db: &DatabaseConnection,
    user: &users::Model,
) -> Result<EligibilityResult> {
    let settings = InstanceSettings::get(db).await?;

    // Fast path: open creation
    if settings.community_creation_open {
        return Ok(EligibilityResult { eligible: true, reasons: vec![] });
    }

    let mut reasons = vec![];

    // Account age
    if settings.require_min_account_age {
        let age_days = (Utc::now() - user.created_at).num_days();
        if age_days < settings.min_account_age_days as i64 {
            reasons.push(format!(
                "Account must be at least {} days old (yours is {} days old).",
                settings.min_account_age_days, age_days
            ));
        }
    }

    // Email verification
    if settings.require_email_verified && !user.email_verified {
        reasons.push("You must verify your email address.".into());
    }

    // Standing score
    if settings.require_min_standing && user.standing_score < settings.min_standing_score {
        reasons.push(format!(
            "Minimum standing score of {} required (yours is {}).",
            settings.min_standing_score, user.standing_score
        ));
    }

    // Disqualification: content removed
    if settings.disqualify_if_content_removed {
        let removed = PostModel::has_removed_content(db, user.id).await?
            || CommentModel::has_removed_content(db, user.id).await?;
        if removed {
            reasons.push("Your account has content that was removed by a moderator.".into());
        }
    }

    // Disqualification: currently suspended
    if settings.disqualify_if_suspended && user.suspended_at.is_some() {
        reasons.push("Your account has a suspension history.".into());
    }

    Ok(EligibilityResult {
        eligible: reasons.is_empty(),
        reasons,
    })
}
```

Call `check_community_creation_eligibility` in the `GET /c/new` and
`POST /c/` (community creation) handlers. If not eligible:
- `GET /c/new` — render the new community form with the reasons listed, create button disabled
- `POST /c/` — return 403 with the reasons

---

### Task 6.5 — Admin dashboard: eligibility settings

Extend the admin dashboard at `/admin` with a section for eligibility configuration.

**Route:**
```
GET  /admin/settings          → eligibility settings form
POST /admin/settings          → save instance_settings row
```

**Form fields** (each gated by a toggle switch):

```
Instance name: [________________]

Community creation:
  ( ) Open — any registered user can create communities
  ( ) Gated — users must meet the following requirements:

    [x] Minimum account age: [30] days
    [x] Email must be verified
    [x] Minimum standing score: [0]
    [ ] Disqualify if content was removed by a moderator
    [ ] Disqualify if account is currently suspended
```

All changes take effect immediately (no restart needed — read from DB per request).

**Template:** `assets/views/admin/settings.html`

---

### Task 6.6 — Admin user management additions

Add to the existing admin dashboard:

- Column showing `standing_score` for each user
- Column showing `email_verified` status
- Column showing `suspended_at` (if not null: "Suspended on {date}")
- Button: "Suspend" (sets `suspended_at = NOW()`) / "Unsuspend" (sets `suspended_at = NULL`)
- Button: "Force verify email" (sets `email_verified = true` without token)

Suspension is separate from the existing `is_banned`. Banning is permanent removal.
Suspension is a reversible administrative action. While it lasts, it also
triggers the `disqualify_if_suspended` eligibility check; unsuspending clears
`suspended_at` and with it the disqualification.

---

### Task 6.7 — User-facing eligibility feedback

On the user's own settings page (`/settings`), add a section:
**"Community creation eligibility"**

Show either:
- ✓ "You are eligible to create communities." (green)
- ✗ "You are not yet eligible. Reasons:" + the list from EligibilityResult

This lets users understand what they need to do without having to attempt creation first.

---

### Task 6.8 — Tests

```rust
// tests/admin_setup_test.rs
test_setup_wizard_accessible_on_first_run()
test_setup_wizard_redirects_when_not_first_run()
test_setup_creates_admin_user()
test_setup_disabled_after_first_user_created()

// tests/eligibility_test.rs
test_open_creation_always_eligible()
test_account_age_gate()
test_email_verified_gate()
test_standing_score_gate()
test_removed_content_disqualifies()
test_suspension_disqualifies()
test_multiple_gates_all_must_pass()
test_reasons_list_is_accurate()

// tests/standing_test.rs
test_upvote_increases_standing()
test_downvote_decreases_standing()
test_vote_removal_reverses_delta()
test_moderation_removal_deducts_standing()

// tests/email_verification_test.rs
test_verification_token_created_on_register()
test_verify_email_sets_verified_flag()
test_expired_token_rejected()
test_already_verified_token_is_noop()
```

---

## Deliverables Checklist

- [ ] `m008_eligibility.rs` migration (instance_settings, email_verifications tables; new user columns)
- [ ] `src/initializers/first_run.rs` — `is_first_run()` + `FirstRunGuard` middleware
- [ ] `GET /setup` and `POST /setup` handlers + `assets/views/setup/wizard.html`
- [ ] `src/mailers/verification.rs` — email verification mailer
- [ ] `GET /verify-email` and `POST /resend-verification` handlers
- [ ] `users.standing_score` maintained by `UserModel::update_standing()`
- [ ] Standing updated in `VoteModel::cast_vote` and `*Model::soft_remove`
- [ ] `src/models/eligibility.rs` — `check_community_creation_eligibility()`
- [ ] Eligibility check in community creation handlers (GET + POST)
- [ ] `GET /admin/settings` and `POST /admin/settings` with eligibility form
- [ ] Admin dashboard shows standing_score, email_verified, suspended_at per user
- [ ] Suspend / Unsuspend and Force-verify-email admin actions
- [ ] User settings page shows eligibility status and reasons
- [ ] All tests pass (`cargo test`)
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes
- [ ] `cargo fmt` applied

---

## Common Mistakes

- DO NOT allow `/setup` to be accessed after first user exists — check `is_first_run()`
  on every POST /setup, not just on GET
- DO NOT cache `is_first_run()` indefinitely — cache for 60 seconds max, or bust
  the cache after setup completes
- DO NOT block `/static/*` paths in `FirstRunGuard` — CSS and JS must load on the setup page
- DO NOT hardcode the standing penalty for content removal — make it a constant or
  config value so it can be tuned
- DO NOT let standing_score go below a floor that you define (e.g. -100) — unbounded
  negative scores are a moderation abuse vector
- DO NOT forget to reverse the standing delta when a vote is removed
  (VoteAction::Removed case in cast_vote)
- DO NOT send verification emails in tests — use a mock mailer in test configuration

---

## Prompts

Prompts for redoing or extending this phase with an AI coding agent. They work with any
tool; see [INSTRUCTIONS.md](INSTRUCTIONS.md) for the workflow.

### Implement

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
  cargo fmt && cargo clippy --all-targets --all-features -- -D warnings && cargo test

Verify the Phase 6 deliverables checklist when done.
```

### Review

```
You are a QA engineer reviewing Phase 6 of the Taverna project.

Read AGENTS.md, docs/ai/SPEC.md Sections 3.5–3.7 and 4.11–4.13, and docs/ai/PHASE_6.md.

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
