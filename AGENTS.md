# Taverna — Agent Instructions

**Read this file first. Read SPEC.md second. Then read your phase file.**

---

## What You Are Building

You are implementing Taverna: a federated community platform (like Reddit + Facebook Groups) that speaks ActivityPub. The full specification is in `SPEC.md`. This file tells you HOW to work. SPEC.md tells you WHAT to build.

---

## Non-Negotiable Rules

1. **Follow SPEC.md exactly.** If you are unsure about a requirement, re-read SPEC.md before guessing.
2. **Do not add features not in SPEC.md.** No media uploads, no OAuth, no full-text search. Build what is specified.
3. **Do not change the database schema** unless SPEC.md says to. The schema is the contract between phases.
4. **Write real code.** No placeholder functions, no `todo!()` macros left in delivered code, no stub implementations that silently succeed.
5. **Every function must be used.** Do not write dead code.
6. **Handle errors properly.** Use `Result<T, E>` everywhere. Do not `.unwrap()` in production paths.
7. **No JavaScript frameworks.** The frontend is HTMX + Tera templates only.
8. **PostgreSQL only.** No Redis, no SQLite, no in-memory stores.

---

## Project Layout (Loco conventions)

```
taverna/
├── Cargo.toml
├── config/
│   ├── development.yaml
│   └── production.yaml
├── migration/
│   └── src/
│       ├── lib.rs
│       ├── m001_extensions.rs
│       ├── m002_users.rs
│       ├── m003_communities.rs
│       ├── m004_posts_comments.rs
│       ├── m005_votes_follows.rs
│       └── m006_federation.rs
├── src/
│   ├── app.rs
│   ├── lib.rs
│   ├── controllers/
│   │   ├── auth.rs
│   │   ├── communities.rs
│   │   ├── posts.rs
│   │   ├── comments.rs
│   │   ├── users.rs
│   │   └── federation/
│   │       ├── mod.rs
│   │       ├── inbox.rs
│   │       ├── outbox.rs
│   │       └── webfinger.rs
│   ├── models/
│   │   ├── _entities/          ← SeaORM generated
│   │   ├── users.rs
│   │   ├── communities.rs
│   │   ├── posts.rs
│   │   ├── comments.rs
│   │   ├── votes.rs
│   │   ├── follows.rs
│   │   └── federation_objects.rs
│   ├── workers/
│   │   ├── deliver_activity.rs
│   │   └── fetch_remote.rs
│   ├── initializers/
│   │   ├── http_signature.rs
│   │   └── content_negotiation.rs
│   ├── views/
│   │   ├── layouts/
│   │   │   ├── base.html
│   │   │   └── sidebar.html
│   │   ├── communities/
│   │   │   ├── feed.html
│   │   │   ├── post.html
│   │   │   └── settings.html
│   │   ├── users/
│   │   │   ├── profile.html
│   │   │   └── settings.html
│   │   ├── auth/
│   │   │   ├── login.html
│   │   │   └── register.html
│   │   └── partials/
│   │       ├── post_card.html
│   │       ├── comment.html
│   │       └── vote_buttons.html
│   └── mailers/
└── assets/
    └── static/
        └── htmx.min.js
```

---

## Coding Standards

### Rust
- Use `async/await` everywhere. This is an async application.
- Prefer `anyhow::Error` for application-level errors, specific error types for library boundaries.
- Use `tracing::instrument` on all controller and worker functions.
- All database queries go through SeaORM — never write raw SQL strings in Rust code (migrations are the exception).
- Derive `Debug`, `Clone`, `Serialize`, `Deserialize` on all data structs.

### Database
- All migrations are reversible (implement both `up` and `down`).
- Always add indexes listed in SPEC.md — do not skip them.
- Use `UUID` primary keys with `uuid_generate_v4()` defaults.
- All timestamps are `TIMESTAMPTZ` (not `TIMESTAMP`).

### Templates (Tera)
- All user-supplied content must be escaped. Tera auto-escapes HTML by default — do not use `| safe` on user content.
- Rendered markdown (from pulldown-cmark + ammonia) can use `| safe` because it has been sanitized server-side.
- Keep template logic minimal — move business logic to Rust.

### ActivityPub
- Never hardcode URLs. Always build them from `config.base_url`.
- Always include the full JSON-LD `@context` array in AP responses.
- HTTP signatures are required on every outbound inbox POST — never skip them.
- Treat inbound activities idempotently — if you receive the same activity twice, the second one should be a no-op.

---

## Phase Execution Instructions

Each phase has its own file: `PHASE_1.md`, `PHASE_2.md`, etc.

**Before starting a phase:**
1. Read this file (AGENTS.md)
2. Read SPEC.md
3. Read your phase file completely before writing any code
4. Identify all files you will create or modify
5. Check that the previous phase's deliverables exist

**While working:**
- Complete tasks in the order listed in the phase file
- After each task, verify it compiles: `cargo check`
- After each task that adds a migration, verify it runs: `cargo loco db migrate`
- Do not proceed to the next task if the current one has compile errors

**When finished with a phase:**
- Run `cargo test` — all tests must pass
- Run `cargo clippy -- -D warnings` — no warnings allowed
- Run `cargo fmt` — code must be formatted
- Confirm every deliverable listed in the phase file exists

---

## How to Run the Project

```bash
# Initial setup
cargo install loco-cli sea-orm-cli
# Scaffold into repo root — see PHASE_1.md Task 1.1

# Database
createdb taverna_development
cargo loco db migrate

# Development server
cargo loco start

# Run tests
cargo test

# Format + lint
cargo fmt && cargo clippy -- -D warnings
```

---

## Environment Variables

The following must be set in development (put in `.env` or `config/development.yaml`):

```
DATABASE_URL=postgres://localhost/taverna_development
BASE_URL=http://localhost:3000
JWT_SECRET=<generate with: openssl rand -hex 32>
```

---

## Key Dependencies (Cargo.toml)

```toml
[dependencies]
loco-rs = { version = "0.14", features = ["with-db"] }
axum = { version = "0.7", features = ["macros"] }
sea-orm = { version = "1.1", features = ["sqlx-postgres", "runtime-tokio-rustls", "macros"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
rsa = "0.9"
sha2 = "0.10"
base64 = "0.22"
reqwest = { version = "0.12", features = ["json", "rustls-tls"] }
tera = "1"
pulldown-cmark = "0.11"
ammonia = "4"
url = "2"
argon2 = "0.5"
tower = "0.4"
tracing = "0.1"
anyhow = "1"

[dev-dependencies]
loco-rs = { version = "0.14", features = ["testing"] }
```
