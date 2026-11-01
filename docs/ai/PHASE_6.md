# Phase 6 — Admin Onboarding & Community Creation Eligibility

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

**Template:** `views/setup/wizard.html`
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

**Template:** `views/admin/settings.html`

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
- [ ] `GET /setup` and `POST /setup` handlers + `views/setup/wizard.html`
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
- [ ] `cargo clippy -- -D warnings` passes
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
