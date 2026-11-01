# Taverna — Agent Instructions

**Location:** Repo root. Read by AI coding tools and human contributors alike.

**Read this file first. Read SPEC.md second. Then read your phase file.**

All other planning files (`SPEC.md`, `PHASE_*.md`, `DECISIONS.md`, etc.) are in `docs/ai/`.

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
taverna-social/
├── Cargo.toml                 ← package `taverna`, binary `taverna-cli`
├── .env.example               ← copy to .env; loaded at startup
├── config/
│   ├── development.yaml
│   ├── test.yaml
│   └── production.example.yaml ← copy to production.yaml (git-ignored)
├── migration/src/             ← m001_extensions .. m010_performance_indexes
├── src/
│   ├── app.rs                 ← routes, middleware stack, workers
│   ├── bin/main.rs
│   ├── common/                ← config.rs (TavernaConfig), jwt, markdown
│   ├── controllers/           ← admin, auth, communities, posts, users,
│   │   │                        setup, verify, theme, csrf, errors, home
│   │   └── federation/        ← actors, inbox, notes, nodeinfo, webfinger
│   ├── federation/            ← activities, protocol, traits, outbound
│   ├── initializers/          ← view_engine, rate_limiter, federation_jobs,
│   │                            first_run, auth/theme/content negotiation
│   ├── mailers/               ← verification email
│   ├── models/                ← one file per table; _entities/ generated
│   ├── views/                 ← the `Views` render extractor
│   └── workers/               ← deliver_activity, expire_remote_cache
├── assets/
│   ├── views/                 ← Tera templates (layouts, partials, pages)
│   └── static/                ← theme.css, htmx.min.js, 404.html
├── tests/                     ← integration tests, one file per area
└── docs/
    ├── CONFIGURATION.md
    └── ai/                    ← SPEC, DECISIONS, PHASE_* plans
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

See [README.md](README.md) for the full guide and
[docs/CONFIGURATION.md](docs/CONFIGURATION.md) for every setting.

```bash
# Database (PostgreSQL 16; migrations create uuid-ossp and ltree)
createdb taverna_development
createdb taverna_test

# Development server on http://localhost:3000 (auto-migrates)
cargo loco start

# Migrations only
cargo loco db migrate

# Tests (need TEST_DATABASE_URL or the default local taverna_test database)
cargo test --all-features

# Format + lint, as CI runs them
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

`cargo loco` is an alias for `cargo run --` (see `.cargo/config.toml`); no
separate `loco-cli` install is needed.

---

## Environment Variables

All are optional in development (defaults in `config/development.yaml`).
Put them in `.env` (see `.env.example`) or the environment:

```
DATABASE_URL=postgres://localhost:5432/taverna_development
TEST_DATABASE_URL=postgres://localhost:5432/taverna_test
BASE_URL=http://localhost:3000
TAVERNA_DOMAIN=localhost:3000
JWT_SECRET=<generate with: openssl rand -hex 32>
FEDERATION_ENABLED=false
RATE_LIMITING_ENABLED=true
TRUSTED_PROXIES=
```

Taverna's settings are read from the `settings:` block of the config file
into `TavernaConfig` (`src/common/config.rs`) and kept in `ctx.shared_store`.
Add new settings there, not as direct `std::env::var` reads. Config YAML
uses `<%= get_env(...) %>`, not `{{ }}`.

---

## Key Dependencies (Cargo.toml)

Versions as of 0.0.8; `Cargo.toml` is the source of truth.

| Crate | Version | Notes |
|---|---|---|
| `loco-rs` | 1.2 | `with-db`; `testing` in dev-dependencies |
| `axum` | 0.8 | |
| `sea-orm` | 2.0 | `sqlx-postgres`, `runtime-tokio-rustls` |
| `tera` | 2 | |
| `activitypub_federation` | 0.7.0-beta | `axum` feature; does all HTTP signatures |
| `reqwest` | 0.13 | not to be used directly in federation code |
| `jsonwebtoken` | 11 | `rust_crypto` |
| `argon2` | 0.6 | password hashing |
| `pulldown-cmark` / `ammonia` | 0.13 / 4 | Markdown + sanitising |
| `governor` / `ipnet` | 0.10 / 2 | per-client rate limiting |
| `axum-test` (dev) | 17.x | keep on 17: Loco 1.2's testing is built on it |
