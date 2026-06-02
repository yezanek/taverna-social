# Phase 1 — Project Scaffold & Database

**Goal:** A compiling Loco project with all database migrations applied and SeaORM entities generated. No web routes yet. No business logic yet. Just the foundation.

**Prerequisites:** Nothing. This is the first phase.

**Estimated complexity:** Low. This is setup work.

---

## Tasks

### Task 1.1 — Create the Loco project

The repo root (`taverna-social/`) is already initialised with the planning files.
Scaffold the Loco project directly into it:

```bash
cargo install loco-cli
# Run from the repo root:
loco new .
# When prompted: choose "Saas app" template, choose "postgres" database
```

If `loco new .` is not supported by your version of loco-cli, scaffold into a
temporary directory and move the files up:

```bash
cd /tmp && loco new taverna
# When prompted: choose "Saas app" template, choose "postgres" database
mv taverna/src taverna/config taverna/migration taverna/assets    taverna/Cargo.toml taverna/Cargo.lock /path/to/taverna-social/
rm -rf /tmp/taverna
```

This generates the initial project structure. You will heavily modify it.

### Task 1.2 — Update Cargo.toml

Replace the generated dependencies with the exact list from AGENTS.md. Do not add or remove dependencies without a reason from SPEC.md.

Add this to Cargo.toml:
```toml
[[bin]]
name = "taverna-cli"
path = "src/bin/main.rs"
```

### Task 1.3 — Write database migrations

Create these files in `migration/src/`. Each migration must implement `MigrationTrait` with both `up()` and `down()` methods.

**m001_extensions.rs** — Enable PostgreSQL extensions:
```sql
-- up
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS ltree;
-- Note: pgmq must be installed separately on the PostgreSQL server
-- up does NOT install pgmq (it requires superuser); document this requirement

-- down
-- Do not drop extensions (other things may use them)
```

**m002_users.rs** — Create users table. Use the exact schema from SPEC.md Section 4.2.

**m003_communities.rs** — Create communities table. Use the exact schema from SPEC.md Section 4.3.

**m004_posts_comments.rs** — Create posts and comments tables. Use the exact schema from SPEC.md Sections 4.4 and 4.5.

**m005_votes_follows.rs** — Create votes, community_members, and follows tables. Use the exact schema from SPEC.md Sections 4.6, 4.7, and 4.8.

**m006_federation.rs** — Create federation_objects and outbound_activities tables. Use the exact schema from SPEC.md Sections 4.9 and 4.10.

Register all migrations in `migration/src/lib.rs`.

### Task 1.4 — Configure development database

Update `config/development.toml` (Loco supports TOML natively — rename from `.yaml` if it was generated as such):
```toml
database:
  uri: postgres://localhost/taverna_development
  enable_logging: true
  min_connections: 1
  max_connections: 10

server:
  binding: localhost
  port: 3000

# Add custom config section
taverna:
  base_url: "http://localhost:3000"
  federation:
    enabled: false   # disabled in development by default
    blocked_domains: []
    http_signature_expiry_seconds: 30
```

### Task 1.5 — Run migrations and generate entities

```bash
# Create the database
createdb taverna_development

# Run migrations
cargo loco db migrate

# Generate SeaORM entities from the live database
sea-orm-cli generate entity \
  --database-url postgres://localhost/taverna_development \
  --output-dir src/models/_entities \
  --with-serde both \
  --date-time-crate chrono

# Verify: src/models/_entities/ should now contain one .rs file per table
```

### Task 1.6 — Verify compilation

```bash
cargo check
```

Fix any compilation errors before marking this phase complete.

### Task 1.7 — Write smoke tests

Create `tests/db_test.rs`:

```rust
#[cfg(test)]
mod tests {
    use loco_rs::testing;

    #[tokio::test]
    async fn test_db_connection() {
        // Verifies migrations ran and DB is accessible
        let _ctx = testing::boot_test::<app::App>().await.unwrap();
    }
}
```

Run: `cargo test`

---

## Deliverables Checklist

Before marking Phase 1 complete, verify ALL of these exist:

- [ ] `Cargo.toml` with all dependencies from AGENTS.md
- [ ] `migration/src/m001_extensions.rs` through `m006_federation.rs`
- [ ] `migration/src/lib.rs` registering all 6 migrations
- [ ] `config/development.toml` with database URI and taverna config section
- [ ] `src/models/_entities/` directory with generated SeaORM entity files
- [ ] `cargo check` passes with zero errors
- [ ] `cargo loco db migrate` runs without errors
- [ ] `cargo test` passes

---

## Common Mistakes to Avoid

- **DO NOT** use `TIMESTAMP` — always use `TIMESTAMPTZ`
- **DO NOT** skip the `ltree` extension — comments won't work without it
- **DO NOT** use `INTEGER` for primary keys — use `UUID`
- **DO NOT** forget to register new migrations in `migration/src/lib.rs`
- **DO NOT** hand-write SeaORM entities — generate them with `sea-orm-cli`
