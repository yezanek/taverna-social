# Taverna

A self-hostable, federated community platform where people gather in
communities to share posts and hold threaded discussions. Taverna is written
in Rust and joins the Fediverse through ActivityPub.

**Status:** pre-release (version 0.0.8). All eight build phases are merged;
expect breaking changes before 1.0.
**License:** AGPL-3.0-or-later (see [LICENSE](LICENSE)).

## Features

- **Communities** you can create, join and leave, with a description, a
  "moderators only" posting mode, and per-community bans that moderators
  manage from the community settings page.
- **Posts and threaded comments** in Markdown (sanitised), with optional
  link posts, voting, editing and deleting. Moderators can pin, lock and
  remove posts and remove comments.
- **User profiles** with their own posts, and follow / unfollow.
- **Three themes** (light, dark, sepia) kept in a cookie, no JavaScript
  framework. The UI is server-rendered Tera templates plus HTMX.
- **First-run setup wizard** at `/setup` that names the instance and creates
  the first admin.
- **Admin dashboard** at `/admin`: ban, suspend, unsuspend and force-verify
  users, delete communities, and edit instance settings (open
  registrations, email verification, community creation eligibility).
- **Email verification** through Loco's mailer, or logged to the console when
  no mailer is configured.
- **Federation** with Lemmy and Mastodon via
  [`activitypub_federation`](https://github.com/LemmyNet/activitypub-federation-rust):
  WebFinger, NodeInfo 2.0, user and community actors with inbox, outbox and
  followers collections, and Lemmy-style `Announce` from communities.
  Outbound deliveries are queued in PostgreSQL and retried with backoff.
- **Per-client rate limiting** that is safe behind a reverse proxy
  (`TRUSTED_PROXIES`), and CSRF protection on every form.

Text and links only: there are no media uploads in v1.

## Stack

| Layer | Technology |
|---|---|
| Web framework | [Loco](https://loco.rs) 1.2 (Axum 0.8) |
| ORM | SeaORM 2.0 |
| Database | PostgreSQL 16 with `uuid-ossp` and `ltree` |
| Templates | Tera 2 + HTMX |
| ActivityPub | `activitypub_federation` 0.7 |

PostgreSQL is the only external service. Background jobs (ActivityPub
delivery and remote cache expiry) run inside the server process.

## Getting started

### Prerequisites

- Rust stable (edition 2024, so Rust 1.85 or newer).
- PostgreSQL 16. The migrations create the `uuid-ossp` and `ltree`
  extensions, so the database user needs permission to do that (both ship
  with the standard `postgresql-contrib` packages). `pgmq` is **not** needed.

No extra CLI is required: `cargo loco` is an alias (in `.cargo/config.toml`)
for `cargo run --`, which runs this app's own `taverna-cli` binary.

### Run locally

```sh
# 1. Create the databases
createdb taverna_development
createdb taverna_test            # only needed for the test suite

# 2. Point Taverna at them (optional: these are the defaults)
cp .env.example .env             # loaded automatically at startup

# 3. Start the server. Development auto-migrates on boot.
cargo loco start
```

Open <http://localhost:3000>. On a fresh database every page redirects to
`/setup`, where you name the instance and create the admin account.

Other useful commands:

```sh
cargo loco db migrate     # apply migrations without starting the server
cargo loco db status      # list applied migrations
cargo loco routes         # print every route
cargo loco doctor         # check the config and the database connection
```

In development no mail is sent: verification links are written to the log.

### Tests and checks

The tests need a PostgreSQL test database (`config/test.yaml` reads
`TEST_DATABASE_URL`, default `postgres://localhost:5432/taverna_test`) and
truncate it on every run.

```sh
export TEST_DATABASE_URL=postgres://taverna:taverna@localhost:5432/taverna_test
cargo test --all-features
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

The test build is large. If your disk is small, build with
`CARGO_PROFILE_DEV_DEBUG=line-tables-only CARGO_INCREMENTAL=0`, as CI does.

CI (`.github/workflows/ci.yml`) runs format, clippy and the tests against
PostgreSQL 16. `pr-checks.yml` requires a conventional-commit PR title whose
subject starts lowercase (for example `docs: update the readme`), runs a
security audit, and scans the code for `unwrap()`, `todo!`, raw `reqwest` in
federation code and hardcoded template colours. `dependencies.yml` runs
weekly: outdated crates, `cargo audit` and the `cargo deny` licence check.

## Documentation

- [docs/CONFIGURATION.md](docs/CONFIGURATION.md): config files, environment
  variables, the `settings:` block and the admin settings.
- [DEPLOY.md](DEPLOY.md): running Taverna in production behind Caddy.
- [docs/DESIGN.md](docs/DESIGN.md): architecture and design.
- [docs/ai/SPEC.md](docs/ai/SPEC.md): the functional specification.
- [docs/ai/DECISIONS.md](docs/ai/DECISIONS.md): architecture decision records.
- [AGENTS.md](AGENTS.md): coding rules and branch/PR naming for contributors and AI tools.
- [docs/ai/INSTRUCTIONS.md](docs/ai/INSTRUCTIONS.md): how to work on Taverna with AI
  coding agents; [docs/ai/REVIEW.md](docs/ai/REVIEW.md) has the review prompts.

## Project layout

```
config/          development.yaml, test.yaml, production.example.yaml
migration/       SeaORM migrations m001..m010
src/app.rs       routes, middleware stack, workers
src/common/      settings (config.rs), JWT, Markdown
src/controllers/ web handlers, federation/ for ActivityPub endpoints
src/federation/  ActivityPub objects, activities and outbound delivery
src/initializers/ view engine, rate limiter, background jobs, first-run guard
src/models/      SeaORM models (_entities/ is generated)
src/workers/     delivery and remote cache expiry
assets/views/    Tera templates
assets/static/   CSS, HTMX, error page
tests/           integration tests
```
