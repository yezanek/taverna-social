use activitypub_federation::config::Data;
use activitypub_federation::protocol::verification::verify_domains_match;
use activitypub_federation::traits::Activity;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use serde::Serialize;
use serde::Serializer;
use tracing::instrument;
use url::Url;

use crate::federation::outbound;
use crate::federation::protocol::*;
use crate::models::_entities::{comments, communities, follows, posts, users};
use crate::models::remote::{
    RemoteActorModel, RemoteCommentModel, RemotePostModel, forget_received_activity,
    record_received_activity,
};
use crate::models::votes::VoteModel;
use crate::workers::deliver_activity::{enqueue_activity, get_follower_inboxes_deduplicated};
use uuid::Uuid;

/// Wrapper for outbound activity JSON that implements Activity + Serialize.
/// This allows us to send pre-built JSON activities through the crate's
/// SendActivityTask::prepare + sign_and_send pipeline.
#[derive(Debug, Clone)]
pub struct SendActivity {
    pub id: Url,
    pub actor: Url,
    pub json: serde_json::Value,
}

#[async_trait::async_trait]
impl Activity for SendActivity {
    type DataType = DatabaseConnection;
    type Error = anyhow::Error;

    fn id(&self) -> &Url {
        &self.id
    }

    fn actor(&self) -> &Url {
        &self.actor
    }

    async fn verify(&self, _data: &Data<Self::DataType>) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn receive(self, _data: &Data<Self::DataType>) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl Serialize for SendActivity {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.json.serialize(serializer)
    }
}

// ── Activity trait implementation ───────────────────────────

#[async_trait::async_trait]
impl Activity for TavernaActivity {
    type DataType = DatabaseConnection;
    type Error = anyhow::Error;

    fn id(&self) -> &Url {
        match self {
            TavernaActivity::CreateActivity(a) => &a.id,
            TavernaActivity::UpdateActivity(a) => &a.id,
            TavernaActivity::DeleteActivity(a) => &a.id,
            TavernaActivity::FollowActivity(a) => &a.id,
            TavernaActivity::AcceptFollowActivity(a) => &a.id,
            TavernaActivity::LikeActivity(a) => &a.id,
            TavernaActivity::DislikeActivity(a) => &a.id,
            TavernaActivity::AnnounceActivity(a) => &a.id,
            TavernaActivity::UndoFollowActivity(a) => &a.id,
            TavernaActivity::UndoLikeActivity(a) => &a.id,
            TavernaActivity::UndoDislikeActivity(a) => &a.id,
        }
    }

    fn actor(&self) -> &Url {
        match self {
            TavernaActivity::CreateActivity(a) => &a.actor,
            TavernaActivity::UpdateActivity(a) => &a.actor,
            TavernaActivity::DeleteActivity(a) => &a.actor,
            TavernaActivity::FollowActivity(a) => &a.actor,
            TavernaActivity::AcceptFollowActivity(a) => &a.actor,
            TavernaActivity::LikeActivity(a) => &a.actor,
            TavernaActivity::DislikeActivity(a) => &a.actor,
            TavernaActivity::AnnounceActivity(a) => &a.actor,
            TavernaActivity::UndoFollowActivity(a) => &a.actor,
            TavernaActivity::UndoLikeActivity(a) => &a.actor,
            TavernaActivity::UndoDislikeActivity(a) => &a.actor,
        }
    }

    #[instrument(skip(self, _data))]
    async fn verify(&self, _data: &Data<Self::DataType>) -> Result<(), Self::Error> {
        // The crate has already checked the HTTP signature against `actor`.
        // An activity's id must live on its actor's server, or one server
        // could forge activities that appear to come from another.
        verify_domains_match(self.id(), self.actor())?;
        Ok(())
    }

    #[instrument(skip(self, data))]
    async fn receive(self, data: &Data<Self::DataType>) -> Result<(), Self::Error> {
        let db = data.app_data();
        let activity_id = self.id().as_str().to_string();
        match record_received_activity(db, &activity_id).await {
            Ok(true) => {}
            Ok(false) => {
                tracing::debug!(activity_id, "skipping already-processed activity");
                return Ok(());
            }
            Err(e) => {
                tracing::warn!(error = %e, activity_id, "idempotency check failed, proceeding");
            }
        }

        let result = match self {
            TavernaActivity::CreateActivity(a) => handle_create(db, a).await,
            TavernaActivity::UpdateActivity(a) => handle_update(db, a).await,
            TavernaActivity::DeleteActivity(a) => handle_delete(db, a).await,
            TavernaActivity::FollowActivity(a) => handle_follow(db, a).await,
            TavernaActivity::LikeActivity(a) => {
                handle_vote(db, a.actor.as_str(), a.object.as_str(), 1).await
            }
            TavernaActivity::DislikeActivity(a) => {
                handle_vote(db, a.actor.as_str(), a.object.as_str(), -1).await
            }
            TavernaActivity::AcceptFollowActivity(a) => handle_accept_follow(db, a).await,
            TavernaActivity::AnnounceActivity(a) => {
                tracing::info!("Received Announce activity: {}", a.id);
                Ok(())
            }
            TavernaActivity::UndoFollowActivity(a) => handle_undo_follow(db, a).await,
            TavernaActivity::UndoLikeActivity(a) => {
                handle_undo_vote(db, a.actor.as_str(), a.object.object.as_str()).await
            }
            TavernaActivity::UndoDislikeActivity(a) => {
                handle_undo_vote(db, a.actor.as_str(), a.object.object.as_str()).await
            }
        };

        // Only a processed activity counts as received: if handling failed,
        // forget it so the sender's retry is not skipped as a duplicate.
        if result.is_err()
            && let Err(e) = forget_received_activity(db, &activity_id).await
        {
            tracing::warn!(error = %e, activity_id, "failed to clear failed activity");
        }
        result
    }
}

// ── Handler implementations ─────────────────────────────────

/// Finds the local community a remote Note is addressed to, if any.
async fn addressed_community(
    db: &DatabaseConnection,
    note: &Note,
    activity: &Create,
) -> Result<Option<communities::Model>, anyhow::Error> {
    for target in note
        .to
        .iter()
        .chain(note.cc.iter())
        .chain(activity.to.iter())
        .chain(activity.cc.iter())
    {
        if let Some(c) = communities::Entity::find()
            .filter(
                sea_orm::Condition::any()
                    .add(communities::Column::ApId.eq(target.as_str()))
                    .add(communities::Column::FollowersUrl.eq(target.as_str())),
            )
            .one(db)
            .await?
        {
            return Ok(Some(c));
        }
    }
    Ok(None)
}

/// Create{Note}: remote posts go to `remote_posts`, remote replies to
/// `remote_comments` (ADR-017). Never writes to local `posts`/`comments`.
async fn handle_create(db: &DatabaseConnection, activity: Create) -> Result<(), anyhow::Error> {
    let note = &activity.object;
    let actor = activity.actor.as_str();
    if note.attributed_to.as_str() != actor {
        anyhow::bail!(
            "Create actor {} does not match attributedTo {}",
            actor,
            note.attributed_to
        );
    }
    RemoteActorModel::ensure(db, actor).await?;
    let raw = serde_json::to_value(note)?;

    if let Some(in_reply_to) = &note.in_reply_to {
        let Some(position) = RemoteCommentModel::resolve_parent(db, in_reply_to.as_str()).await?
        else {
            tracing::debug!(note = %note.id, parent = %in_reply_to, "reply to unknown object, ignoring");
            return Ok(());
        };
        RemoteCommentModel::upsert(db, note, &position, raw).await?;
        return Ok(());
    }

    let community = addressed_community(db, note, &activity).await?;
    let (_, is_new) =
        RemotePostModel::upsert(db, note, community.as_ref().map(|c| c.id), raw).await?;

    // ★ THE ANNOUNCE PATTERN (SPEC.md §6.4)
    // A post addressed to one of our communities is announced to its followers.
    if is_new && let Some(community) = community.filter(|c| c.is_local) {
        // Lemmy expects the original Create wrapped in the Announce.
        let announce_activity = serde_json::json!({
            "@context": AP_CONTEXT,
            "id": outbound::activity_id(&community.ap_id),
            "type": "Announce",
            "actor": community.ap_id,
            "object": serde_json::to_value(&activity)?,
            "to": [outbound::AP_PUBLIC],
            "cc": [community.followers_url],
            "published": chrono::Utc::now().to_rfc3339(),
        });

        let inboxes = get_follower_inboxes_deduplicated(db, community.id)
            .await
            .unwrap_or_default();
        if let Err(e) = enqueue_activity(db, &announce_activity, &community.ap_id, inboxes).await {
            tracing::warn!(community_id = %community.id, error = %e, "failed to enqueue announce to followers");
        }
    }

    Ok(())
}

/// Update{Note}: only the author may edit, and only cached remote copies change.
async fn handle_update(db: &DatabaseConnection, activity: Update) -> Result<(), anyhow::Error> {
    let actor = activity.actor.as_str();
    if !RemotePostModel::apply_update(db, actor, &activity.object).await? {
        RemoteCommentModel::apply_update(db, actor, &activity.object).await?;
    }
    Ok(())
}

/// Delete: only the author may delete, and only cached remote copies change.
async fn handle_delete(db: &DatabaseConnection, activity: Delete) -> Result<(), anyhow::Error> {
    let actor = activity.actor.as_str();
    let object = activity.object.as_str();
    if !RemotePostModel::mark_deleted(db, actor, object).await? {
        RemoteCommentModel::mark_deleted(db, actor, object).await?;
    }
    Ok(())
}

async fn handle_follow(db: &DatabaseConnection, activity: Follow) -> Result<(), anyhow::Error> {
    let object_ap_id = activity.object.as_str();
    let activity_id = activity.id.as_str();
    let follower = RemoteActorModel::ensure(db, activity.actor.as_str()).await?;

    let (target_ap_id, existing, following_id, community_id) = if let Some(target_user) =
        users::Entity::find()
            .filter(users::Column::ApId.eq(object_ap_id))
            .one(db)
            .await?
    {
        let existing = follows::Entity::find()
            .filter(follows::Column::RemoteFollowerApId.eq(follower.ap_id.as_str()))
            .filter(follows::Column::FollowingId.eq(target_user.id))
            .one(db)
            .await?;
        (target_user.ap_id, existing, Some(target_user.id), None)
    } else if let Some(target_community) = communities::Entity::find()
        .filter(communities::Column::ApId.eq(object_ap_id))
        .filter(communities::Column::IsLocal.eq(true))
        .one(db)
        .await?
    {
        let existing = follows::Entity::find()
            .filter(follows::Column::RemoteFollowerApId.eq(follower.ap_id.as_str()))
            .filter(follows::Column::CommunityId.eq(target_community.id))
            .one(db)
            .await?;
        (
            target_community.ap_id,
            existing,
            None,
            Some(target_community.id),
        )
    } else {
        return Ok(());
    };

    if existing.is_none() {
        follows::ActiveModel {
            id: Set(Uuid::new_v4()),
            follower_id: Set(None),
            remote_follower_ap_id: Set(Some(follower.ap_id.clone())),
            following_id: Set(following_id),
            community_id: Set(community_id),
            remote_following_ap_id: Set(None),
            ap_id: Set(Some(activity_id.to_string())),
            accepted: Set(false),
            created_at: Set(chrono::Utc::now().into()),
        }
        .insert(db)
        .await?;
    }

    // ADR-008: auto-accept. Re-send Accept for repeated Follows too, in case
    // the first Accept was lost.
    send_accept_follow(db, &activity, &target_ap_id, &follower.inbox_url).await?;
    Ok(())
}

async fn handle_accept_follow(
    db: &DatabaseConnection,
    activity: AcceptFollow,
) -> Result<(), anyhow::Error> {
    let follow_id = activity.object.id.as_str();
    follows::Entity::update_many()
        .filter(follows::Column::ApId.eq(follow_id))
        .col_expr(
            follows::Column::Accepted,
            sea_orm::sea_query::Expr::value(true),
        )
        .exec(db)
        .await?;
    tracing::info!("Accepted follow: {}", follow_id);
    Ok(())
}

async fn send_accept_follow(
    db: &DatabaseConnection,
    follow_activity: &Follow,
    object_ap_id: &str,
    follower_inbox: &str,
) -> Result<(), anyhow::Error> {
    let accept_activity = serde_json::json!({
        "@context": AP_CONTEXT,
        "id": format!("{}/accepts/{}", object_ap_id.trim_end_matches('/'), Uuid::new_v4()),
        "type": "Accept",
        "actor": object_ap_id,
        "object": {
            "@context": AP_CONTEXT,
            "id": follow_activity.id.as_str(),
            "type": "Follow",
            "actor": follow_activity.actor.as_str(),
            "object": follow_activity.object.as_str(),
        },
        "to": [follow_activity.actor.as_str()],
    });

    enqueue_activity(
        db,
        &accept_activity,
        object_ap_id,
        vec![follower_inbox.to_string()],
    )
    .await?;

    follows::Entity::update_many()
        .filter(follows::Column::ApId.eq(follow_activity.id.as_str()))
        .col_expr(
            follows::Column::Accepted,
            sea_orm::sea_query::Expr::value(true),
        )
        .exec(db)
        .await?;

    Ok(())
}

/// Resolves a vote target to a local post or local comment id.
async fn local_vote_target(
    db: &DatabaseConnection,
    object_ap_id: &str,
) -> Result<Option<(Option<Uuid>, Option<Uuid>)>, anyhow::Error> {
    if let Some(post) = posts::Entity::find()
        .filter(posts::Column::ApId.eq(object_ap_id))
        .one(db)
        .await?
    {
        return Ok(Some((Some(post.id), None)));
    }
    if let Some(comment) = comments::Entity::find()
        .filter(comments::Column::ApId.eq(object_ap_id))
        .one(db)
        .await?
    {
        return Ok(Some((None, Some(comment.id))));
    }
    Ok(None)
}

/// Like (+1) / Dislike (-1) from a remote actor on local content.
async fn handle_vote(
    db: &DatabaseConnection,
    actor_ap_id: &str,
    object_ap_id: &str,
    value: i16,
) -> Result<(), anyhow::Error> {
    let Some((post_id, comment_id)) = local_vote_target(db, object_ap_id).await? else {
        return Ok(());
    };
    RemoteActorModel::ensure(db, actor_ap_id).await?;
    if let Err(e) = VoteModel::cast_remote_vote(db, actor_ap_id, post_id, comment_id, value).await {
        tracing::warn!(actor = actor_ap_id, object = object_ap_id, error = %e, "failed to record remote vote");
    }
    Ok(())
}

async fn handle_undo_vote(
    db: &DatabaseConnection,
    actor_ap_id: &str,
    object_ap_id: &str,
) -> Result<(), anyhow::Error> {
    let Some((post_id, comment_id)) = local_vote_target(db, object_ap_id).await? else {
        return Ok(());
    };
    if let Err(e) = VoteModel::delete_remote_vote(db, actor_ap_id, post_id, comment_id).await {
        tracing::warn!(actor = actor_ap_id, object = object_ap_id, error = %e, "failed to delete remote vote");
    }
    Ok(())
}

async fn handle_undo_follow(
    db: &DatabaseConnection,
    activity: UndoFollow,
) -> Result<(), anyhow::Error> {
    follows::Entity::delete_many()
        .filter(follows::Column::ApId.eq(activity.object.id.as_str()))
        .filter(follows::Column::RemoteFollowerApId.eq(activity.actor.as_str()))
        .exec(db)
        .await?;
    Ok(())
}
