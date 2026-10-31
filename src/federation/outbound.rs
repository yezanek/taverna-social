//! Builders for the ActivityPub objects and activities Taverna sends, and the
//! one place that queues them for delivery (ADR-019).
//!
//! Controllers used to assemble these with ad-hoc `json!` blocks; keeping
//! them here gives every activity the same `@context`, addressing and id
//! scheme, and makes `Undo` reference the exact `Follow` it undoes.
use sea_orm::DatabaseConnection;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::common::markdown::render_markdown;
use crate::federation::protocol::AP_CONTEXT;
use crate::models::_entities::{comments, communities, posts};
use crate::workers::deliver_activity::{
    enqueue_activity, get_follower_inboxes_deduplicated, get_user_follower_inboxes_deduplicated,
};

/// The ActivityStreams public collection.
pub const AP_PUBLIC: &str = "https://www.w3.org/ns/activitystreams#Public";

/// A fresh, unique activity id under the actor's URL.
#[must_use]
pub fn activity_id(actor_ap_id: &str) -> String {
    format!(
        "{}/activities/{}",
        actor_ap_id.trim_end_matches('/'),
        Uuid::new_v4()
    )
}

/// The id of `actor`'s Follow of `target`. It is deterministic so a later
/// `Undo{Follow}` names the same Follow the remote server stored.
#[must_use]
pub fn follow_id(actor_ap_id: &str, target_id: Uuid) -> String {
    format!(
        "{}/follows/{}",
        actor_ap_id.trim_end_matches('/'),
        target_id
    )
}

/// `content` (HTML) plus `source` (the original markdown), as Lemmy expects.
fn content_fields(markdown: &str) -> (Value, Value) {
    (
        json!(render_markdown(markdown)),
        json!({ "content": markdown, "mediaType": "text/markdown" }),
    )
}

/// A local post as a `Note` (SPEC §6). Posts in a community are addressed
/// to it via `audience` and `cc`; profile posts go to the author's followers.
#[must_use]
pub fn post_note(
    post: &posts::Model,
    author_ap_id: &str,
    community: Option<&communities::Model>,
    author_followers_url: &str,
) -> Value {
    let mut note = json!({
        "@context": AP_CONTEXT,
        "id": post.ap_id,
        "type": "Note",
        "attributedTo": author_ap_id,
        "name": post.title,
        "published": post.published_at.to_rfc3339(),
        "url": post.ap_id,
        "to": [AP_PUBLIC],
    });
    if let Some(body) = &post.body {
        let (content, source) = content_fields(body);
        note["content"] = content;
        note["source"] = source;
    }
    if let Some(link) = &post.url {
        note["attachment"] = json!([{ "type": "Link", "href": link }]);
    }
    match community {
        Some(c) => {
            note["audience"] = json!(c.ap_id);
            note["cc"] = json!([c.ap_id, c.followers_url]);
        }
        None => note["cc"] = json!([author_followers_url]),
    }
    note
}

/// A local comment as a `Note` replying to `in_reply_to`.
#[must_use]
pub fn comment_note(comment: &comments::Model, author_ap_id: &str, in_reply_to: &str) -> Value {
    let (content, source) = content_fields(&comment.body);
    json!({
        "@context": AP_CONTEXT,
        "id": comment.ap_id,
        "type": "Note",
        "attributedTo": author_ap_id,
        "content": content,
        "source": source,
        "inReplyTo": in_reply_to,
        "published": comment.published_at.to_rfc3339(),
        "url": comment.ap_id,
        "to": [AP_PUBLIC],
        "cc": [],
    })
}

/// Wraps `object` in an activity of `kind`, copying its addressing.
#[must_use]
pub fn wrap(kind: &str, actor_ap_id: &str, object: Value) -> Value {
    let to = object
        .get("to")
        .cloned()
        .unwrap_or_else(|| json!([AP_PUBLIC]));
    let cc = object.get("cc").cloned().unwrap_or_else(|| json!([]));
    json!({
        "@context": AP_CONTEXT,
        "id": activity_id(actor_ap_id),
        "type": kind,
        "actor": actor_ap_id,
        "object": object,
        "to": to,
        "cc": cc,
        "published": chrono::Utc::now().to_rfc3339(),
    })
}

/// `Like` (value > 0) or `Dislike` of `object_ap_id`.
#[must_use]
pub fn vote(actor_ap_id: &str, object_ap_id: &str, value: i16) -> Value {
    json!({
        "@context": AP_CONTEXT,
        "id": activity_id(actor_ap_id),
        "type": if value > 0 { "Like" } else { "Dislike" },
        "actor": actor_ap_id,
        "object": object_ap_id,
        "to": [AP_PUBLIC],
        "published": chrono::Utc::now().to_rfc3339(),
    })
}

/// `Delete` of `object_ap_id`.
#[must_use]
pub fn delete(actor_ap_id: &str, object_ap_id: &str) -> Value {
    json!({
        "@context": AP_CONTEXT,
        "id": activity_id(actor_ap_id),
        "type": "Delete",
        "actor": actor_ap_id,
        "object": object_ap_id,
        "to": [AP_PUBLIC],
        "published": chrono::Utc::now().to_rfc3339(),
    })
}

/// `Follow` of `target_ap_id`, with the deterministic [`follow_id`].
#[must_use]
pub fn follow(actor_ap_id: &str, target_ap_id: &str, target_id: Uuid) -> Value {
    json!({
        "@context": AP_CONTEXT,
        "id": follow_id(actor_ap_id, target_id),
        "type": "Follow",
        "actor": actor_ap_id,
        "object": target_ap_id,
    })
}

/// `Undo{Follow}` naming the same Follow that [`follow`] sent.
#[must_use]
pub fn undo_follow(actor_ap_id: &str, target_ap_id: &str, target_id: Uuid) -> Value {
    json!({
        "@context": AP_CONTEXT,
        "id": activity_id(actor_ap_id),
        "type": "Undo",
        "actor": actor_ap_id,
        "object": {
            "id": follow_id(actor_ap_id, target_id),
            "type": "Follow",
            "actor": actor_ap_id,
            "object": target_ap_id,
        },
    })
}

/// Queues `activity` for `inboxes`. Federation is best effort: a failure to
/// queue is logged and never fails the user's request.
pub async fn send(
    db: &DatabaseConnection,
    activity: &Value,
    actor_ap_id: &str,
    inboxes: Vec<String>,
) {
    if let Err(e) = enqueue_activity(db, activity, actor_ap_id, inboxes).await {
        tracing::error!(activity = %activity["id"], error = %e, "failed to enqueue activity");
    }
}

/// Queues `activity` for the remote followers of a local community.
pub async fn send_to_community_followers(
    db: &DatabaseConnection,
    activity: &Value,
    actor_ap_id: &str,
    community_id: Uuid,
) {
    match get_follower_inboxes_deduplicated(db, community_id).await {
        Ok(inboxes) => send(db, activity, actor_ap_id, inboxes).await,
        Err(e) => {
            tracing::error!(%community_id, error = %e, "failed to load community follower inboxes");
        }
    }
}

/// Queues `activity` for the remote followers of a local user.
pub async fn send_to_user_followers(
    db: &DatabaseConnection,
    activity: &Value,
    user_id: Uuid,
    actor_ap_id: &str,
) {
    match get_user_follower_inboxes_deduplicated(db, user_id).await {
        Ok(inboxes) => send(db, activity, actor_ap_id, inboxes).await,
        Err(e) => {
            tracing::error!(%user_id, error = %e, "failed to load user follower inboxes");
        }
    }
}
