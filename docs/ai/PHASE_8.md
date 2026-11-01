# Phase 8 — Performance, Scalability & Federated Content Isolation

> **Status:** complete. This file is the plan this phase was built from. It is kept as a
> record of why the code looks the way it does, and as a template for planning similar work.
> Where it disagrees with the code, `SPEC.md`, `DECISIONS.md` or [DESIGN.md](../DESIGN.md), those win.
>
> **Changed since:** Config files are YAML (`config/development.yaml`, `config/production.example.yaml`), not TOML. Delivery is dispatched by an in-process poller (ADR-021).

**Goal:** Address confirmed performance bottlenecks observed during testing, ensure
the application can handle several thousand concurrent users (comparable to a
mid-sized Lemmy instance), and restructure the database so that cached content
from remote federated servers lives in dedicated tables that can be expired
independently of local content.

**Prerequisites:** Phases 1–7 complete.

---

## Background: What Was Observed

- Some test suites take minutes to complete — indicating either missing indexes,
  synchronous federation delivery blocking request threads, or test isolation
  issues causing sequential rather than parallel execution
- The `posts`, `comments`, and `users` tables currently mix local and federated
  content, making it impossible to expire remote cached content without
  risking local data

---

## Part A — Federated Content Isolation

### Design Principle

Local content (authored on this instance) must never be at risk when managing
remote cached content. Remote content has a different lifecycle: it can be
re-fetched on demand, it may go stale, and it should be purgeable without
affecting anything local.

### New Tables (migration m009_federated_cache.rs)

```sql
-- Remote users (actors fetched from other instances)
-- Local users remain in the existing `users` table (is_local = true)
CREATE TABLE remote_actors (
  ap_id             TEXT PRIMARY KEY,
  actor_type        TEXT NOT NULL CHECK (actor_type IN ('Person', 'Group', 'Service')),
  username          TEXT NOT NULL,
  display_name      TEXT,
  bio               TEXT,
  avatar_url        TEXT,
  public_key        TEXT NOT NULL DEFAULT '',
  inbox_url         TEXT NOT NULL DEFAULT '',
  shared_inbox_url  TEXT,
  followers_url     TEXT NOT NULL,
  instance_domain   TEXT NOT NULL,           -- extracted from ap_id for easy filtering
  raw_json          JSONB NOT NULL,           -- full AP actor JSON for re-serving
  fetched_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  stale_after       TIMESTAMPTZ NOT NULL DEFAULT NOW() + INTERVAL '1 day',
  created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_remote_actors_domain   ON remote_actors(instance_domain);
CREATE INDEX idx_remote_actors_stale    ON remote_actors(stale_after);
CREATE INDEX idx_remote_actors_username ON remote_actors(username);

-- Remote posts (Notes received from other instances via federation)
CREATE TABLE remote_posts (
  ap_id             TEXT PRIMARY KEY,
  community_id      UUID REFERENCES communities(id) ON DELETE CASCADE,
  author_ap_id      TEXT NOT NULL REFERENCES remote_actors(ap_id) ON DELETE CASCADE,
  title             TEXT,
  body              TEXT,
  url               TEXT,
  score             INT NOT NULL DEFAULT 0,
  comment_count     INT NOT NULL DEFAULT 0,
  instance_domain   TEXT NOT NULL,
  raw_json          JSONB NOT NULL,
  published_at      TIMESTAMPTZ NOT NULL,
  fetched_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  stale_after       TIMESTAMPTZ NOT NULL DEFAULT NOW() + INTERVAL '7 days',
  is_deleted        BOOL NOT NULL DEFAULT false,
  is_removed        BOOL NOT NULL DEFAULT false   -- mod action on this instance
);
CREATE INDEX idx_remote_posts_community  ON remote_posts(community_id);
CREATE INDEX idx_remote_posts_author     ON remote_posts(author_ap_id);
CREATE INDEX idx_remote_posts_domain     ON remote_posts(instance_domain);
CREATE INDEX idx_remote_posts_stale      ON remote_posts(stale_after);
CREATE INDEX idx_remote_posts_published  ON remote_posts(published_at DESC);
CREATE INDEX idx_remote_posts_score      ON remote_posts(score DESC);

-- Remote comments (Notes that are replies, received from other instances)
CREATE TABLE remote_comments (
  ap_id             TEXT PRIMARY KEY,
  post_ap_id        TEXT,                         -- may reference remote_posts or local posts.ap_id
  local_post_id     UUID REFERENCES posts(id) ON DELETE CASCADE,  -- set if replying to local post
  parent_ap_id      TEXT,
  author_ap_id      TEXT NOT NULL REFERENCES remote_actors(ap_id) ON DELETE CASCADE,
  body              TEXT NOT NULL,
  score             INT NOT NULL DEFAULT 0,
  instance_domain   TEXT NOT NULL,
  raw_json          JSONB NOT NULL,
  published_at      TIMESTAMPTZ NOT NULL,
  fetched_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  stale_after       TIMESTAMPTZ NOT NULL DEFAULT NOW() + INTERVAL '7 days',
  is_deleted        BOOL NOT NULL DEFAULT false,
  is_removed        BOOL NOT NULL DEFAULT false
);
CREATE INDEX idx_remote_comments_post     ON remote_comments(local_post_id);
CREATE INDEX idx_remote_comments_author   ON remote_comments(author_ap_id);
CREATE INDEX idx_remote_comments_stale    ON remote_comments(stale_after);
CREATE INDEX idx_remote_comments_post_ap  ON remote_comments(post_ap_id);
```

### Expiry

Add a background worker `src/workers/expire_remote_cache.rs` that runs on a
schedule (daily, via a pgmq recurring job):

```rust
pub async fn expire_remote_cache(db: &DatabaseConnection) -> Result<()> {
    let cutoff = Utc::now();

    // Hard-delete remote comments older than stale_after
    RemoteComments::delete_many()
        .filter(remote_comments::Column::StaleAfter.lt(cutoff))
        .exec(db).await?;

    // Hard-delete remote posts older than stale_after
    // (comments cascade-deleted by FK)
    RemotePosts::delete_many()
        .filter(remote_posts::Column::StaleAfter.lt(cutoff))
        .exec(db).await?;

    // Hard-delete remote actors with no remaining posts or comments
    // (actors shared across posts, only delete if nothing references them)
    sqlx::query(
        "DELETE FROM remote_actors ra
         WHERE ra.stale_after < NOW()
         AND NOT EXISTS (SELECT 1 FROM remote_posts rp WHERE rp.author_ap_id = ra.ap_id)
         AND NOT EXISTS (SELECT 1 FROM remote_comments rc WHERE rc.author_ap_id = ra.ap_id)"
    ).execute(db).await?;

    Ok(())
}
```

### Migration of existing data

The existing `users` table keeps local users only. Any rows where `is_local = false`
should be migrated to `remote_actors`. The existing `federation_objects` table
(generic JSON cache from Phase 4) can be dropped once `remote_actors` is in place —
`remote_actors` is the typed replacement.

```sql
-- In m009 migration up():

-- Migrate remote users to remote_actors
INSERT INTO remote_actors (ap_id, actor_type, username, display_name, bio,
  public_key, inbox_url, shared_inbox_url, followers_url, instance_domain,
  raw_json, fetched_at)
SELECT
  ap_id, 'Person', username, display_name, bio,
  public_key, inbox_url, shared_inbox_url, followers_url,
  split_part(ap_id, '/', 3),          -- extract domain from https://domain/...
  '{}'::jsonb,                         -- raw_json not previously stored per-actor
  COALESCE(last_fetched_at, NOW())
FROM users
WHERE is_local = false;

-- Remove remote users from the users table
DELETE FROM users WHERE is_local = false;

-- Drop the old generic cache table
DROP TABLE IF EXISTS federation_objects;
```

### Code changes required

**Federation inbox handlers**: when `Create{Note}` arrives from a remote user:
- Look up the author in `remote_actors` (not `users`)
- If not found, fetch and insert into `remote_actors`
- Insert the post into `remote_posts` (not `posts`)

**Feed queries**: community feeds must UNION or JOIN local and remote content.
Use a view for this to keep handler code clean:

```sql
CREATE VIEW community_feed AS
  SELECT
    p.id::text AS id, p.ap_id, p.community_id, p.author_id::text AS author_ap_id,
    p.title, p.body, p.url, p.score, p.comment_count,
    p.published_at, p.is_deleted, p.is_removed, p.is_locked, p.is_pinned,
    'local' AS origin
  FROM posts p
  WHERE p.is_deleted = false AND p.is_removed = false

  UNION ALL

  SELECT
    md5(rp.ap_id) AS id,            -- stable UUID-like key for HTMX targets
    rp.ap_id, rp.community_id, rp.author_ap_id,
    rp.title, rp.body, rp.url, rp.score, rp.comment_count,
    rp.published_at, rp.is_deleted, rp.is_removed,
    false AS is_locked, false AS is_pinned,
    'remote' AS origin
  FROM remote_posts rp
  WHERE rp.is_deleted = false AND rp.is_removed = false;
```

---

## Part B — Performance Bottlenecks

### B1 — Missing indexes

Add these indexes that are likely missing and cause slow queries at scale:

```sql
-- m010_performance_indexes.rs

-- Feed sorting: most common query pattern
CREATE INDEX IF NOT EXISTS idx_posts_community_score
  ON posts(community_id, score DESC, published_at DESC)
  WHERE is_deleted = false AND is_removed = false;

CREATE INDEX IF NOT EXISTS idx_remote_posts_community_score
  ON remote_posts(community_id, score DESC, published_at DESC)
  WHERE is_deleted = false AND is_removed = false;

-- Home feed (all communities)
CREATE INDEX IF NOT EXISTS idx_posts_score_published
  ON posts(score DESC, published_at DESC)
  WHERE is_deleted = false AND is_removed = false;

-- Comment threading: path queries
-- (already added in Phase 1 as GIST, but add btree for equality lookups)
CREATE INDEX IF NOT EXISTS idx_comments_post_path
  ON comments(post_id, path);

-- Votes: checking user's existing vote before cast
CREATE INDEX IF NOT EXISTS idx_votes_user_post
  ON votes(user_id, post_id) WHERE post_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_votes_user_comment
  ON votes(user_id, comment_id) WHERE comment_id IS NOT NULL;

-- Follows: fetching follower inboxes for fan-out (most critical federation query)
CREATE INDEX IF NOT EXISTS idx_follows_community_accepted
  ON follows(community_id, accepted)
  WHERE accepted = true AND community_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_follows_user_accepted
  ON follows(following_id, accepted)
  WHERE accepted = true AND following_id IS NOT NULL;

-- outbound_activities: worker polling query
-- (already exists from Phase 1 but confirm it covers the full WHERE clause)
CREATE INDEX IF NOT EXISTS idx_outbound_state_next
  ON outbound_activities(state, next_attempt_at)
  WHERE state = 'pending';

-- Remote actors: re-fetch scheduling
CREATE INDEX IF NOT EXISTS idx_remote_actors_stale_domain
  ON remote_actors(stale_after, instance_domain);
```

### B2 — Federation delivery must be fully async

**Problem:** If `SendActivityTask::send` is awaited directly in a request
handler, it blocks the response until all remote inboxes respond. For a community
with 1,000 followers across 200 instances, this could take 30+ seconds.

**Fix:** Never call `SendActivityTask::send` directly in a web request handler.
Always enqueue to the `outbound_activities` table and let the background worker
deliver. The request handler returns immediately after enqueuing.

```rust
// WRONG — blocks the request:
async fn create_post(/* ... */) {
    let post = PostModel::create(/* ... */).await?;
    SendActivityTask::send(&activity, &actor, inboxes, &data).await?; // blocks!
    Ok(redirect("/c/slug"))
}

// CORRECT — returns immediately:
async fn create_post(/* ... */) {
    let post = PostModel::create(/* ... */).await?;
    enqueue_activity(&data.db, &activity, &actor.ap_id, inboxes).await?; // fast
    Ok(redirect("/c/slug"))
}
```

The `deliver_activity` worker picks up from `outbound_activities` and calls
`SendActivityTask::send` with per-inbox retry logic.

### B3 — Connection pool sizing

The default Loco connection pool is too small for concurrent load.

In `config/production.toml`:

```toml
[database]
min_connections = 10
max_connections = 100
connect_timeout_seconds = 5
idle_timeout_seconds = 600
max_lifetime_seconds = 1800
```

For development (`config/development.toml`):
```toml
[database]
min_connections = 2
max_connections = 20
```

### B4 — Pagination must use keyset, not OFFSET

**Problem:** `LIMIT N OFFSET M` gets slower as M grows — at page 100 with 25
posts per page, PostgreSQL scans and discards 2,500 rows before returning 25.
For a busy community this is measurable.

**Fix:** Use keyset (cursor-based) pagination for all feed queries.

```rust
// Instead of:
//   WHERE community_id = $1 ORDER BY score DESC, published_at DESC
//   LIMIT 25 OFFSET (page * 25)

// Use:
//   WHERE community_id = $1
//     AND (score, published_at) < ($last_score, $last_published_at)
//   ORDER BY score DESC, published_at DESC
//   LIMIT 25
```

The cursor is a tuple of `(score, published_at)` encoded as a base64 string
passed as a query parameter (`?cursor=...`). The first page has no cursor.
The last item's score and published_at are returned in the response for the
next page's cursor.

Apply keyset pagination to:
- Community feed (`GET /c/:slug`)
- Home feed (`GET /`)
- User post history (`GET /u/:username/posts`)
- Admin user list and community list

### B5 — Test isolation and parallelism

**Problem:** If tests share a single database and run sequentially, the suite
takes minutes. Each test should get a clean slate.

**Fix:** Use Loco's test database isolation. Each test boots a fresh transaction
that is rolled back after the test, or uses a per-test database schema.

```rust
// In each test file, use the Loco testing helpers:
use loco_rs::testing;

#[tokio::test]
async fn test_something() {
    let boot = testing::boot_test::<App>().await.unwrap();
    // boot.db is a connection to the test database
    // Each test should use testing::clean_db or transaction rollback
}
```

Also ensure `DATABASE_URL` in CI points to a test-only database
(`taverna_test`, not `taverna_development`).

For tests that don't need the database at all (pure unit tests on model
logic, eligibility checks, ltree path construction), avoid booting the full
app — test the function directly.

### B6 — HTTP keep-alive and compression for federation

For the web server in production, ensure these are enabled in `config/production.toml`:

```toml
[server]
binding = "0.0.0.0"
port = 3000

[server.middlewares]
compression = true      # gzip/brotli for HTML and JSON responses
```

Reverse proxy (Caddy) should handle TLS termination and HTTP/2, which gives
multiplexing for free. Ensure Caddy is configured with:

```
taverna.example.com {
    reverse_proxy localhost:3000 {
        header_up X-Forwarded-For {remote_host}
        header_up X-Real-IP {remote_host}
    }
    encode gzip zstd
}
```

### B7 — Shared inbox delivery

When delivering to followers on the same instance as each other, Lemmy and
Mastodon support a `sharedInbox` URL. Instead of sending N copies to N users
on instance X, send one copy to instance X's shared inbox.

The `remote_actors` table has a `shared_inbox_url` column. When fanning out:

```rust
pub async fn get_follower_inboxes_deduplicated(
    db: &DatabaseConnection,
    community_id: Uuid,
) -> Result<Vec<Url>> {
    // For each follower, prefer shared_inbox_url over inbox_url
    // Then deduplicate — one request per unique inbox URL
    let followers = /* query remote_actors via follows table */;
    let mut seen = HashSet::new();
    let inboxes = followers.into_iter()
        .map(|f| f.shared_inbox_url.unwrap_or(f.inbox_url))
        .filter(|url| seen.insert(url.clone()))
        .map(|url| Url::parse(&url))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(inboxes)
}
```

This can reduce outbound requests by 10–50x for popular communities.

---

## Tasks

### Task 8.1 — Write migration m009_federated_cache.rs
Create the `remote_actors`, `remote_posts`, `remote_comments` tables.
Migrate existing remote users from `users` to `remote_actors`.
Drop `federation_objects`.

### Task 8.2 — Write migration m010_performance_indexes.rs
Add all indexes from Part B1.

### Task 8.3 — Update federation inbox handlers
Incoming remote content (Create{Note}, Update{Note}, Delete, etc.) must now
write to `remote_posts` and `remote_comments`, not `posts` and `comments`.
Update all `ActivityHandler::receive` implementations.

### Task 8.4 — Update Object trait implementations
`DbUser` (local only) and new `RemoteActor` type implement `Object` and `Actor`
differently. Remote actors use `remote_actors` table. Update federation type
files accordingly.

### Task 8.5 — Create community_feed view
Create the `community_feed` SQL view as specified in Part A.
Update feed query in `CommunityModel` and home feed handler to query this view.

### Task 8.6 — Make federation delivery fully async
Audit all handlers. Any `SendActivityTask::send` called directly in a handler
must be replaced with `enqueue_activity`. Only the background worker calls
`SendActivityTask::send`.

### Task 8.7 — Keyset pagination
Implement cursor-based pagination for all feed and list endpoints.
Define a `Cursor` struct with base64 encode/decode.
Update templates to pass the cursor as a query parameter in the "load more" link.

### Task 8.8 — Shared inbox deduplication
Implement `get_follower_inboxes_deduplicated` as described in Part B7.
Use it everywhere follower inboxes are queried for fan-out.

### Task 8.9 — Expire remote cache worker
Implement `src/workers/expire_remote_cache.rs`.
Register it to run daily.

### Task 8.10 — Update connection pool config
Update `config/development.toml` and `config/production.toml` with pool sizing
from Part B3.

### Task 8.11 — Tests

**tests/federation_test.rs** — add:
```rust
test_incoming_create_writes_to_remote_posts()
  // Create{Note} from remote actor must insert into remote_posts, not posts
test_remote_actor_upserted_on_receive()
  // Receiving activity from unknown remote actor creates remote_actors row
test_expire_remote_cache_removes_stale()
  // After expiry worker runs, remote_posts with stale_after in past are deleted
  // Local posts are untouched
test_shared_inbox_deduplication()
  // get_follower_inboxes_deduplicated returns one URL per instance, not per user
```

**tests/post_test.rs** — add:
```rust
test_community_feed_includes_remote_posts()
  // GET /c/:slug feed must include both local and remote posts
test_keyset_pagination_first_page()
  // GET /c/:slug returns first 25 posts, includes cursor for next page
test_keyset_pagination_second_page()
  // GET /c/:slug?cursor=... returns next 25 posts, different from first page
```

**tests/performance_test.rs** — add (these are integration benchmarks, not unit tests):
```rust
// Use criterion or just assert timing bounds
test_feed_query_under_100ms_with_1000_posts()
  // Insert 1000 posts, measure feed query time, assert < 100ms
test_follower_inbox_query_under_50ms_with_10000_followers()
  // Insert 10000 follows, measure inbox query time, assert < 50ms
```

---

## Deliverables Checklist

- [x] `m009_federated_cache.rs` — remote_actors, remote_posts, remote_comments tables
- [x] `m010_performance_indexes.rs` — all indexes from Part B1
- [x] Remote users migrated from `users` to `remote_actors` in m009 up()
- [x] `federation_objects` table dropped in m009 up()
- [x] All ActivityHandler::receive implementations write to remote_* tables
- [x] `community_feed` SQL view created and used in feed handlers
- [x] No `SendActivityTask::send` called directly in any web handler
- [x] Keyset pagination implemented for all feed and list endpoints
- [x] `get_follower_inboxes_deduplicated` implemented and used for fan-out
- [x] `expire_remote_cache` worker registered and scheduled daily
- [x] Connection pool config updated in both development.toml and production.toml
- [x] All federation tests updated to reference remote_* tables
- [x] New tests added to semantic test files (not a phase8_test.rs)
- [x] `cargo check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt` pass
- [x] `cargo test` passes

---

## Common Mistakes

- DO NOT delete local posts when running the expiry worker — only rows in
  `remote_posts`, `remote_comments`, `remote_actors` are eligible for expiry
- DO NOT use OFFSET-based pagination anywhere — use keyset cursors throughout
- DO NOT call SendActivityTask::send in request handlers — enqueue only
- DO NOT store remote actors in the `users` table after this migration —
  the users table is local-only from this point on
- DO NOT forget to update the follows table foreign keys — `following_id`
  references `users.id` for local follows; remote actor follows reference
  `remote_actors.ap_id` (text). You may need a separate `remote_follows` table
  or a nullable `remote_actor_ap_id TEXT` column on `follows` alongside the
  existing `following_id UUID` column
- DO NOT forget the cascade: deleting a remote_actor must cascade to their
  remote_posts and remote_comments

---

## Prompts

Prompts for redoing or extending this phase with an AI coding agent. They work with any
tool; see [INSTRUCTIONS.md](INSTRUCTIONS.md) for the workflow.

### Implement

```
Read AGENTS.md and docs/ai/PHASE_8.md completely before writing any code.

Phases 1–7 are complete.

This phase has two parts: (A) splitting federated content into dedicated tables,
and (B) fixing performance bottlenecks. Show me a plan before starting any task.

PART A — Federated content isolation (tasks 8.1–8.5, 8.8, 8.9)

Task 8.1 — Migration m009
  Create remote_actors, remote_posts, remote_comments tables exactly as specified.
  Migrate remote users from users table to remote_actors.
  Drop federation_objects table.
  Show me the full migration SQL before running it.

Task 8.2 — Migration m010
  Add all performance indexes from Part B1 of PHASE_8.md.

Task 8.3 — Update ActivityHandler implementations
  All incoming remote content writes to remote_posts / remote_comments.
  Verify no remote content is inserted into the local posts or comments tables.

Task 8.4 — Update Object/Actor trait implementations
  Local users → users table. Remote actors → remote_actors table.
  Two separate Rust types: DbUser (local) and RemoteActor (remote).

Task 8.5 — community_feed SQL view
  Create the UNION ALL view as specified. Update feed handlers to use it.

Task 8.8 — Shared inbox deduplication
  Implement get_follower_inboxes_deduplicated.
  Replace all direct inbox-list queries in fan-out with this function.

Task 8.9 — Expire remote cache worker
  Daily scheduled worker. Deletes from remote_comments, remote_posts,
  remote_actors where stale_after < NOW() and nothing references the actor.
  MUST NOT touch any rows in the local posts, comments, or users tables.

PART B — Performance fixes (tasks 8.6, 8.7, 8.10)

Task 8.6 — Async federation delivery
  Audit every handler. Any SendActivityTask::send in a handler body is wrong.
  Replace with enqueue_activity. Only the background worker calls SendActivityTask::send.

Task 8.7 — Keyset pagination
  Replace all OFFSET-based pagination with cursor-based (score, published_at) tuples.
  Cursor is base64-encoded and passed as ?cursor= query parameter.
  Apply to: community feed, home feed, user post history, admin lists.

Task 8.10 — Connection pool config
  Update config/development.yaml and config/production.yaml as specified.

Task 8.11 — Tests
  Add tests to semantic files. DO NOT create tests/phase8_test.rs.
  federation_test.rs, post_test.rs, performance_test.rs as detailed in PHASE_8.md.

After all tasks:
  cargo fmt && cargo clippy --all-targets --all-features -- -D warnings && cargo test
  Verify all deliverables in PHASE_8.md checklist.
```

### Review

```
You are a QA engineer reviewing Phase 8 of the Taverna project.

Read AGENTS.md and docs/ai/PHASE_8.md.

PART A — Federated content isolation

A1. remote_* tables exist
    Run: \dt in psql against the database.
    Must find: remote_actors, remote_posts, remote_comments.
    Must NOT find: federation_objects (should be dropped).

A2. users table is local-only
    Run: SELECT COUNT(*) FROM users WHERE is_local = false;
    Must return 0. If any remote users remain in users: CRITICAL.

A3. Incoming federation writes to remote tables
    Open all ActivityHandler::receive implementations.
    Create{Note} from a remote actor must INSERT into remote_posts, not posts.
    If any remote content is inserted into posts or comments: CRITICAL.

A4. community_feed view exists and is correct
    Run: \dv in psql. Must find community_feed.
    Run: SELECT origin, COUNT(*) FROM community_feed GROUP BY origin;
    Must show both 'local' and 'remote' rows (if remote posts exist).

A5. Expiry worker does not touch local content
    Review src/workers/expire_remote_cache.rs.
    DELETE statements must only reference remote_posts, remote_comments,
    remote_actors — never posts, comments, or users.
    If any local table is touched: CRITICAL.

A6. Shared inbox deduplication
    Open get_follower_inboxes_deduplicated.
    Must prefer shared_inbox_url over inbox_url when non-null.
    Must deduplicate — HashSet or equivalent.
    If it returns one URL per follower without deduplication: MEDIUM.

PART B — Performance

B1. No SendActivityTask::send in handlers
    grep -rn "SendActivityTask::send" src/controllers/ src/models/
    Any match is CRITICAL. Only src/workers/ may call SendActivityTask::send.

B2. No OFFSET pagination
    grep -rn "offset\|OFFSET\|\.paginate(" src/models/ src/controllers/
    Any paginate() or OFFSET in feed queries: CRITICAL.
    Cursor-based pagination must be used for all feeds.

B3. Connection pool config
    Open config/production.yaml.
    max_connections must be >= 50.
    min_connections must be >= 10.
    If defaults (typically 5/1) are unchanged: MEDIUM.

B4. Performance indexes exist
    Run in psql:
      SELECT indexname FROM pg_indexes
      WHERE tablename IN ('posts','remote_posts','votes','follows','comments')
      ORDER BY tablename, indexname;
    Check for:
      idx_posts_community_score (partial: not deleted/removed)
      idx_votes_user_post (partial: post_id not null)
      idx_follows_community_accepted (partial: accepted=true)
    Missing any: MEDIUM.

B5. Test file naming
    ls tests/
    Must NOT contain phase8_test.rs.
    New tests must be in federation_test.rs, post_test.rs, performance_test.rs.
    If phase8_test.rs exists: MEDIUM — reorganise into semantic files.

Run: cargo test
Report any failures as CRITICAL.

Run: cargo clippy --all-targets --all-features -- -D warnings
Report any warnings as MEDIUM.

Report CRITICAL / MEDIUM / LOW.
```
