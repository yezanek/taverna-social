# Phase 7 — Bug Fixes & UX Improvements

> **Status:** complete. This file is the plan this phase was built from. It is kept as a
> record of why the code looks the way it does, and as a template for planning similar work.
> Where it disagrees with the code, `SPEC.md`, `DECISIONS.md` or [DESIGN.md](../DESIGN.md), those win.
>
> **Changed since:** Some UI described here was redesigned later (PR #28); the templates in `assets/views/` are the current UI.

**Goal:** Fix six confirmed bugs and one missing feature discovered during early
use. All issues are self-contained — no new database tables, no federation
changes. This phase is purely UI, routing, and logic corrections.

**Prerequisites:** Phases 1–6 complete and the application running.

---

## Issues Being Fixed

| # | Issue | Type |
|---|---|---|
| 1 | Setup wizard is unstyled (ignores theme system) | Bug |
| 2 | Communities list crashes when no communities exist | Bug |
| 3 | No way to create a new community from the UI | Missing feature |
| 4 | New post form has no link/text type selector | Missing feature |
| 5 | Clicking a post title does not navigate to the post | Bug |
| 6 | Login only accepts email, not username | Bug |
| 7 | Admin dashboard shows only users, not communities | Missing feature |

---

## Task 7.1 — Setup wizard uses the theme system

**Problem:** The setup wizard at `/setup` renders with a plain standalone layout
that ignores `theme.css` and the `data-theme` attribute. It looks broken compared
to the rest of the platform.

**Fix:**

The setup page should use `base.html` like every other page, with two small
adjustments:

- Hide the navigation bar on the setup page (there is nothing to navigate to yet)
- Still load `theme.css` and apply `data-theme` from the cookie (default `light`
  if no cookie is set yet)

Change `assets/views/setup/wizard.html` to extend `base.html`:

```html
{% extends "layouts/base.html" %}

{% block title %}Set up Taverna{% endblock %}

{% block content %}
<div style="max-width:480px; margin:4rem auto;">
  <h1 style="color:var(--text-primary)">Set up your Taverna instance</h1>
  <p style="color:var(--text-secondary)">
    Create the administrator account to get started.
  </p>
  <!-- form fields -->
</div>
{% endblock %}
```

Update `assets/views/layouts/base.html` to conditionally hide the nav:

```html
{% if not hide_nav %}
<nav> ... </nav>
{% endif %}
```

In the setup handlers, inject `hide_nav: true` into the Tera context alongside
`theme`. The `CurrentTheme` middleware already runs on all routes including
`/setup`, so `data-theme` will be set correctly without any extra work.

**Note:** Do NOT use a separate plain layout. The setup page must use the same
CSS variables and theme system as the rest of the platform.

---

## Task 7.2 — Communities list handles empty state

**Problem:** `GET /c/` crashes (or shows an error page) when there are no
communities in the database. The query likely panics or the template fails to
render an empty list.

**Fix:**

In `src/controllers/communities.rs`, the `list_communities` handler must treat
an empty result as a valid state, not an error:

```rust
// Correct — empty vec is fine
let communities = CommunityModel::list_all(&ctx.db, page, PER_PAGE).await?;
// Render even if communities is empty
```

In `assets/views/communities/list.html` (create this if it does not exist), handle the
empty case explicitly:

```html
{% if communities | length == 0 %}
  <div class="card" style="text-align:center; padding:3rem;">
    <p style="color:var(--text-secondary)">No communities yet.</p>
    {% if current_user %}
      <a href="/c/new" style="color:var(--accent)">Create the first one →</a>
    {% endif %}
  </div>
{% else %}
  {% for community in communities %}
    <!-- community card -->
  {% endfor %}
{% endif %}
```

Also check `CommunityModel::list_all` — if it returns an error on empty results
instead of `Ok(vec![])`, fix that too.

---

## Task 7.3 — Create new community page and nav link

**Problem:** There is no route or link to create a new community. Users have
no way to discover or navigate to community creation.

**Fix:**

**Route** (should already exist from Phase 3 — verify it is registered):
```
GET  /c/new    → new_community_form handler
POST /c/       → create_community handler
```

If the routes exist but the form is broken or missing, fix `assets/views/communities/new.html`.

**Nav link** — add to `assets/views/layouts/base.html`, visible to all logged-in users
(subject to eligibility check in Phase 6, but the link itself always shows):

```html
{% if current_user %}
  <a href="/c/new" style="color:var(--nav-text)">+ New Community</a>
{% endif %}
```

**`assets/views/communities/new.html`:**

```html
{% extends "layouts/base.html" %}
{% block title %}Create a Community — Taverna{% endblock %}
{% block content %}
<div style="max-width:600px; margin:2rem auto;">
  <h1 style="color:var(--text-primary)">Create a Community</h1>

  {% if not eligible %}
    <div class="card" style="border-left:4px solid var(--accent); padding:1rem;">
      <p style="color:var(--text-primary); font-weight:bold">
        You are not yet eligible to create communities.
      </p>
      <ul>
        {% for reason in eligibility_reasons %}
          <li style="color:var(--text-secondary)">{{ reason }}</li>
        {% endfor %}
      </ul>
    </div>
  {% else %}
    <form method="POST" action="/c/">
      <label style="color:var(--text-primary)">
        Slug (URL name, e.g. "rust-programming")
        <input type="text" name="slug" required
               pattern="[a-z0-9_-]{3,30}"
               title="3–30 characters, lowercase letters, numbers, hyphens, underscores"
               style="display:block; width:100%; margin-top:0.25rem;
                      background:var(--bg-secondary); color:var(--text-primary);
                      border:1px solid var(--border); border-radius:4px; padding:0.5rem;">
      </label>
      <br>
      <label style="color:var(--text-primary)">
        Display name
        <input type="text" name="name" required
               style="display:block; width:100%; margin-top:0.25rem;
                      background:var(--bg-secondary); color:var(--text-primary);
                      border:1px solid var(--border); border-radius:4px; padding:0.5rem;">
      </label>
      <br>
      <label style="color:var(--text-primary)">
        Description (markdown)
        <textarea name="description" rows="4"
                  style="display:block; width:100%; margin-top:0.25rem;
                         background:var(--bg-secondary); color:var(--text-primary);
                         border:1px solid var(--border); border-radius:4px; padding:0.5rem;">
        </textarea>
      </label>
      <br>
      <label style="color:var(--text-primary)">
        <input type="checkbox" name="is_nsfw" value="true"> Mark as NSFW
      </label>
      <br><br>
      <button type="submit"
              style="background:var(--accent); color:#fff; border:none;
                     border-radius:4px; padding:0.6rem 1.5rem; cursor:pointer">
        Create Community
      </button>
    </form>
  {% endif %}
</div>
{% endblock %}
```

The handler for `GET /c/new` must run the eligibility check and pass `eligible`
and `eligibility_reasons` into the template context.

---

## Task 7.4 — Post type selector (link vs. text)

**Problem:** The new post form has no way to distinguish between a link post and
a text post. The `post_has_content` constraint requires at least one of `body`
or `url`, but the UI gives no guidance.

**Fix:**

Update `assets/views/communities/new_post.html` (or wherever the new post form lives)
to add a two-tab selector at the top of the form, similar to Reddit's approach:

```html
<!-- Tab switcher — no JavaScript needed, uses radio buttons + CSS -->
<div class="post-type-tabs" style="display:flex; gap:0; margin-bottom:1rem;
     border:1px solid var(--border); border-radius:4px; overflow:hidden;">
  <label style="flex:1; text-align:center; padding:0.6rem;
                background:var(--bg-secondary); color:var(--text-secondary);
                cursor:pointer; border-right:1px solid var(--border);">
    <input type="radio" name="post_type" value="text" checked
           style="display:none"
           onchange="document.getElementById('url-field').style.display='none';
                     document.getElementById('body-field').style.display='block';">
    ✏ Text
  </label>
  <label style="flex:1; text-align:center; padding:0.6rem;
                background:var(--bg-secondary); color:var(--text-secondary);
                cursor:pointer;">
    <input type="radio" name="post_type" value="link"
           style="display:none"
           onchange="document.getElementById('url-field').style.display='block';
                     document.getElementById('body-field').style.display='none';">
    🔗 Link
  </label>
</div>

<div id="url-field" style="display:none; margin-bottom:1rem;">
  <label style="color:var(--text-primary)">
    URL
    <input type="url" name="url" placeholder="https://..."
           style="display:block; width:100%; margin-top:0.25rem;
                  background:var(--bg-secondary); color:var(--text-primary);
                  border:1px solid var(--border); border-radius:4px; padding:0.5rem;">
  </label>
</div>

<div id="body-field" style="margin-bottom:1rem;">
  <label style="color:var(--text-primary)">
    Text (markdown)
    <textarea name="body" rows="6"
              style="display:block; width:100%; margin-top:0.25rem;
                     background:var(--bg-secondary); color:var(--text-primary);
                     border:1px solid var(--border); border-radius:4px; padding:0.5rem;">
    </textarea>
  </label>
</div>
```

Note: this uses minimal inline JavaScript (`onchange`) only for the tab switch —
acceptable because it is purely cosmetic, not functional. The form still submits
correctly without JavaScript (both fields are present in the DOM, the server
validates `post_has_content`).

Server-side: the `create_post` handler already accepts both `body` and `url` as
optional fields. No changes needed in Rust — only the template.

---

## Task 7.5 — Post title links to the post detail page

**Problem:** Clicking the title of a post in the community feed does nothing
(or goes to the wrong URL).

**Fix:**

In `assets/views/partials/post_card.html`, wrap the post title in an anchor tag:

```html
<!-- Before (broken): -->
<span class="post-title">{{ post.title }}</span>

<!-- After (fixed): -->
<a href="/c/{{ community.slug }}/posts/{{ post.id }}"
   style="color:var(--text-primary); text-decoration:none; font-weight:bold;">
  {{ post.title }}
</a>
```

Also check `assets/views/communities/feed.html` — if it renders post titles directly
rather than using the `post_card` partial, fix it there too.

Verify the route `GET /c/:slug/posts/:id` is registered in `src/app.rs` and
the handler returns the post detail page (not a redirect or 404).

If the handler exists but returns an error when the post has no community
(profile posts), fix the handler to handle `community_id IS NULL` gracefully.

---

## Task 7.6 — Login accepts username or email

**Problem:** The login form and handler only accept email. Users who remember
their username cannot log in without knowing their email.

**Fix:**

Update the login form in `assets/views/auth/login.html`:

```html
<!-- Change the label and input -->
<label>
  Username or email
  <input type="text" name="identifier" required autocomplete="username"
         placeholder="username or email@example.com">
</label>
```

Update the `POST /login` handler in `src/controllers/auth.rs`:

```rust
#[derive(Deserialize)]
pub struct LoginForm {
    pub identifier: String,   // was: email
    pub password: String,
}

// In the handler:
let user = if form.identifier.contains('@') {
    UserModel::find_by_email(&ctx.db, &form.identifier).await?
} else {
    UserModel::find_by_username(&ctx.db, &form.identifier).await?
};
```

The error message must remain generic regardless of which lookup fails:
"Invalid credentials" — never reveal whether the identifier was not found
or the password was wrong.

Add `UserModel::find_by_username` if it does not already exist (it should from
Phase 2, but verify).

---

## Task 7.7 — Admin dashboard includes communities

**Problem:** The admin dashboard at `GET /admin` only shows a list of users.
There is no way to see, search, or manage communities from the admin UI.

**Fix:**

Update the `GET /admin` handler to also query communities:

```rust
let users = UserModel::list_all_local(&ctx.db, user_page, PER_PAGE).await?;
let communities = CommunityModel::list_all(&ctx.db, community_page, PER_PAGE).await?;
```

Update `assets/views/admin/dashboard.html` to add a communities section below the
users section:

```html
<h2 style="color:var(--text-primary)">Communities</h2>
<table style="width:100%; border-collapse:collapse;">
  <thead>
    <tr style="background:var(--bg-secondary);">
      <th style="padding:0.5rem; text-align:left; color:var(--text-primary)">Slug</th>
      <th style="padding:0.5rem; text-align:left; color:var(--text-primary)">Name</th>
      <th style="padding:0.5rem; text-align:left; color:var(--text-primary)">Members</th>
      <th style="padding:0.5rem; text-align:left; color:var(--text-primary)">Posts</th>
      <th style="padding:0.5rem; text-align:left; color:var(--text-primary)">Local</th>
      <th style="padding:0.5rem; text-align:left; color:var(--text-primary)">Actions</th>
    </tr>
  </thead>
  <tbody>
    {% for community in communities %}
    <tr style="border-bottom:1px solid var(--border);">
      <td style="padding:0.5rem;">
        <a href="/c/{{ community.slug }}" style="color:var(--link)">
          {{ community.slug }}
        </a>
      </td>
      <td style="padding:0.5rem; color:var(--text-primary)">{{ community.name }}</td>
      <td style="padding:0.5rem; color:var(--text-secondary)">{{ community.member_count }}</td>
      <td style="padding:0.5rem; color:var(--text-secondary)">{{ community.post_count }}</td>
      <td style="padding:0.5rem; color:var(--text-secondary)">
        {% if community.is_local %}✓{% else %}remote{% endif %}
      </td>
      <td style="padding:0.5rem;">
        <form method="POST" action="/admin/communities/{{ community.slug }}/delete"
              style="display:inline">
          <button type="submit"
                  style="background:transparent; border:1px solid var(--accent);
                         color:var(--accent); border-radius:4px; padding:0.2rem 0.6rem;
                         cursor:pointer; font-size:0.85rem;"
                  onclick="return confirm('Delete this community?')">
            Delete
          </button>
        </form>
      </td>
    </tr>
    {% endfor %}
  </tbody>
</table>
```

Add `member_count` and `post_count` as computed fields. The simplest approach
is a query that joins and counts, or two separate COUNT queries per community.
For the admin dashboard, N+1 is acceptable at small scale — but prefer a single
query with subquery counts if you can:

```sql
SELECT c.*,
  (SELECT COUNT(*) FROM community_members cm WHERE cm.community_id = c.id) AS member_count,
  (SELECT COUNT(*) FROM posts p WHERE p.community_id = c.id AND p.is_deleted = false AND p.is_removed = false) AS post_count
FROM communities c
WHERE c.is_local = true
ORDER BY c.created_at DESC
LIMIT $1 OFFSET $2
```

---

## Task 7.9 — Admin dashboard link in nav

**Problem:** There is no link to the admin dashboard in the navigation bar,
even when logged in as an admin. Admins must navigate to `/admin` manually.

**Fix:**

In `assets/views/layouts/base.html`, add an admin link visible only to admin users:

```html
{% if current_user %}
  <a href="/c/new" style="color:var(--nav-text)">+ New Community</a>
  {% if current_user.is_admin %}
    <a href="/admin"
       style="color:var(--nav-text); border:1px solid var(--nav-text);
              border-radius:4px; padding:0.2rem 0.6rem; font-size:0.85rem;">
      ⚙ Admin
    </a>
  {% endif %}
{% endif %}
```

The `current_user` variable must expose `is_admin`. Verify the Tera context
includes this field — if `current_user` is a struct serialised to JSON, SeaORM
entities include all columns by default, so `is_admin` should already be present.

Add to the admin_test.rs tests described in Task 7.8.

## Task 7.8 — Tests

Tests belong in the file that owns the domain being tested, not a shared phase file.

**tests/auth_test.rs** — add:
```rust
// Login by username
test_login_with_username()
  // POST /login with identifier=username (not email) must succeed and set cookies

// Login by email still works after the identifier change
test_login_with_email_still_works()
  // POST /login with identifier=email@example.com must still succeed
```

**tests/community_test.rs** — add:
```rust
// Empty communities list returns 200
test_communities_list_empty_state()
  // GET /c/ with no communities must return 200
  // Response body must contain "No communities yet" or equivalent

// New community page accessible to eligible user
test_new_community_page_renders()
  // GET /c/new as logged-in eligible user must return 200
  // Response must contain a form with action="/c/"
```

**tests/post_test.rs** — add:
```rust
// Post type selector present in new post form
test_new_post_has_type_selector()
  // GET /c/:slug/new as member must return 200
  // Response body must contain name="url", name="body", name="post_type"

// Post titles link to post detail
test_post_title_is_a_link()
  // GET /c/:slug feed must contain <a href="/c/:slug/posts/:id"> for each post

// Post detail page is accessible
test_post_detail_page_accessible()
  // GET /c/:slug/posts/:id must return 200 and contain the post title
```

**tests/admin_test.rs** — add (create file if it does not exist):
```rust
// Admin dashboard shows communities section
test_admin_dashboard_shows_communities()
  // GET /admin as admin must contain a community table
  // Must list at least one community if one exists in DB

// Admin nav link visible when logged in as admin
test_admin_link_visible_for_admin()
  // GET / as admin user must contain href="/admin" in the response body

// Admin nav link not visible for non-admin
test_admin_link_hidden_for_regular_user()
  // GET / as regular user must NOT contain href="/admin"
```

**tests/setup_test.rs** — add (or add to existing Phase 6 setup tests):
```rust
// Setup page uses theme system
test_setup_page_uses_base_layout()
  // GET /setup response must contain data-theme= on <html>
  // Must contain <link rel="stylesheet" href="/static/theme.css">
  // Must NOT contain the nav bar (hide_nav=true removes it)
```

---

## Deliverables Checklist

- [ ] Setup wizard extends `base.html`, uses `data-theme`, hides nav via `hide_nav`
- [ ] `GET /c/` returns 200 with empty-state message when no communities exist
- [ ] `GET /c/new` renders the new community form (with eligibility check)
- [ ] `+ New Community` link visible in nav for logged-in users
- [ ] New post form has Text / Link tab selector
- [ ] Post titles in feed are clickable anchor tags linking to post detail
- [ ] `GET /c/:slug/posts/:id` returns 200 (verify route is registered)
- [ ] Login form field is `identifier` (username or email), not `email`
- [ ] `POST /login` resolves identifier as username if no `@`, email if contains `@`
- [ ] Admin dashboard includes communities table with member/post counts
- [ ] Admin nav link (⚙ Admin) visible in nav bar when logged in as admin
- [ ] Admin nav link not visible to non-admin users
- [ ] New tests added to their semantic test files (auth_test.rs, community_test.rs, post_test.rs, admin_test.rs, setup_test.rs)
- [ ] `cargo test` passes
- [ ] `cargo check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt` all pass

---

## Common Mistakes

- DO NOT create a new plain layout for the setup page — extend `base.html`
  and use `hide_nav: true` in the context
- DO NOT use JavaScript for the post type selector's core functionality —
  the minimal `onchange` inline handler is acceptable only for cosmetic toggling;
  the form must work without it
- DO NOT change the login error message to reveal whether it was the
  identifier or the password that was wrong
- DO NOT forget to pass `community` (or at least `community.slug`) into the
  `post_card` partial context — the title link needs the slug
- DO NOT add member_count and post_count as permanent columns on the communities
  table — compute them in the admin query only

---

## Prompts

Prompts for redoing or extending this phase with an AI coding agent. They work with any
tool; see [INSTRUCTIONS.md](INSTRUCTIONS.md) for the workflow.

### Implement

```
Read AGENTS.md and docs/ai/PHASE_7.md.

Phases 1–6 are complete. This phase fixes confirmed bugs and missing features
found during early use. No new database tables. No federation changes.

Show me a plan before writing any code.

Work through the tasks in order. After each task run cargo check.

Task 7.1 — Setup wizard styling
  Extend base.html, pass hide_nav=true into context. Do NOT create a new layout.

Task 7.2 — Empty communities list
  list_all must return Ok(vec![]) not an error. Template must render an
  empty-state message with a link to /c/new for logged-in users.

Task 7.3 — New community page and nav link
  Verify GET /c/new and POST /c/ routes are registered.
  Add "+ New Community" link to base.html nav (logged-in users only).
  Run eligibility check in the GET handler; pass eligible + reasons to template.

Task 7.4 — Post type selector
  Template change only — Text/Link tab using radio buttons.
  The form must work without JavaScript (both fields present in DOM always).
  No Rust changes needed.

Task 7.5 — Post title link
  Wrap post title in <a href="/c/{{ community.slug }}/posts/{{ post.id }}">.
  Verify the route GET /c/:slug/posts/:id is registered and returns 200.

Task 7.6 — Login by username or email
  Change form field from email to identifier.
  Handler: if identifier contains @ treat as email, else treat as username.
  Error message must remain generic — never reveal which field was wrong.

Task 7.7 — Admin communities table
  Query communities with member_count and post_count computed in SQL.
  Do NOT add these as permanent columns — compute in the admin query only.

Task 7.9 — Admin nav link
  Add ⚙ Admin link in base.html nav, inside {% if current_user.is_admin %}.
  Verify current_user.is_admin is available in the Tera context.

Task 7.8 — Tests
  Add tests to their semantic files — do NOT create tests/phase7_test.rs.
  auth_test.rs: test_login_with_username, test_login_with_email_still_works
  community_test.rs: test_communities_list_empty_state, test_new_community_page_renders
  post_test.rs: test_new_post_has_type_selector, test_post_title_is_a_link, test_post_detail_page_accessible
  admin_test.rs: test_admin_dashboard_shows_communities, test_admin_link_visible_for_admin, test_admin_link_hidden_for_regular_user
  setup_test.rs: test_setup_page_uses_base_layout

After all tasks:
  cargo fmt && cargo clippy --all-targets --all-features -- -D warnings && cargo test

Verify the Phase 7 deliverables checklist when done.
```

### Review

```
You are a QA engineer reviewing Phase 7 of the Taverna project.

Read AGENTS.md and docs/ai/PHASE_7.md.

Verify each fix:

TASK 7.1 — Setup wizard styling
- GET /setup response body must contain data-theme= attribute on the <html> element.
- Response must contain <link rel="stylesheet" href="/static/theme.css">.
- Response must NOT contain the nav bar (no <nav> element, or nav is hidden).
- Must NOT reference a separate plain layout — must extend base.html.

TASK 7.2 — Empty communities list
- GET /c/ with zero communities in DB must return HTTP 200, not 500 or 404.
- Response body must contain an empty-state message (e.g. "No communities yet").
- For logged-in users, must contain a link to /c/new.
- CommunityModel::list_all must return Ok(vec![]) not Err on empty result.

TASK 7.3 — New community page and nav link
- GET /c/new as a logged-in eligible user must return 200.
- Response must contain a form with action="/c/" and fields: slug, name, description.
- base.html must contain href="/c/new" inside an {% if current_user %} block.
- Ineligible users seeing GET /c/new must see their eligibility reasons, not a 403.

TASK 7.4 — Post type selector
- GET /c/:slug/new (new post form) response body must contain:
    name="post_type" (the radio selector)
    name="url" (the link field)
    name="body" (the text field)
- Both fields must be present in the DOM (not conditionally omitted server-side).
- Minimal onchange JS for cosmetic toggling is acceptable; core form must work
  without JavaScript.

TASK 7.5 — Post title links
- GET /c/:slug feed response must contain:
    <a href="/c/{slug}/posts/{id}"> wrapping the post title
  for each post in the feed.
- If post_card.html is a partial, check there too.
- GET /c/:slug/posts/:id must return 200 for an existing post.
- Route must be registered in src/app.rs.

TASK 7.6 — Login by username or email
- Login form must have input name="identifier", not name="email".
- POST /login with a username (no @) must authenticate successfully.
- POST /login with an email address must still authenticate successfully.
- Failed login must return a generic error message — not "user not found"
  or "wrong password" separately.
- Check src/controllers/auth.rs: identifier containing @ → find_by_email,
  otherwise → find_by_username.

TASK 7.9 — Admin nav link
- GET / as admin user response body must contain href="/admin".
- GET / as non-admin user response body must NOT contain href="/admin".
- The admin link must be inside a {% if current_user.is_admin %} block in base.html.

TASK 7.7 — Admin communities table
- GET /admin as admin must return a table containing community data.
- Table must include columns for: slug, name, member count, post count.
- member_count and post_count must NOT be permanent columns on the
  communities table — verify by checking migrations and the SeaORM entity.
- Delete button must exist per community row.

Run: cargo test
Tests must be in their semantic files: auth_test.rs, community_test.rs,
post_test.rs, admin_test.rs, setup_test.rs — not a monolithic phase7_test.rs.
Report any failing tests as CRITICAL.

Report CRITICAL / MEDIUM / LOW.
```
