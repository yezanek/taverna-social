# Findings Verification & Fix Plan

## Date: 2026-05-12
## Status: Verified - Ready for Implementation

---

## Summary

Out of **22 findings** reviewed:
- **14 issues** - ALREADY FIXED (code already addresses them)
- **1 issue** - SKIPPED (raw SQL for ltree is necessary)
- **7+ issues** - NEED FIX (verified against current code)

---

## Part 1: Issues Already Fixed (14)

These can be skipped - the current code already addresses them.

| # | Finding | Verification |
|---|---------|--------------|
| 1 | `communities/list.html` - Add empty state | ✅ Already has `{% if communities %}...{% else %}<p>No communities yet — create one!</p>{% endif %}` |
| 2 | `communities/settings.html` - Add CSRF token + validate | ✅ Template has token, `update_settings` calls `validate_csrf()` |
| 3 | `posts.rs` lines 40-48 - Verify `post.community_id == community.id` | ✅ Checked in `detail`, `vote`, `delete`, `create_comment` handlers (e.g., line 50-52) |
| 4 | `posts.rs` lines 247-255 - `vote_comment` validate `comment.post_id` | ✅ Lines 278-280: `if comment.post_id != post_id { return Err(Error::NotFound) }` |
| 5 | `comments.rs` lines 88-95 - Parent post_id validation | ✅ Lines 92-94: `if parent.post_id != post_id { return Err(CommentError::NotFound) }` |
| 6 | `communities.rs` lines 186-216 - `update_settings` validation | ✅ Lines 186-237: has `MAX_NAME_LEN=100`, `MAX_DESCRIPTION_LEN=5000`, validates name not empty, enforces max lengths, trims input |
| 7 | `posts.rs` lines 183-185 - `increment_comment_count` unwrap | ✅ Lines 185-190: uses `take().ok_or(...)` not `unwrap()` - properly returns error |
| 8 | `posts.rs` lines 40-44 - Whitespace-only body/url validation | ✅ Lines 40-46: uses `body.map(|s| s.trim()).filter(|s| !s.is_empty())` for both body and url |
| 9 | `new_post.html` - Extract inline styles to CSS | ✅ Already uses `.form-row`, `.form-input`, `.form-textarea` classes; `theme.css` defines these classes (lines 100-123) |
| 10 | `home.rs` lines 20-22 - Log DB errors | ✅ Lines 22-25: `tracing::error!(error = %e, "failed to list posts"); Error::InternalServerError` |
| 11 | `votes.rs` lines 24-42 - Vote value 1/-1 validation | ✅ Lines 24-28: `if value != 1 && value != -1 { return Err(...) }` |
| 12 | `votes.rs` lines 24-42 - Exactly one target (post_id/comment_id) | ✅ Lines 29-33: `if post_id.is_some() == comment_id.is_some() { return Err(...) }` |
| 13 | `comments.rs` lines 98-117 - Raw SQL INSERT | ✅ **SKIPPED** - Uses PostgreSQL `ltree` type with `CAST($6 AS ltree)` and `CAST(path AS TEXT)`. SeaORM doesn't natively support ltree casts. This is a legitimate exception. |
| 14 | `votes.rs` ActiveModel - Setting both post_id and comment_id | ✅ Using `Set(post_id)` and `Set(comment_id)` with `Option<Uuid>` is correct - SeaORM properly handles `None` as NULL |

---

## Part 2: Issues Needing Fix (Verified)

### High Priority: Security & Data Protection

#### Issue A: Tracing - `current_user` not skipped in `#[instrument]`

**Sensitive user data could be logged.**

**Files to fix:**

| File | Line | Current | Fix |
|------|------|---------|-----|
| `src/controllers/home.rs` | 13 | `#[instrument(skip(v, ctx, theme))]` | Add `current_user` to skip |
| `src/controllers/posts.rs` | 32 | `#[instrument(skip(v, ctx, theme))]` | Add `current_user` to skip |
| `src/controllers/communities.rs` | 40 | `#[instrument(skip(v, ctx, theme))]` | Add `current_user` to skip |
| `src/controllers/communities.rs` | 63 | `#[instrument(skip(v, ctx, theme))]` | Add `current_user` to skip |

---

#### Issue B: Full CSRF Protection (Multiple Handlers)

**Finding originally mentioned only `new_post.html`, but user requested fixing ALL CSRF gaps.**

Current CSRF coverage:
- ✅ `settings.html` + `update_settings` - Protected
- ❌ All other forms/POST handlers - NOT protected

##### B.1: Form Structs - Add `csrf_token` field

**File: `src/controllers/communities.rs`**
```rust
// Add to CreatePostForm (line 21)
#[derive(Debug, Deserialize)]
pub struct CreatePostForm {
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]  // Add this
    pub csrf_token: Option<String>,  // Add this
}
```

**File: `src/controllers/posts.rs`**
```rust
// Add to VoteForm (line 20)
#[derive(Debug, Deserialize)]
pub struct VoteForm {
    pub vote: i16,
    #[serde(default)]  // Add this
    pub csrf_token: Option<String>,  // Add this
}

// Add to CommentForm (line 25)
#[derive(Debug, Deserialize)]
pub struct CommentForm {
    pub body: String,
    #[serde(default)]
    pub parent_id: Option<Uuid>,
    #[serde(default)]  // Add this
    pub csrf_token: Option<String>,  // Add this
}
```

##### B.2: POST Handlers - Add `validate_csrf()` call

**Import pattern (already in communities.rs):**
```rust
use crate::controllers::auth::validate_csrf;
use axum::http::HeaderMap;
use axum_extra::extract::cookie::CookieJar;
```

**Files needing CSRF validation:**

| File | Handler | Params to Add | Validation Code |
|------|---------|---------------|-----------------|
| `communities.rs` | `create_post` | `jar: CookieJar`, `headers: HeaderMap`, `Extension(taverna)` | `if !validate_csrf(&jar, form.csrf_token.as_deref(), &headers, &taverna) { return Err(Error::Unauthorized(...)); }` |
| `communities.rs` | `join` | `jar: CookieJar`, `headers: HeaderMap`, `Extension(taverna)` | Same (use `None` for form_token since join has no form) |
| `communities.rs` | `leave` | `jar: CookieJar`, `headers: HeaderMap`, `Extension(taverna)` | Same |
| `posts.rs` | `vote` | `jar: CookieJar`, `headers: HeaderMap`, `Extension(taverna)` | `if !validate_csrf(&jar, form.csrf_token.as_deref(), &headers, &taverna) { ... }` |
| `posts.rs` | `delete` | `jar: CookieJar`, `headers: HeaderMap`, `Extension(taverna)` | Same (use `None` for form_token) |
| `posts.rs` | `create_comment` | `jar: CookieJar`, `headers: HeaderMap`, `Extension(taverna)` | `if !validate_csrf(&jar, form.csrf_token.as_deref(), &headers, &taverna) { ... }` |
| `posts.rs` | `vote_comment` | `jar: CookieJar`, `headers: HeaderMap`, `Extension(taverna)` | `if !validate_csrf(&jar, form.csrf_token.as_deref(), &headers, &taverna) { ... }` |

**Note:** For handlers without form tokens (join, leave, delete), we rely on Origin/Referer check which is already part of `validate_csrf`.

##### B.3: GET Handlers - Set CSRF Cookie

Create a helper function (or extract the pattern from `auth.rs`'s `render_with_csrf`).

**Check: `auth.rs` has `render_with_csrf` helper (lines 273-291):**
- Sets CSRF cookie
- Injects `csrf_token` into template context
- Returns response with cookie

**We need to make this `pub(crate)` or create a similar helper in `views/helpers.rs`.**

**Handlers that need to set CSRF cookie:**

| Handler | Renders | Needs CSRF? |
|---------|---------|-------------|
| `home::index` | `home/index.html` | Links to posts with voting - set cookie |
| `communities::list` | `communities/list.html` | Lists communities - set cookie |
| `communities::feed` | `communities/feed.html` | Has join/leave forms - set cookie |
| `communities::new_post_form` | `communities/new_post.html` | Has create post form - set cookie |
| `communities::settings_form` | `communities/settings.html` | Already does this ✓ |
| `posts::detail` | `communities/post.html` | Has votes, comments, replies - set cookie |

##### B.4: Templates - Add CSRF Hidden Input

| Template | Form | Action |
|----------|------|--------|
| `assets/views/communities/new_post.html` | Create post | Add `<input type="hidden" name="csrf_token" value="{{ csrf_token }}">` |
| `assets/views/communities/feed.html` | Join/Leave | Add CSRF token to both forms |
| `assets/views/communities/post.html` | New comment | Add CSRF token to comment form |
| `assets/views/partials/comment.html` | Reply | Add CSRF token to reply form |

##### B.5: HTMX Vote Buttons - Include `csrf_token` in `hx-vals`

**For HTMX, we need to include the token in the request values.**

| Template | Location | Change |
|----------|----------|--------|
| `assets/views/partials/vote_buttons.html` | Lines 4, 16 | Change `hx-vals='{"vote":"1"}'` to `hx-vals='{"vote":"1","csrf_token":"{{ csrf_token }}"}'` |
| `assets/views/communities/post.html` | Lines 20, 32 | Same change for inline vote buttons |
| `assets/views/partials/comment.html` | Lines 11, 23 | Same change for inline comment vote buttons |

---

### Medium Priority: Nitpicks & Code Quality

#### Issue C: `settings.html` - Remove `| default(value='')` filter

**File: `assets/views/communities/settings.html` (line 7)**

Current:
```html
<input type="hidden" name="csrf_token" value="{{ csrf_token | default(value='') }}">
```

Change to:
```html
<input type="hidden" name="csrf_token" value="{{ csrf_token }}">
```

**Reason:** Fail fast if `csrf_token` is not injected into context, rather than silently using empty string.

---

#### Issue D: `votes.rs` - Multiple Nitpicks

**File: `src/models/votes.rs`**

##### D.1: Add serde derives to `VoteAction`

Current (line 6):
```rust
#[derive(Debug, Clone, PartialEq)]
pub enum VoteAction {
```

Change to:
```rust
use serde::{Serialize, Deserialize};  // Add at top

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum VoteAction {
```

##### D.2: Remove unreachable `else` branch

Current (lines 35-51):
```rust
let existing = if let Some(pid) = post_id {
    votes::Entity::find()...
} else if let Some(cid) = comment_id {
    votes::Entity::find()...
} else {
    return Err(sea_orm::DbErr::RecordNotFound(
        "No target specified".to_string(),
    ));
};
```

The `else` branch is unreachable because line 29-33 already validates:
```rust
if post_id.is_some() == comment_id.is_some() {
    return Err(...);
}
```

**Fix:** Restructure to avoid the unreachable else. Use `match` or separate `let` statements.

Option 1 - Use `unwrap_unchecked` logic (but `if let` chain is fine if we remove the else):

Actually, since we've already validated, we can do:
```rust
let existing = if let Some(pid) = post_id {
    votes::Entity::find()
        .filter(votes::Column::UserId.eq(user_id))
        .filter(votes::Column::PostId.eq(pid))
        .one(db)
        .await?
} else {
    // comment_id is guaranteed to be Some by earlier validation
    let cid = comment_id.unwrap();
    votes::Entity::find()
        .filter(votes::Column::UserId.eq(user_id))
        .filter(votes::Column::CommentId.eq(cid))
        .one(db)
        .await?
};
```

##### D.3: Use `DbErr::Custom` instead of `DbErr::RecordNotFound` for validation

Current (lines 24-33):
```rust
if value != 1 && value != -1 {
    return Err(sea_orm::DbErr::RecordNotFound(
        "Vote must be 1 or -1".to_string(),
    ));
}
if post_id.is_some() == comment_id.is_some() {
    return Err(sea_orm::DbErr::RecordNotFound(
        "Exactly one of post_id or comment_id must be set".to_string(),
    ));
}
```

Change to:
```rust
if value != 1 && value != -1 {
    return Err(sea_orm::DbErr::Custom(
        "Vote must be 1 or -1".to_string(),
    ));
}
if post_id.is_some() == comment_id.is_some() {
    return Err(sea_orm::DbErr::Custom(
        "Exactly one of post_id or comment_id must be set".to_string(),
    ));
}
```

**Reason:** These are validation errors, not "record not found" errors.

---

## Part 3: Implementation Order

### Phase 1: Quick Wins (Low Risk)
1. Fix tracing: add `current_user` to `skip()` in 4 handlers
2. Fix `votes.rs` nitpicks (D.1, D.2, D.3)
3. Fix `settings.html` remove default filter (Issue C)

### Phase 2: CSRF Preparation
4. Add `csrf_token` to form structs (`VoteForm`, `CommentForm`, `CreatePostForm`)
5. Export or create `render_with_csrf` helper
6. Update GET handlers to set CSRF cookie

### Phase 3: CSRF Template Updates
7. Update all HTML forms with hidden `csrf_token` input
8. Update all HTMX vote buttons to include `csrf_token` in `hx-vals`

### Phase 4: CSRF Handler Protection
9. Add `validate_csrf()` call to all POST handlers
10. Add required parameters (`CookieJar`, `HeaderMap`, `Extension<TavernaConfig>`)

### Phase 5: Validation
11. Run `cargo check`
12. Run `cargo test`
13. Run `cargo clippy -- -D warnings`
14. Run `cargo fmt`

---

## Part 4: Files to Modify Checklist

### Rust Files
- [ ] `src/controllers/home.rs` - tracing skip
- [ ] `src/controllers/posts.rs` - tracing skip, add csrf_token to forms, add validate_csrf
- [ ] `src/controllers/communities.rs` - tracing skip, add csrf_token to CreatePostForm, add validate_csrf, update GET handlers
- [ ] `src/controllers/auth.rs` - export render_with_csrf or create helper
- [ ] `src/models/votes.rs` - serde derives, remove unreachable else, DbErr::Custom

### Template Files
- [ ] `assets/views/communities/settings.html` - remove default filter
- [ ] `assets/views/communities/new_post.html` - add csrf_token input
- [ ] `assets/views/communities/feed.html` - add csrf_token to join/leave forms
- [ ] `assets/views/communities/post.html` - add csrf_token to comment form, update hx-vals
- [ ] `assets/views/partials/comment.html` - add csrf_token to reply form, update hx-vals
- [ ] `assets/views/partials/vote_buttons.html` - update hx-vals

---

## Part 5: Validation Commands

```bash
# Check compilation
cargo check

# Run tests
cargo test

# Lint
cargo clippy -- -D warnings

# Format
cargo fmt
```

---

## Summary of Changes Count

| Category | Files | Changes |
|----------|-------|---------|
| Tracing security | 4 | Add `current_user` to skip |
| CSRF forms | 3 | Add `csrf_token` field to structs |
| CSRF handlers | 7 | Add `validate_csrf()` call |
| CSRF templates | 6 | Add hidden inputs, update hx-vals |
| votes.rs nitpicks | 1 | 3 changes |
| settings.html | 1 | Remove default filter |

**Total: ~15 files, ~25 individual changes**
