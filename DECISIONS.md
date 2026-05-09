# Architecture Decision Log

This file records WHY specific decisions were made. If you are an AI agent reading this, do not reverse these decisions without explicit user instruction.

---

## ADR-001: PostgreSQL for Everything (No Redis)

**Decision:** Use PostgreSQL for the job queue (pgmq) and federation cache, not Redis.

**Reason:** Self-hosting is simpler with one service. pgmq provides durable queues with at-least-once delivery. The federation cache (federation_objects table) needs persistence across restarts anyway. Redis would add operational complexity for no meaningful performance gain at the expected scale.

**Do not add Redis.**

---

## ADR-002: No JavaScript Framework

**Decision:** HTMX + Tera templates only. No React, Vue, Svelte, or similar.

**Reason:** The project is Rust-first. A JS framework would require a Node.js build step, TypeScript compilation, and a separate development server. HTMX achieves the necessary interactivity (voting, infinite scroll, inline forms) with server-rendered HTML and minimal JavaScript. This keeps the entire application in one language ecosystem.

**Do not add a JavaScript build step or framework.**

---

## ADR-003: RSA-2048 for ActivityPub Keys

**Decision:** Use RSA-2048 keypairs for HTTP signatures, not Ed25519.

**Reason:** Lemmy and Mastodon both use RSA-SHA256 for HTTP signature verification (draft-cavage-http-signatures-12). Ed25519 is not supported by the major Fediverse implementations as of 2024. Interoperability is more important than key size modernity here.

**Do not switch to Ed25519 without verifying Lemmy and Mastodon support it.**

---

## ADR-004: Separate Keypairs for Users and Communities

**Decision:** Each community has its own RSA keypair, separate from any user keypair.

**Reason:** In Lemmy's federation model, the community Group actor signs the Announce activities it sends to followers. If communities shared a user's key, the key ID in the signature wouldn't match the community actor, breaking signature verification on remote servers.

**Each community must have its own keypair.**

---

## ADR-005: ltree for Comment Threading

**Decision:** Use PostgreSQL ltree extension for comment paths.

**Reason:** Adjacency list queries (parent_id recursion with CTEs) become expensive at depth. ltree allows fetching an entire thread subtree with a single index scan: `WHERE path <@ 'root_path'`. It also makes sorting by thread path trivial.

**Do not replace ltree with recursive CTEs or application-side threading.**

---

## ADR-006: Denormalized score on posts and comments

**Decision:** Maintain a `score` column on posts and comments that is recalculated after each vote.

**Reason:** Counting votes on every feed query would make the community feed slow (aggregate over votes table per post). Denormalization trades write complexity for read performance. The feed is read far more often than it is written.

**Always call recalculate_score after any vote change.**

---

## ADR-007: Content Negotiation (same URL, dual response)

**Decision:** `/u/:username` and `/c/:slug` serve both HTML (for browsers) and AP JSON-LD (for federation), using the Accept header to decide.

**Reason:** This is how Mastodon and Lemmy do it. Remote servers fetch the actor by its canonical URL. That URL must return AP JSON when requested with the right Accept header. Having separate `/ap/u/:username` URLs would break WebFinger discovery, which points to the canonical actor URL.

**Do not split HTML and AP JSON into separate URL paths.**

---

## ADR-008: Auto-accept all follows

**Decision:** When a Follow activity arrives, immediately send Accept{Follow}.

**Reason:** Taverna communities and profiles are public. Manual approval adds friction for legitimate federation. If a moderation need arises, banning the remote instance's domain is more effective than per-follow approval.

**Do not implement manual follow approval in v1.**

---

## ADR-009: Idempotent inbox processing

**Decision:** Receiving the same activity twice must be a no-op, not an error or duplicate insert.

**Reason:** The Fediverse is eventually consistent and unreliable. Remote servers retry deliveries. Network failures cause duplicate sends. Idempotency (checking ap_id before insert) is required for correctness.

**Always check ap_id existence before inserting any federated object.**
