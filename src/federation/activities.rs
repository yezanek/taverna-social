use activitypub_federation::config::Data;
use activitypub_federation::traits::Activity;
use activitypub_federation::traits::Object;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use serde::Serialize;
use serde::Serializer;
use tracing::instrument;
use url::Url;

use crate::federation::protocol::*;
use crate::models::_entities::{communities, federation_objects, follows, posts, users};

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
        // Basic verification: check that actor is not empty
        if self.actor().as_str().is_empty() {
            anyhow::bail!("Activity has no actor");
        }
        Ok(())
    }

    #[instrument(skip(self, data))]
    async fn receive(self, data: &Data<Self::DataType>) -> Result<(), Self::Error> {
        let db = data.app_data();
        let activity_id = self.id().as_str();
        let now: chrono::DateTime<chrono::FixedOffset> = chrono::Utc::now().into();
        let obj = federation_objects::ActiveModel {
            id: Set(uuid::Uuid::new_v4()),
            ap_id: Set(activity_id.to_string()),
            object_type: Set("Activity".to_string()),
            object_json: Set(serde_json::Value::Null),
            fetched_at: Set(now),
            created_at: Set(now),
            updated_at: Set(now),
        };
        match obj.insert(db).await {
            Ok(_) => {}
            Err(sea_orm::DbErr::Exec(ref e)) if e.to_string().contains("duplicate key") => {
                tracing::debug!(activity_id, "skipping already-processed activity");
                return Ok(());
            }
            Err(e) => {
                tracing::warn!(error = %e, activity_id, "idempotency check failed, proceeding");
            }
        }

        match self {
            TavernaActivity::CreateActivity(a) => handle_create(data, a).await,
            TavernaActivity::UpdateActivity(a) => handle_update(db, a).await,
            TavernaActivity::DeleteActivity(a) => handle_delete(db, a).await,
            TavernaActivity::FollowActivity(a) => handle_follow(db, a).await,
            TavernaActivity::LikeActivity(a) => handle_like(db, a).await,
            TavernaActivity::DislikeActivity(a) => handle_dislike(db, a).await,
            TavernaActivity::AcceptFollowActivity(a) => handle_accept_follow(db, a).await,
            TavernaActivity::AnnounceActivity(a) => {
                tracing::info!("Received Announce activity: {}", a.id);
                Ok(())
            }
            TavernaActivity::UndoFollowActivity(a) => handle_undo_follow(db, a).await,
            TavernaActivity::UndoLikeActivity(a) => handle_undo_like(db, a).await,
            TavernaActivity::UndoDislikeActivity(a) => handle_undo_dislike(db, a).await,
        }
    }
}

// ── Handler implementations ─────────────────────────────────

async fn handle_create(
    data: &Data<DatabaseConnection>,
    activity: Create,
) -> Result<(), anyhow::Error> {
    let db = data.app_data();
    let note = activity.object;

    if note.in_reply_to.is_some() {
        // It's a comment — use the Object trait's from_json for standardized verification + insertion
        let _comment = crate::federation::traits::DbComment::from_json(note, data).await?;
    } else {
        // It's a post — use the Object trait's from_json for standardized verification + insertion
        let db_post = crate::federation::traits::DbPost::from_json(note, data).await?;
        let post = db_post.0;

        // ★ THE ANNOUNCE PATTERN (SPEC.md §6.4)
        // If the note was addressed to a community's followers, the community
        // enqueues an Announce activity to all its followers.
        if let Some(cid) = post.community_id
            && let Some(community) = communities::Entity::find_by_id(cid).one(db).await?
        {
            let announce_id = build_announce_id(&community.ap_id, &cid, &post.id);
            let announce_note = serde_json::json!({
                "@context": AP_CONTEXT,
                "id": post.ap_id,
                "type": "Note",
                "attributedTo": community.ap_id,
                "content": post.body,
                "name": post.title,
                "published": post.published_at.to_rfc3339(),
                "url": post.ap_id,
                "to": [community.followers_url],
                "cc": ["https://www.w3.org/ns/activitystreams#Public"],
            });
            let announce_activity = serde_json::json!({
                "@context": AP_CONTEXT,
                "id": announce_id,
                "type": "Announce",
                "actor": community.ap_id,
                "object": announce_note,
                "to": [community.followers_url],
                "cc": ["https://www.w3.org/ns/activitystreams#Public"],
                "published": chrono::Utc::now().to_rfc3339(),
            });

            let inboxes =
                crate::workers::deliver_activity::get_follower_inboxes_for_community(db, cid)
                    .await
                    .unwrap_or_default();

            if !inboxes.is_empty()
                && let Err(e) = crate::workers::deliver_activity::enqueue_to_followers(
                    db,
                    &announce_activity,
                    &community.ap_id,
                    inboxes,
                )
                .await
            {
                tracing::warn!(community_id = %cid, error = %e, "failed to enqueue announce to followers");
            }
        }
    }

    Ok(())
}

async fn handle_update(db: &DatabaseConnection, activity: Update) -> Result<(), anyhow::Error> {
    let note = activity.object;
    let ap_id = note.id.as_str();

    if let Some(post) = posts::Entity::find()
        .filter(posts::Column::ApId.eq(ap_id))
        .one(db)
        .await?
    {
        let mut active: posts::ActiveModel = post.into();
        if let Some(content) = note.content {
            active.body = Set(Some(content));
        }
        if let Some(name) = note.name {
            active.title = Set(name);
        }
        active.updated_at = Set(chrono::Utc::now().into());
        ActiveModelTrait::update(active, db).await?;
    }

    Ok(())
}

async fn handle_delete(db: &DatabaseConnection, activity: Delete) -> Result<(), anyhow::Error> {
    let object_str = activity.object.as_str();

    if let Some(post) = posts::Entity::find()
        .filter(posts::Column::ApId.eq(object_str))
        .one(db)
        .await?
    {
        let mut active: posts::ActiveModel = post.into();
        active.is_deleted = Set(true);
        ActiveModelTrait::update(active, db).await?;
    }

    Ok(())
}

async fn handle_follow(db: &DatabaseConnection, activity: Follow) -> Result<(), anyhow::Error> {
    let actor_ap_id = activity.actor.as_str();
    let object_ap_id = activity.object.as_str();
    let activity_id = activity.id.as_str();

    // Idempotency: check if follow already exists
    // We need to look up both actor and target first
    let follower = match users::Entity::find()
        .filter(users::Column::ApId.eq(actor_ap_id))
        .one(db)
        .await?
    {
        Some(u) => u,
        None => return Ok(()), // Silently skip if follower doesn't exist yet
    };

    // Try user follow first
    if let Some(target_user) = users::Entity::find()
        .filter(users::Column::ApId.eq(object_ap_id))
        .one(db)
        .await?
    {
        // Check idempotency
        if follows::Entity::find()
            .filter(follows::Column::FollowerId.eq(follower.id))
            .filter(follows::Column::FollowingId.eq(Some(target_user.id)))
            .one(db)
            .await?
            .is_some()
        {
            return Ok(());
        }

        follows::ActiveModel {
            id: Set(uuid::Uuid::new_v4()),
            follower_id: Set(follower.id),
            following_id: Set(Some(target_user.id)),
            community_id: Set(None),
            ap_id: Set(Some(activity_id.to_string())),
            accepted: Set(false),
            created_at: Set(chrono::Utc::now().into()),
        }
        .insert(db)
        .await?;

        // Send Accept{Follow}
        send_accept_follow(db, &activity, &target_user.ap_id, &follower.inbox_url).await?;
    } else if let Some(target_community) = communities::Entity::find()
        .filter(communities::Column::ApId.eq(object_ap_id))
        .one(db)
        .await?
    {
        // Check idempotency
        if follows::Entity::find()
            .filter(follows::Column::FollowerId.eq(follower.id))
            .filter(follows::Column::CommunityId.eq(Some(target_community.id)))
            .one(db)
            .await?
            .is_some()
        {
            return Ok(());
        }

        follows::ActiveModel {
            id: Set(uuid::Uuid::new_v4()),
            follower_id: Set(follower.id),
            following_id: Set(None),
            community_id: Set(Some(target_community.id)),
            ap_id: Set(Some(activity_id.to_string())),
            accepted: Set(false),
            created_at: Set(chrono::Utc::now().into()),
        }
        .insert(db)
        .await?;

        send_accept_follow(db, &activity, &target_community.ap_id, &follower.inbox_url).await?;
    }

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
        "id": format!("{}/accepts/{}", object_ap_id.trim_end_matches('/'), uuid::Uuid::new_v4()),
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

    let inboxes = vec![follower_inbox.to_string()];
    crate::workers::deliver_activity::enqueue_to_followers(
        db,
        &accept_activity,
        object_ap_id,
        inboxes,
    )
    .await?;

    // Mark follow as accepted
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

async fn handle_like(db: &DatabaseConnection, activity: Like) -> Result<(), anyhow::Error> {
    let object_str = activity.object.as_str();
    let actor_ap_id = activity.actor.as_str();

    let actor = ensure_remote_user(db, actor_ap_id).await?;

    if let Some(post) = posts::Entity::find()
        .filter(posts::Column::ApId.eq(object_str))
        .one(db)
        .await?
        && let Err(e) =
            crate::models::votes::VoteModel::cast_vote(db, actor.id, Some(post.id), None, 1).await
    {
        tracing::warn!(actor_id = %actor.id, post_id = %post.id, error = %e, "failed to record like");
    }

    Ok(())
}

async fn handle_dislike(db: &DatabaseConnection, activity: Dislike) -> Result<(), anyhow::Error> {
    let object_str = activity.object.as_str();
    let actor_ap_id = activity.actor.as_str();

    let actor = ensure_remote_user(db, actor_ap_id).await?;

    if let Some(post) = posts::Entity::find()
        .filter(posts::Column::ApId.eq(object_str))
        .one(db)
        .await?
        && let Err(e) =
            crate::models::votes::VoteModel::cast_vote(db, actor.id, Some(post.id), None, -1).await
    {
        tracing::warn!(actor_id = %actor.id, post_id = %post.id, error = %e, "failed to record dislike");
    }

    Ok(())
}

async fn handle_undo_follow(
    db: &DatabaseConnection,
    activity: UndoFollow,
) -> Result<(), anyhow::Error> {
    let follow_ap_id = activity.object.id.as_str();

    follows::Entity::delete_many()
        .filter(follows::Column::ApId.eq(follow_ap_id))
        .exec(db)
        .await?;

    Ok(())
}

async fn handle_undo_like(
    db: &DatabaseConnection,
    activity: UndoLike,
) -> Result<(), anyhow::Error> {
    let object_str = activity.object.object.as_str();
    let actor_ap_id = activity.actor.as_str();

    let actor = ensure_remote_user(db, actor_ap_id).await?;

    if let Some(post) = posts::Entity::find()
        .filter(posts::Column::ApId.eq(object_str))
        .one(db)
        .await?
        && let Err(e) =
            crate::models::votes::VoteModel::delete_vote(db, actor.id, Some(post.id), None).await
    {
        tracing::warn!(actor_id = %actor.id, post_id = %post.id, error = %e, "failed to delete like");
    }

    Ok(())
}

async fn handle_undo_dislike(
    db: &DatabaseConnection,
    activity: UndoDislike,
) -> Result<(), anyhow::Error> {
    let object_str = activity.object.object.as_str();
    let actor_ap_id = activity.actor.as_str();

    let actor = ensure_remote_user(db, actor_ap_id).await?;

    if let Some(post) = posts::Entity::find()
        .filter(posts::Column::ApId.eq(object_str))
        .one(db)
        .await?
        && let Err(e) =
            crate::models::votes::VoteModel::delete_vote(db, actor.id, Some(post.id), None).await
    {
        tracing::warn!(actor_id = %actor.id, post_id = %post.id, error = %e, "failed to delete dislike");
    }

    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────

pub(crate) async fn ensure_remote_user(
    db: &DatabaseConnection,
    ap_id: &str,
) -> Result<users::Model, anyhow::Error> {
    if let Some(user) = users::Entity::find()
        .filter(users::Column::ApId.eq(ap_id))
        .one(db)
        .await?
    {
        return Ok(user);
    }

    let id = uuid::Uuid::new_v4();
    let new_user = users::ActiveModel {
        id: Set(id),
        username: Set(format!("remote_{}", &id.to_string()[..8])),
        email: Set(None),
        password_hash: Set(None),
        display_name: Set(Some(ap_id.to_string())),
        bio: Set(None),
        avatar_url: Set(None),
        ap_id: Set(ap_id.to_string()),
        public_key: Set(String::new()),
        private_key: Set(None),
        is_local: Set(false),
        is_admin: Set(false),
        is_banned: Set(false),
        email_verified: Set(false),
        standing_score: Set(0),
        suspended_at: Set(None),
        inbox_url: Set(format!("{}/inbox", ap_id.trim_end_matches('/'))),
        shared_inbox_url: Set(None),
        followers_url: Set(format!("{}/followers", ap_id.trim_end_matches('/'))),
        last_fetched_at: Set(None),
        created_at: Set(chrono::Utc::now().into()),
        updated_at: Set(chrono::Utc::now().into()),
    };

    Ok(new_user.insert(db).await?)
}
