# Phase 2 — Authentication & User Model

**Goal:** Working registration, login, and JWT session management. A user can register, log in, and access protected routes.

**Prerequisites:** Phase 1 complete. `cargo check` passes. Migrations applied. Entities generated.

---

## Tasks

### Task 2.1 — User model (src/models/users.rs)

Create a `UserModel` struct that wraps the SeaORM entity and provides domain methods. Do NOT put business logic in entity files — put it here.

Required methods:

```rust
impl UserModel {
    /// Create a new local user. Generates RSA keypair, hashes password, sets AP URLs.
    pub async fn create_local(
        db: &DatabaseConnection,
        username: &str,
        email: &str,
        password: &str,
        base_url: &str,
    ) -> Result<Model>;

    /// Find by email for login
    pub async fn find_by_email(db: &DatabaseConnection, email: &str) -> Result<Option<Model>>;

    /// Find by username
    pub async fn find_by_username(db: &DatabaseConnection, username: &str) -> Result<Option<Model>>;

    /// Find by AP ID (for federation)
    pub async fn find_by_ap_id(db: &DatabaseConnection, ap_id: &str) -> Result<Option<Model>>;

    /// Verify password. Returns true if password matches hash.
    pub fn verify_password(model: &Model, password: &str) -> bool;

    /// Generate RSA-2048 keypair. Returns (public_key_pem, private_key_pem).
    pub fn generate_keypair() -> Result<(String, String)>;
}
```

**Implementation notes for `create_local`:**
1. Validate username: 3–30 chars, alphanumeric + underscore only. Return error if invalid.
2. Check username is not already taken. Return specific error if taken.
3. Check email is not already taken. Return specific error if taken.
4. Hash password with argon2id.
5. Generate RSA-2048 keypair.
6. Set `ap_id = format!("{}/u/{}", base_url, username)`
7. Set `inbox_url = format!("{}/u/{}/inbox", base_url, username)`
8. Set `followers_url = format!("{}/u/{}/followers", base_url, username)`
9. Set `is_local = true`
10. Insert and return the new user.

### Task 2.2 — JWT utilities (src/common/jwt.rs)

Loco provides JWT support. Extend it:

```rust
pub struct Claims {
    pub sub: String,      // user UUID as string
    pub exp: usize,       // expiration timestamp
    pub is_admin: bool,
}

pub fn create_access_token(user: &Model, secret: &str) -> Result<String>;
pub fn create_refresh_token(user: &Model, secret: &str) -> Result<String>;
pub fn verify_token(token: &str, secret: &str) -> Result<Claims>;
```

Access token: expires in 1 hour.
Refresh token: expires in 7 days.

### Task 2.3 — Auth controller (src/controllers/auth.rs)

Implement these handlers:

```
GET  /register  → render auth/register.html
POST /register  → validate, call UserModel::create_local, redirect to /login
GET  /login     → render auth/login.html
POST /login     → validate credentials, set cookies, redirect to /
POST /logout    → clear cookies, redirect to /login
```

**POST /login details:**
1. Find user by email
2. Call `UserModel::verify_password`
3. On success: create access + refresh tokens
4. Set `access_token` cookie: httpOnly, SameSite=Lax, 1h max-age
5. Set `refresh_token` cookie: httpOnly, SameSite=Lax, 7d max-age
6. Redirect to `/`
7. On failure: re-render login form with error message (do NOT reveal whether email vs password was wrong — just say "invalid credentials")

**POST /register details:**
1. Validate all fields (non-empty, valid formats)
2. Call `UserModel::create_local`
3. On success: redirect to `/login` with success flash message
4. On error: re-render register form with specific field errors

### Task 2.4 — Auth middleware (src/initializers/auth_middleware.rs)

Create an Axum middleware extractor `AuthUser` that:
1. Reads `access_token` cookie
2. Verifies JWT
3. Loads user from DB
4. Makes `AuthUser` available in handlers

Two variants:
- `AuthUser` — required auth. Returns 401 if not authenticated.
- `OptionalAuthUser` — optional auth. Contains `Option<Model>`.

Usage in handlers:
```rust
async fn create_post(
    auth: AuthUser,           // fails with 401 if not logged in
    State(ctx): State<AppContext>,
    // ...
) -> Result<Response>
```

### Task 2.5 — Auth templates

Create minimal but functional templates:

**views/auth/login.html:**
- Extends `layouts/base.html`
- Email input, password input, submit button
- Show error message if present
- Link to register page

**views/auth/register.html:**
- Extends `layouts/base.html`
- Username, email, password, password_confirm inputs
- Show field-level errors
- Link to login page

**views/layouts/base.html:**
- HTML5 boilerplate
- Load HTMX from assets/static/htmx.min.js
- Navigation bar: logo, "Communities", login/register links (or username if logged in)
- `{% block content %}{% endblock %}`
- Minimal inline CSS (no external CSS framework — keep it functional)

### Task 2.6 — Register routes in app.rs

Add auth routes to the Loco router:

```rust
// In src/app.rs, inside the router setup:
Router::new()
    .route("/register", get(auth::register_form).post(auth::register))
    .route("/login", get(auth::login_form).post(auth::login))
    .route("/logout", post(auth::logout))
```

### Task 2.7 — Write tests

Create `tests/auth_test.rs`:

```rust
#[tokio::test]
async fn test_register_creates_user() { /* ... */ }

#[tokio::test]
async fn test_duplicate_username_fails() { /* ... */ }

#[tokio::test]
async fn test_login_valid_credentials() { /* ... */ }

#[tokio::test]
async fn test_login_wrong_password() { /* ... */ }

#[tokio::test]
async fn test_user_has_keypair_after_create() { /* ... */ }
```

---

## Deliverables Checklist

- [ ] `src/models/users.rs` with all 6 methods implemented
- [ ] `src/common/jwt.rs` with create and verify functions
- [ ] `src/controllers/auth.rs` with all 5 handlers
- [ ] `src/initializers/auth_middleware.rs` with `AuthUser` and `OptionalAuthUser`
- [ ] `views/auth/login.html`
- [ ] `views/auth/register.html`
- [ ] `views/layouts/base.html`
- [ ] Routes registered in `src/app.rs`
- [ ] `cargo check` passes
- [ ] `cargo test` passes (auth tests)

---

## Common Mistakes to Avoid

- **DO NOT** store plaintext passwords — use argon2id
- **DO NOT** use `unwrap()` on JWT decode — handle the error and return 401
- **DO NOT** reveal which field was wrong on failed login (security)
- **DO NOT** skip keypair generation for local users — federation requires it
- **DO NOT** use `| safe` on user input in templates
