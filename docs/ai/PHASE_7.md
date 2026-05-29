# Phase 7 — Bug Fixes & UX Improvements

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

Change `views/setup/wizard.html` to extend `base.html`:

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

Update `views/layouts/base.html` to conditionally hide the nav:

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

In `views/communities/list.html` (create this if it does not exist), handle the
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

If the routes exist but the form is broken or missing, fix `views/communities/new.html`.

**Nav link** — add to `views/layouts/base.html`, visible to all logged-in users
(subject to eligibility check in Phase 6, but the link itself always shows):

```html
{% if current_user %}
  <a href="/c/new" style="color:var(--nav-text)">+ New Community</a>
{% endif %}
```

**`views/communities/new.html`:**

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

Update `views/communities/new_post.html` (or wherever the new post form lives)
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

In `views/partials/post_card.html`, wrap the post title in an anchor tag:

```html
<!-- Before (broken): -->
<span class="post-title">{{ post.title }}</span>

<!-- After (fixed): -->
<a href="/c/{{ community.slug }}/posts/{{ post.id }}"
   style="color:var(--text-primary); text-decoration:none; font-weight:bold;">
  {{ post.title }}
</a>
```

Also check `views/communities/feed.html` — if it renders post titles directly
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

Update the login form in `views/auth/login.html`:

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

Update `views/admin/dashboard.html` to add a communities section below the
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

## Task 7.8 — Tests

Phase 7 tests are integrated into existing test files to maintain logical grouping:

- `tests/setup_test.rs`: `test_setup_page_uses_base_layout`
- `tests/community_test.rs`: `test_communities_list_empty_state`, `test_community_create_link_in_nav`, `test_new_community_page_renders`
- `tests/post_test.rs`: `test_new_post_has_type_selector`, `test_post_title_is_a_link`, `test_post_detail_page_accessible`
- `tests/auth_test.rs`: `test_login_success` (verified with username identifier)
- `tests/admin_test.rs`: `test_admin_dashboard_shows_communities`

---

## Deliverables Checklist

- [x] Setup wizard extends `base.html`, uses `data-theme`, hides nav via `hide_nav`
- [x] `GET /c/` returns 200 with empty-state message when no communities exist
- [x] `GET /c/new` renders the new community form (with eligibility check)
- [x] `+ New Community` link visible in nav for logged-in users
- [x] New post form has Text / Link tab selector
- [x] Post titles in feed are clickable anchor tags linking to post detail
- [x] `GET /c/:slug/posts/:id` returns 200 (verify route is registered)
- [x] Login form field is `identifier` (username or email), not `email`
- [x] `POST /login` resolves identifier as username if no `@`, email if contains `@`
- [x] Admin dashboard includes communities table with member/post counts
- [x] All integrated tests pass
- [x] `cargo check`, `cargo clippy -- -D warnings`, `cargo fmt` all pass
 
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
