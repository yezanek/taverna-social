---
description: Implement the ActivityPub federation layer (Phase 4) using the activitypub_federation crate. Shows a plan first.
---

You are implementing ActivityPub federation for Taverna.

Read AGENTS.md, SPEC.md Section 6, and PHASE_4.md completely before doing anything.

**What the activitypub_federation crate handles — do NOT reimplement:**
- HTTP signature signing and verification
- Remote actor fetching and caching (ObjectId::dereference)
- WebFinger response building
- Activity sending with retry (SendActivityTask::send)
- SSRF protection
- FederationMiddleware and Data<T> extractors
- FederationJson response type

**What you implement:**
- AP struct types (Person, Group, Note, all activities)
- Object trait on DbUser, DbCommunity, DbPost, DbComment
- Actor trait on DbUser, DbCommunity
- ActivityHandler trait on every activity struct
- Route handlers for inbox, outbox, WebFinger, NodeInfo, actor endpoints

Show me a plan listing every file you will create before writing any code.
Wait for my approval.

Then implement in this order (compile after each step):

1. src/federation/types/person.rs — Person struct + Object + Actor impls
2. src/federation/types/group.rs — Group struct + Object + Actor impls
3. src/federation/types/note.rs — Note struct + Object impl
4. src/federation/types/activities.rs — all activity structs + ActivityHandler impls
   (Follow, Accept, Create, Announce, Like, Dislike, Delete, Update, Undo variants)
   (PersonInboxActivities and CommunityInboxActivities enums)
5. src/federation/send.rs — helpers wrapping SendActivityTask::send
6. src/controllers/federation/webfinger.rs — using crate helpers
7. src/controllers/federation/nodeinfo.rs — manual implementation
8. src/controllers/federation/inbox.rs — using receive_activity()
9. src/controllers/federation/actors.rs — content negotiation, outbox, collections
10. src/controllers/federation/objects.rs — /posts/:id and /comments/:id
11. Hook local actions to send outbound activities
12. Register all federation routes with FederationMiddleware

**Critical rules:**
- verify_domains_match() in EVERY Object::verify implementation
- Idempotency: check ap_id exists before every insert in ActivityHandler::receive
- Announce pattern: Create::receive in community inbox MUST send Announce to followers
- Use FEDERATION_CONTENT_TYPE constant (not hardcoded string) for content negotiation

After all steps, run cargo test including federation tests.
