# Phase 3 — Communities, Posts & Theme Switcher (Local Only)

**Goal:** Full community and post CRUD, voting, comments, and the theme switcher (light/dark/sepia). No federation yet.

**Prerequisites:** Phase 2 complete. Auth working. `cargo test` passes.

---

## Tasks

### Task 3.1 — Community model (src/models/communities.rs)

```rust
impl CommunityModel {
    pub async fn create(db, slug, name, description, creator_id, base_url) -> Result<Model>;
    pub async fn find_by_slug(db, slug) -> Result<Option<Model>>;
    pub async fn find_by_ap_id(db, ap_id) -> Result<Option<Model>>;
    pub async fn list_all(db, page, per_page) -> Result<Vec<Model>>;
    pub async fn get_member_role(db, community_id, user_id) -> Result<Option<String>>;
    pub async fn add_member(db, community_id, user_id, role) -> Result<()>;
    pub async fn remove_member(db, community_id, user_id) -> Result<()>;
}
```

`create` must: validate slug, check uniqueness, call `generate_actor_keypair()` from `activitypub_federation`, set `ap_id`/`inbox_url`/`followers_url`, set `is_local=true`, insert, add creator as moderator.

### Task 3.2 — Post model (src/models/posts.rs)

```rust
impl PostModel {
    pub async fn create(db, community_id, author_id, title, body, url, base_url) -> Result<Model>;
    pub async fn find_by_id(db, id) -> Result<Option<Model>>;
    pub async fn find_by_ap_id(db, ap_id) -> Result<Option<Model>>;
    pub async fn list_for_community(db, community_id, page, per_page) -> Result<Vec<Model>>;
    pub async fn recalculate_score(db, post_id) -> Result<i32>;
    pub async fn soft_delete(db, post_id) -> Result<()>;
    pub async fn soft_remove(db, post_id) -> Result<()>;
}
```

### Task 3.3 — Comment model (src/models/comments.rs)

```rust
impl CommentModel {
    pub async fn create(db, post_id, author_id, parent_id, body, base_url) -> Result<Model>;
    pub async fn list_for_post(db, post_id) -> Result<Vec<Model>>;
    pub async fn find_by_id(db, id) -> Result<Option<Model>>;
    pub async fn recalculate_score(db, comment_id) -> Result<i32>;
    pub async fn soft_delete(db, comment_id) -> Result<()>;
}
```

ltree path logic:
```rust
let post_seg = format!("c{}", &post_id.to_string().replace("-","")[..8]);
let comment_seg = format!("c{}", &id.to_string().replace("-","")[..8]);
// top-level: format!("{}.{}", post_seg, comment_seg)
// reply: format!("{}.{}", parent.path, comment_seg)
```

### Task 3.4 — Vote model (src/models/votes.rs)

```rust
pub enum VoteAction { Added, Changed, Removed }

impl VoteModel {
    pub async fn cast_vote(db, user_id, post_id, comment_id, value) -> Result<VoteAction>;
    pub async fn get_user_vote_on_post(db, user_id, post_id) -> Result<Option<i16>>;
}
```

Always call `recalculate_score` after any vote change.

### Task 3.5 — Theme middleware (src/initializers/theme_middleware.rs)

Create Axum middleware that:
1. Reads the `taverna_theme` cookie
2. Validates value is one of `light`, `dark`, `sepia`; defaults to `light` if missing or invalid
3. Inserts the theme string into the request extensions as `CurrentTheme(String)`

```rust
#[derive(Clone)]
pub struct CurrentTheme(pub String);
```

This runs on every request so every handler and template has access to the theme.

### Task 3.6 — Theme route handler (src/controllers/theme.rs)

```
POST /theme
```

```rust
#[derive(Deserialize)]
pub struct ThemeForm {
    pub theme: String,
}

pub async fn set_theme(
    Form(form): Form<ThemeForm>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let theme = match form.theme.as_str() {
        "dark" | "sepia" => form.theme.clone(),
        _ => "light".to_string(),
    };
    let redirect_to = headers
        .get("referer")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("/")
        .to_string();

    let cookie = format!(
        "taverna_theme={}; Path=/; Max-Age=31536000; SameSite=Lax",
        theme
    );
    (
        [(axum::http::header::SET_COOKIE, cookie)],
        axum::response::Redirect::to(&redirect_to),
    )
}
```

Note: this cookie is NOT httpOnly — the server reads it via middleware. JavaScript does not need it.

### Task 3.7 — Inject theme into Tera context (src/initializers/tera_globals.rs)

Every template needs the `theme` variable. Instead of adding it to every handler manually, use a Tera global or inject it via a custom response helper.

The recommended approach: create a `render_with_theme` helper:

```rust
pub fn render_with_theme(
    tera: &Tera,
    template_name: &str,
    mut ctx: tera::Context,
    current_theme: &CurrentTheme,
) -> Result<String, AppError> {
    ctx.insert("theme", &current_theme.0);
    Ok(tera.render(template_name, &ctx)?)
}
```

Call this instead of `tera.render()` in every handler. All handlers extract `Extension(theme): Extension<CurrentTheme>` from the request.

### Task 3.8 — Theme CSS file (assets/static/theme.css)

Create `assets/static/theme.css` with the exact CSS custom properties defined in SPEC.md Section 3.5.

The file must contain all three theme blocks:
- `[data-theme="light"] { ... }`
- `[data-theme="dark"] { ... }`
- `[data-theme="sepia"] { ... }`

Plus base styles that use the custom properties for all elements:

```css
body {
  background-color: var(--bg-primary);
  color: var(--text-primary);
  font-family: system-ui, sans-serif;
  margin: 0;
}

a { color: var(--link); }
a:hover { color: var(--link-hover); }

nav {
  background-color: var(--nav-bg);
  color: var(--nav-text);
  padding: 0.75rem 1rem;
  display: flex;
  align-items: center;
  gap: 1rem;
}

.card {
  background: var(--bg-card);
  border: 1px solid var(--border);
  border-radius: 4px;
  padding: 1rem;
  margin-bottom: 0.5rem;
}

.score { color: var(--text-secondary); font-weight: bold; }
.score.positive { color: var(--score-positive); }
.score.negative { color: var(--score-negative); }

/* Theme switcher buttons in nav */
.theme-switcher button {
  background: transparent;
  border: 1px solid var(--nav-text);
  color: var(--nav-text);
  border-radius: 4px;
  padding: 0.2rem 0.5rem;
  cursor: pointer;
  font-size: 0.85rem;
}
.theme-switcher button:hover {
  background: rgba(255,255,255,0.15);
}
```

### Task 3.9 — Update base layout template (views/layouts/base.html)

```html
<!DOCTYPE html>
<html lang="en" data-theme="{{ theme }}">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>{% block title %}Taverna{% endblock %}</title>
  <link rel="stylesheet" href="/static/theme.css">
  <script src="/static/htmx.min.js" defer></script>
</head>
<body>
  <nav>
    <a href="/" style="color:var(--nav-text); font-weight:bold; text-decoration:none">Taverna</a>
    <a href="/c/" style="color:var(--nav-text)">Communities</a>
    <span style="flex:1"></span>

    <!-- Theme switcher: no JS needed, plain form POST -->
    <form class="theme-switcher" method="POST" action="/theme">
      <button type="submit" name="theme" value="light"  title="Light mode">☀</button>
      <button type="submit" name="theme" value="dark"   title="Dark mode">●</button>
      <button type="submit" name="theme" value="sepia"  title="Sepia mode">✦</button>
    </form>

    {% if current_user %}
      <a href="/u/{{ current_user.username }}" style="color:var(--nav-text)">
        {{ current_user.display_name | default(value=current_user.username) }}
      </a>
      <form method="POST" action="/logout" style="display:inline">
        <button type="submit" style="background:transparent;border:none;color:var(--nav-text);cursor:pointer">
          Log out
        </button>
      </form>
    {% else %}
      <a href="/login" style="color:var(--nav-text)">Log in</a>
      <a href="/register" style="color:var(--nav-text)">Register</a>
    {% endif %}
  </nav>

  <main style="max-width:900px; margin:0 auto; padding:1rem">
    {% block content %}{% endblock %}
  </main>
</body>
</html>
```

### Task 3.10 — Communities controller (src/controllers/communities.rs)

```
GET  /c/              → list communities (paginated)
GET  /c/:slug         → community feed (OptionalAuthUser)
GET  /c/:slug/new     → new post form (AuthUser)
POST /c/:slug/posts   → create post (AuthUser, must be member)
POST /c/:slug/join    → join (AuthUser)
POST /c/:slug/leave   → leave (AuthUser)
GET  /c/:slug/settings → settings form (moderator only)
POST /c/:slug/settings → update settings (moderator only)
```

All handlers extract `Extension(theme): Extension<CurrentTheme>` and pass it to `render_with_theme`.

### Task 3.11 — Posts controller (src/controllers/posts.rs)

```
GET  /c/:slug/posts/:id                     → post detail + threaded comments
POST /c/:slug/posts/:id/vote                → vote (HTMX, returns score fragment only)
POST /c/:slug/posts/:id/delete              → delete
POST /c/:slug/posts/:id/comments            → create comment
POST /c/:slug/posts/:id/comments/:cid/vote  → vote comment (HTMX fragment)
```

Vote endpoint detects `HX-Request: true` and returns only the score HTML fragment (no theme injection needed for fragments).

### Task 3.12 — Markdown utility (src/common/markdown.rs)

```rust
pub fn render_markdown(input: &str) -> String {
    let parser = pulldown_cmark::Parser::new(input);
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, parser);
    ammonia::clean(&html)
}
```

Store raw markdown. Render in handler, pass rendered HTML to template context.

### Task 3.13 — Templates

- `views/communities/feed.html` — community info, join/leave (HTMX), post list, infinite scroll
- `views/communities/post.html` — post, vote buttons, comment form, threaded comments
- `views/partials/post_card.html` — score, votes (HTMX), title, author, time, comment count
- `views/partials/comment.html` — score, votes (HTMX), author, body (rendered MD), reply button
- `views/partials/vote_buttons.html` — the HTMX swap target for votes

All templates use CSS custom properties via `var(--...)` — no hardcoded colours.

### Task 3.14 — Home feed (src/controllers/home.rs)

```
GET / → posts across all communities, score DESC, paginated
```

### Task 3.15 — Register all routes in app.rs

Include `POST /theme` → `theme::set_theme`.
Include theme middleware in the middleware stack (must run before all handlers).

### Task 3.16 — Tests

```rust
// tests/theme_test.rs
test_set_theme_light()           // POST /theme with theme=light sets cookie
test_set_theme_dark()
test_set_theme_sepia()
test_invalid_theme_defaults_to_light()
test_theme_cookie_max_age()      // cookie has Max-Age=31536000
test_theme_middleware_reads_cookie()  // middleware injects CurrentTheme correctly

// tests/community_test.rs
test_create_community_sets_ap_id()
test_duplicate_slug_fails()
test_creator_is_moderator()
test_join_community()

// tests/post_test.rs
test_create_text_post()
test_create_link_post()
test_post_requires_body_or_url()
test_vote_up()
test_vote_flip()
test_vote_toggle_removes()
test_score_recalculates()

// tests/comment_test.rs
test_top_level_comment_path()
test_reply_path_extends_parent()
test_nested_reply_path()
```

---

## Deliverables Checklist

- [ ] `src/models/communities.rs` — all methods
- [ ] `src/models/posts.rs` — all methods
- [ ] `src/models/comments.rs` — all methods, correct ltree path logic
- [ ] `src/models/votes.rs` — toggle logic
- [ ] `src/common/markdown.rs`
- [ ] `src/initializers/theme_middleware.rs` — `CurrentTheme` extractor
- [ ] `src/controllers/theme.rs` — `POST /theme` handler with cookie
- [ ] `assets/static/theme.css` — all three themes with CSS custom properties
- [ ] `views/layouts/base.html` — `data-theme="{{ theme }}"`, theme switcher form, nav
- [ ] All community and post templates using `var(--...)` colours
- [ ] All routes registered in app.rs including `/theme`
- [ ] Theme middleware in middleware stack
- [ ] `cargo check` and `cargo test` pass (including theme tests)

---

## Common Mistakes

- DO NOT hardcode colours in templates — use `var(--bg-primary)` etc.
- DO NOT make the theme cookie httpOnly — the middleware reads it server-side, but it must be readable as a cookie header, not a JS property
- DO NOT return full pages from HTMX vote endpoints — fragments only
- DO NOT forget to call `recalculate_score` after every vote
- DO NOT apply theme injection to HTMX fragment responses — only to full-page responses
- DO NOT forget `post_has_content` constraint (body OR url required)
