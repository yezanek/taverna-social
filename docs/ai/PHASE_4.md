# Phase 4 — ActivityPub Federation

> **Status:** complete. This file is the plan this phase was built from. It is kept as a
> record of why the code looks the way it does, and as a template for planning similar work.
> Where it disagrees with the code, `SPEC.md`, `DECISIONS.md` or [DESIGN.md](../DESIGN.md), those win.
>
> **Changed since:** HTTP signatures, actor fetching and WebFinger are done by the `activitypub_federation` crate (0.7), not by a hand-written `src/federation/signatures.rs`; see SPEC.md §6. Outbound activities are built in `federation::outbound` (ADR-023) and only enqueued by handlers (ADR-019).

**Goal:** Full ActivityPub federation. Lemmy instances can subscribe to Taverna communities. Mastodon users can follow Taverna profiles. Outbound activities are delivered via the background worker.

**Prerequisites:** Phase 3 complete. All local features working. `cargo test` passes.

**Warning:** This is the most complex phase. Read SPEC.md Section 6 in full before writing any code.

---

## Tasks

### Task 4.1 — HTTP Signature implementation (src/federation/signatures.rs)

This is the foundation of everything. Get it right before moving on.

```rust
pub struct HttpSigner {
    private_key_pem: String,
    key_id: String,           // e.g. "https://host/u/alice#main-key"
}

impl HttpSigner {
    pub fn new(private_key_pem: &str, key_id: &str) -> Result<Self>;

    /// Sign a POST request. Returns the Signature header value.
    /// Signs: (request-target), host, date, digest
    pub fn sign_request(
        &self,
        method: &str,
        url: &Url,
        body: &[u8],
    ) -> Result<SignedHeaders>;
}

pub struct SignedHeaders {
    pub date: String,
    pub digest: String,       // "SHA-256={base64}"
    pub signature: String,    // full Signature header value
}

pub fn verify_request(
    headers: &HeaderMap,
    method: &str,
    url: &Url,
    body: &[u8],
    public_key_pem: &str,
) -> Result<()>;
```

**Signing algorithm:**
1. Generate RFC 7231 date string (`Tue, 07 May 2026 12:00:00 GMT`)
2. Compute SHA-256 of body bytes
3. Base64-encode the digest → `digest = format!("SHA-256={}", base64_digest)`
4. Build the signing string:
   ```
   (request-target): post /u/alice/inbox
   host: remote.example.com
   date: {date}
   digest: SHA-256={base64_digest}
   ```
5. Sign the signing string with RSA-SHA256 using the private key
6. Base64-encode the signature
7. Build the Signature header:
   ```
   keyId="{key_id}",algorithm="rsa-sha256",headers="(request-target) host date digest",signature="{sig}"
   ```

**Verification algorithm:**
1. Parse Signature header, extract `keyId`, `headers`, `signature`
2. Reconstruct signing string from the listed headers in order
3. Verify RSA-SHA256 signature using public key
4. Check Date header: if more than 30 seconds from now, reject
5. Verify Digest header matches body

Write unit tests for sign + verify round-trip before proceeding.

### Task 4.2 — HTTP Signature middleware (src/initializers/http_signature.rs)

Axum middleware layer that:
1. For routes matching `/u/*/inbox` or `/c/*/inbox`:
   a. Read and buffer the request body (needed for digest verification)
   b. Extract actor key_id from Signature header
   c. Fetch actor's public key (from `federation_objects` cache or via HTTP)
   d. Call `verify_request`
   e. If invalid: return `401 Unauthorized`
   f. Re-inject buffered body into request for the handler
2. For all other routes: pass through unchanged

### Task 4.3 — Content negotiation middleware (src/initializers/content_negotiation.rs)

Axum middleware that:
1. Checks `Accept` header
2. If it contains `application/activity+json` OR `application/ld+json`:
   → Set request extension: `RenderMode::ActivityPub`
3. Otherwise:
   → Set request extension: `RenderMode::Html`

```rust
#[derive(Clone, Debug)]
pub enum RenderMode {
    Html,
    ActivityPub,
}
```

### Task 4.4 — Federation object cache (src/models/federation_objects.rs)

```rust
impl FederationObjectModel {
    /// Get cached object or None if not cached / stale
    pub async fn get_fresh(db: &DatabaseConnection, ap_id: &str) -> Result<Option<serde_json::Value>>;

    /// Store or update cached object
    pub async fn upsert(
        db: &DatabaseConnection,
        ap_id: &str,
        object_type: &str,
        json: &serde_json::Value,
    ) -> Result<()>;
}
```

### Task 4.5 — Remote actor fetcher (src/federation/fetch.rs)

```rust
/// Fetch a remote actor (Person or Group) by AP ID.
/// Checks federation_objects cache first. If stale or absent, fetches via HTTP.
/// Validates the response is a valid AP actor.
/// SSRF protection: rejects RFC-1918 and loopback addresses.
pub async fn fetch_actor(
    db: &DatabaseConnection,
    ap_id: &str,
    http_client: &reqwest::Client,
) -> Result<serde_json::Value>;

/// Extract publicKeyPem from an actor JSON object
pub fn extract_public_key(actor: &serde_json::Value) -> Result<String>;

/// SSRF guard: returns Err if the URL is not safe to fetch
pub fn validate_remote_url(url: &Url) -> Result<()>;
```

`validate_remote_url` must reject:
- Non-HTTPS schemes
- `127.0.0.0/8`, `::1`
- `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`
- `169.254.0.0/16` (link-local)

### Task 4.6 — Activity builder (src/federation/activities.rs)

Helper functions to construct AP activity JSON. Never build these by hand in handlers.

```rust
const AP_CONTEXT: &[&str] = &[
    "https://www.w3.org/ns/activitystreams",
    "https://w3id.org/security/v1",
];

pub fn build_create_note(note: &serde_json::Value, actor_ap_id: &str, activity_id: &str) -> serde_json::Value;
pub fn build_announce(object: &serde_json::Value, actor_ap_id: &str, activity_id: &str, followers_url: &str) -> serde_json::Value;
pub fn build_like(note_ap_id: &str, actor_ap_id: &str, activity_id: &str) -> serde_json::Value;
pub fn build_dislike(note_ap_id: &str, actor_ap_id: &str, activity_id: &str) -> serde_json::Value;
pub fn build_undo(object: &serde_json::Value, actor_ap_id: &str, activity_id: &str) -> serde_json::Value;
pub fn build_delete(object_ap_id: &str, actor_ap_id: &str, activity_id: &str) -> serde_json::Value;
pub fn build_follow(target_ap_id: &str, actor_ap_id: &str, activity_id: &str) -> serde_json::Value;
pub fn build_accept(follow_activity: &serde_json::Value, actor_ap_id: &str, activity_id: &str) -> serde_json::Value;

pub fn build_post_note(post: &posts::Model, author: &users::Model, community: Option<&communities::Model>, base_url: &str) -> serde_json::Value;
pub fn build_comment_note(comment: &comments::Model, author: &users::Model, post: &posts::Model, base_url: &str) -> serde_json::Value;
```

Each activity must include the full `@context` array.

### Task 4.7 — Outbound delivery worker (src/workers/deliver_activity.rs)

This worker polls `outbound_activities` where `state='pending'` and `next_attempt_at <= NOW()`.

```rust
pub struct DeliverActivityWorker;

impl Worker<DeliverActivityArgs> for DeliverActivityWorker {
    async fn perform(&self, ctx: &AppContext, args: DeliverActivityArgs) -> Result<()> {
        // 1. Load the outbound_activity row
        // 2. Load signing key from actor_ap_id (look up in users or communities table)
        // 3. Build HttpSigner
        // 4. Serialize activity_json to bytes
        // 5. Call signer.sign_request(POST, target_inbox_url, body)
        // 6. Send HTTP POST with: Content-Type: application/activity+json,
        //    Date, Digest, Signature headers
        // 7. On 2xx: update state = 'sent'
        // 8. On error: increment attempts, set next_attempt_at with backoff,
        //    set state = 'failed' if attempts >= 5
    }
}
```

Backoff schedule: 10s, 30s, 2m, 10m, 1h (after each failure, pick from this list by attempt count).

Helper function to enqueue activities:
```rust
pub async fn enqueue_to_followers(
    db: &DatabaseConnection,
    activity: &serde_json::Value,
    actor_ap_id: &str,
    follower_inboxes: Vec<String>,
) -> Result<()>;
```

This inserts one row into `outbound_activities` per inbox. Use shared inboxes when available to reduce fan-out.

### Task 4.8 — WebFinger (src/controllers/federation/webfinger.rs)

```
GET /.well-known/webfinger?resource=acct:{user_or_community}@{host}
```

1. Parse the `resource` query parameter
2. Determine if it's a user (`/u/username`) or community (`/c/slug`)
3. Look up in DB — return 404 if not found
4. Return JRD JSON (not AP JSON — different content type: `application/jrd+json`)

### Task 4.9 — NodeInfo (src/controllers/federation/nodeinfo.rs)

```
GET /.well-known/nodeinfo → link document
GET /nodeinfo/2.0         → stats document
```

Count local users and local posts from DB for the stats document. Cache the count for 5 minutes (just use a `tokio::sync::Mutex<(Instant, u64)>` — no need for anything fancy).

### Task 4.10 — Actor endpoints (src/controllers/federation/actors.rs)

For `/u/:username` and `/c/:slug`:

1. Read `RenderMode` extension set by content negotiation middleware
2. If `ActivityPub`: return AP JSON-LD actor document
3. If `Html`: delegate to the existing HTML handler (profile or community feed)

Actor JSON-LD format: exactly as specified in SPEC.md Sections 6.2 and 6.3.

```
GET /u/:username/followers  → OrderedCollection of follower AP IDs
GET /u/:username/following  → OrderedCollection of following AP IDs
GET /u/:username/outbox     → OrderedCollection of recent activities
GET /c/:slug/followers      → OrderedCollection of subscriber AP IDs
GET /c/:slug/outbox         → OrderedCollection of recent Announce activities
```

### Task 4.11 — Inbox handler (src/controllers/federation/inbox.rs)

```
POST /u/:username/inbox
POST /c/:slug/inbox
```

Both inboxes share the same activity dispatch logic. Implement as a shared function:

```rust
async fn dispatch_activity(
    ctx: &AppContext,
    activity: serde_json::Value,
    inbox_owner: InboxOwner,  // User(Model) or Community(Model)
) -> Result<()>;
```

**Dispatch by activity type:**

```
Create + object.type == Note  →  handle_create_note()
Announce                      →  handle_announce()
Like                          →  handle_like()
Dislike                       →  handle_dislike()
Undo + object.type == Like    →  handle_undo_like()
Undo + object.type == Dislike →  handle_undo_dislike()
Undo + object.type == Follow  →  handle_undo_follow()
Delete                        →  handle_delete()
Update + object.type == Note  →  handle_update_note()
Follow                        →  handle_follow()
unknown type                  →  log and return Ok(()) (do not error)
```

**handle_create_note:** 
1. Extract `attributedTo` (author AP ID), fetch actor
2. Upsert remote user into `users` table (is_local=false)
3. Insert post into `posts` table
4. If inbox owner is a Community: enqueue `Announce{Create{Note}}` to all community followers (THE ANNOUNCE PATTERN — see SPEC.md 6.4)

**handle_follow:**
1. Insert into `follows` table, `accepted=false`
2. Immediately send `Accept{Follow}` back to follower's inbox
3. Update `follows` row to `accepted=true`

**Idempotency:** Before inserting any object, check if it already exists by `ap_id`. If it does, return `Ok(())` without error.

### Task 4.12 — Hook federation into local actions

After local actions succeed, enqueue outbound activities:

| Local action | Outbound activity | Actor |
|---|---|---|
| Create post in community | Create{Note}, then community Announce | user, then community |
| Create profile post | Create{Note} to followers | user |
| Create comment | Create{Note} | user |
| Upvote post | Like{Note} | user |
| Downvote post | Dislike{Note} | user |
| Remove vote | Undo{Like/Dislike} | user |
| Delete post | Delete{Note} | user or community |
| Edit post | Update{Note} | user |
| Follow user | Follow{Person} | user |
| Follow community | Follow{Group} | user |
| Unfollow | Undo{Follow} | user |

Only enqueue if federation is enabled (`config.taverna.federation.enabled == true`).

### Task 4.13 — Note and collection endpoints

```
GET /posts/:id    → Note JSON-LD for the post
GET /comments/:id → Note JSON-LD for the comment
```

These return pure AP JSON, no content negotiation needed (they're not dual-use with HTML).

### Task 4.14 — Write federation tests

```rust
// tests/federation_test.rs
test_http_signature_sign_verify_roundtrip()
test_http_signature_rejects_old_date()
test_webfinger_user()
test_webfinger_community()
test_webfinger_unknown_returns_404()
test_actor_json_returned_for_ap_accept_header()
test_html_returned_for_browser_accept_header()
test_ssrf_guard_rejects_loopback()
test_ssrf_guard_rejects_rfc1918()
test_inbox_create_note_inserts_post()
test_inbox_follow_sends_accept()
test_inbox_idempotent_on_duplicate()
test_announce_pattern_fires_on_community_post()
```

---

## Deliverables Checklist

- [ ] `src/federation/signatures.rs` — sign and verify, with unit tests
- [ ] `src/initializers/http_signature.rs` — inbox verification middleware
- [ ] `src/initializers/content_negotiation.rs` — RenderMode middleware
- [ ] `src/federation/fetch.rs` — actor fetcher with SSRF guard
- [ ] `src/federation/activities.rs` — all activity builder functions
- [ ] `src/models/federation_objects.rs` — cache model
- [ ] `src/workers/deliver_activity.rs` — delivery worker with backoff
- [ ] `src/controllers/federation/webfinger.rs`
- [ ] `src/controllers/federation/nodeinfo.rs`
- [ ] `src/controllers/federation/actors.rs` — actor endpoints + collections
- [ ] `src/controllers/federation/inbox.rs` — full dispatch logic
- [ ] All local actions hooked to enqueue outbound activities
- [ ] `GET /posts/:id` and `GET /comments/:id` returning AP JSON
- [ ] All federation routes registered in app.rs
- [ ] `cargo check` passes
- [ ] `cargo test` passes (all federation tests)

---

## Common Mistakes to Avoid

- **DO NOT** skip the Announce pattern — this is the #1 Lemmy compatibility issue
- **DO NOT** fetch remote actors without SSRF validation
- **DO NOT** ignore unknown activity types — log them and return Ok(())
- **DO NOT** error on duplicate inbound activities — be idempotent
- **DO NOT** hardcode the `@context` array inline — use the constant
- **DO NOT** forget to send `Accept{Follow}` in response to incoming `Follow`
- **DO NOT** sign requests with the community key when the actor is a user and vice versa
- **DO NOT** block the web request thread for activity delivery — always use the worker queue

---

## Prompts

Prompts for redoing or extending this phase with an AI coding agent. They work with any
tool; see [INSTRUCTIONS.md](INSTRUCTIONS.md) for the workflow.

### Implement

```
Read AGENTS.md, docs/ai/SPEC.md, and docs/ai/PHASE_4.md.

Phases 1–3 are complete. Local features and the theme switcher work.

Show me a plan before writing any code.

Implement ActivityPub federation using the activitypub_federation crate (version 0.6,
axum feature). The crate handles HTTP signatures, actor fetching, and activity sending.

Do NOT implement these manually:
- Use receive_activity() in inbox handlers (signatures auto-verified)
- Use SendActivityTask::send() for all outbound activities
- Use build_webfinger_response() and extract_webfinger_name() for WebFinger
- Use FederationJson and FEDERATION_CONTENT_TYPE for content negotiation
- Use ObjectId::dereference() for actor resolution

Critical:
1. Announce pattern (Task 4.4) is mandatory for Lemmy compatibility
2. All ActivityHandler::receive implementations must be idempotent (check ap_id)
3. Call verify_domains_match() in every Object::verify implementation
4. Apply FederationMiddleware to all AP route groups

Implement Task 4.1 (AP struct types) first and show me the result before continuing.

Verify the Phase 4 deliverables checklist when done.
```

Gate:
```bash
cargo check && cargo test
cargo loco start &
curl "http://localhost:3000/.well-known/webfinger?resource=acct:admin@localhost"
curl -H "Accept: application/activity+json" "http://localhost:3000/u/admin"
curl "http://localhost:3000/nodeinfo/2.0"
```

### Review

```
You are a QA engineer reviewing Phase 4 of the Taverna project.

Read AGENTS.md, docs/ai/SPEC.md Section 6, and docs/ai/PHASE_4.md.

Verify:

NO HAND-ROLLED SIGNATURES
- Search src/ for: use rsa::, use sha2::, use base64:: in federation context.
- If found: CRITICAL — the activitypub_federation crate handles this.

INBOX HANDLERS USE receive_activity
- Open src/controllers/federation/inbox.rs.
- Both user_inbox and community_inbox must call receive_activity::<...>() from the crate.
- If they verify signatures manually: CRITICAL.

OUTBOUND USES SendActivityTask
- Search src/ for reqwest::Client and reqwest::post in federation code.
- If raw reqwest is used to send to remote inboxes: CRITICAL.

WEBFINGER USES CRATE HELPERS
- Open src/controllers/federation/webfinger.rs.
- Must call build_webfinger_response() and extract_webfinger_name().
- If it manually builds the JRD JSON: MEDIUM.

verify_domains_match IN EVERY Object::verify
- Open each Object trait implementation (person.rs, group.rs, note.rs).
- Each verify() must call verify_domains_match(json.id.inner(), expected_domain)?.
- Missing calls: CRITICAL (enables cross-domain actor spoofing).

ANNOUNCE PATTERN
- Find Create's ActivityHandler::receive implementation.
- When the inbox owner is a Community, after saving the post, it must:
  (a) Build an Announce activity with the community as actor
  (b) Call SendActivityTask::send with the community as signing actor
  (c) Send to the community's follower inboxes
- If either (a) or (b) is missing: CRITICAL (Lemmy federation broken).

IDEMPOTENCY IN ALL receive() METHODS
- Check every ActivityHandler::receive.
- Each must check if the object exists by ap_id before inserting.
- Missing checks: CRITICAL (duplicate data on delivery retries).

FederationMiddleware APPLIED
- Open src/app.rs.
- The AP route group must have .layer(FederationMiddleware::new(...)).
- If missing: CRITICAL (runtime panics on Data<T> extraction).

CONTENT NEGOTIATION USES FEDERATION_CONTENT_TYPE
- Search for hardcoded "application/activity+json" string.
- Should use the FEDERATION_CONTENT_TYPE constant from the crate.
- If hardcoded: LOW.

NodeInfo CORRECTNESS
- Open src/controllers/federation/nodeinfo.rs.
- /nodeinfo/2.0 must return: "name": "taverna" and "protocols": ["activitypub"].
- If name is wrong or protocols is missing: MEDIUM.

Run: cargo check
Run: cargo test

Then manually test:
  cargo loco start &
  curl "http://localhost:3000/.well-known/webfinger?resource=acct:admin@localhost"
  curl -H "Accept: application/activity+json" "http://localhost:3000/u/admin"
  curl "http://localhost:3000/nodeinfo/2.0"

Report CRITICAL / MEDIUM / LOW.
```
