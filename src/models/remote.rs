//! Remote federated content cache (Phase 8, ADR-017).
//!
//! Everything received from other instances lives in `remote_actors`,
//! `remote_posts` and `remote_comments`. Nothing in this module writes to the
//! local `users`, `posts` or `comments` tables, except bumping a local post's
//! `comment_count` when a remote reply to it arrives.
use chrono::{Duration, Utc};
use sea_orm::sea_query::{Expr, ExprTrait, OnConflict};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder,
    Set, Statement,
};
use tracing::instrument;
use url::Url;
use uuid::Uuid;

use crate::federation::protocol::{Note, RemoteActorJson};
use crate::models::_entities::{
    comments, posts, received_activities, remote_actors, remote_comments, remote_posts,
};

/// How long a fetched remote actor is considered fresh.
pub const ACTOR_TTL_DAYS: i64 = 1;
/// How long cached remote posts and comments are kept before expiry.
pub const CONTENT_TTL_DAYS: i64 = 7;
/// How long inbox idempotency records are kept.
pub const RECEIVED_ACTIVITY_TTL_DAYS: i64 = 30;

/// Host (and port, if any) of an AP id, used for `instance_domain`.
pub fn instance_domain(ap_id: &str) -> String {
    match Url::parse(ap_id) {
        Ok(u) => match (u.host_str(), u.port()) {
            (Some(h), Some(p)) => format!("{h}:{p}"),
            (Some(h), None) => h.to_string(),
            _ => String::new(),
        },
        Err(_) => String::new(),
    }
}

fn now() -> chrono::DateTime<chrono::FixedOffset> {
    Utc::now().into()
}

fn parse_published(published: Option<&str>) -> chrono::DateTime<chrono::FixedOffset> {
    published
        .and_then(|p| chrono::DateTime::parse_from_rfc3339(p).ok())
        .unwrap_or_else(now)
}

/// Where a remote comment sits in a thread.
#[derive(Debug, Clone, PartialEq)]
pub struct ThreadPosition {
    pub post_ap_id: String,
    pub local_post_id: Option<Uuid>,
}

pub struct RemoteActorModel;

impl RemoteActorModel {
    #[instrument(skip(db))]
    pub async fn find(
        db: &DatabaseConnection,
        ap_id: &str,
    ) -> Result<Option<remote_actors::Model>, DbErr> {
        remote_actors::Entity::find_by_id(ap_id.to_string())
            .one(db)
            .await
    }

    /// Inserts or refreshes a remote actor from its fetched JSON.
    #[instrument(skip(db, json, raw))]
    pub async fn upsert_from_json(
        db: &DatabaseConnection,
        json: &RemoteActorJson,
        raw: serde_json::Value,
    ) -> Result<remote_actors::Model, anyhow::Error> {
        let ap_id = json.id.as_str();
        let public_key = json
            .public_key
            .as_ref()
            .and_then(|pk| pk.get("publicKeyPem"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("Missing or empty publicKeyPem for {}", ap_id))?;
        if !public_key.starts_with("-----BEGIN") {
            anyhow::bail!("Invalid PEM format for remote actor {}", ap_id);
        }

        let username = json
            .preferred_username
            .clone()
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| {
                json.id
                    .path_segments()
                    .and_then(|mut s| s.next_back().map(str::to_string))
                    .unwrap_or_else(|| "unknown".to_string())
            });
        let avatar_url = json.icon.as_ref().and_then(|icon| match icon {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Object(o) => o.get("url").and_then(|u| u.as_str()).map(String::from),
            _ => None,
        });
        let followers_url = json
            .followers
            .as_ref()
            .map(Url::to_string)
            .unwrap_or_else(|| format!("{}/followers", ap_id.trim_end_matches('/')));
        let shared_inbox_url = json
            .endpoints
            .as_ref()
            .and_then(|e| e.shared_inbox.as_ref())
            .map(Url::to_string);

        let fetched = now();
        let model = remote_actors::ActiveModel {
            ap_id: Set(ap_id.to_string()),
            actor_type: Set(json.kind.as_db_str().to_string()),
            username: Set(username),
            display_name: Set(json.name.clone()),
            bio: Set(json.summary.clone()),
            avatar_url: Set(avatar_url),
            public_key: Set(public_key.to_string()),
            inbox_url: Set(json.inbox.to_string()),
            shared_inbox_url: Set(shared_inbox_url),
            followers_url: Set(followers_url),
            instance_domain: Set(instance_domain(ap_id)),
            raw_json: Set(raw),
            fetched_at: Set(fetched),
            stale_after: Set((Utc::now() + Duration::days(ACTOR_TTL_DAYS)).into()),
            created_at: Set(fetched),
        };

        remote_actors::Entity::insert(model)
            .on_conflict(
                OnConflict::column(remote_actors::Column::ApId)
                    .update_columns([
                        remote_actors::Column::ActorType,
                        remote_actors::Column::Username,
                        remote_actors::Column::DisplayName,
                        remote_actors::Column::Bio,
                        remote_actors::Column::AvatarUrl,
                        remote_actors::Column::PublicKey,
                        remote_actors::Column::InboxUrl,
                        remote_actors::Column::SharedInboxUrl,
                        remote_actors::Column::FollowersUrl,
                        remote_actors::Column::InstanceDomain,
                        remote_actors::Column::RawJson,
                        remote_actors::Column::FetchedAt,
                        remote_actors::Column::StaleAfter,
                    ])
                    .to_owned(),
            )
            .exec_without_returning(db)
            .await?;

        Self::find(db, ap_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("remote actor {} vanished after upsert", ap_id))
    }

    /// Returns the cached actor, inserting a placeholder if it is unknown.
    ///
    /// A placeholder has no public key and is already stale, so the next
    /// signature check for this actor re-fetches it over the network.
    #[instrument(skip(db))]
    pub async fn ensure(
        db: &DatabaseConnection,
        ap_id: &str,
    ) -> Result<remote_actors::Model, DbErr> {
        if let Some(actor) = Self::find(db, ap_id).await? {
            return Ok(actor);
        }
        let base = ap_id.trim_end_matches('/');
        let username = Url::parse(ap_id)
            .ok()
            .and_then(|u| {
                u.path_segments()
                    .and_then(|mut s| s.next_back().map(str::to_string))
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "unknown".to_string());
        let ts = now();
        let placeholder = remote_actors::ActiveModel {
            ap_id: Set(ap_id.to_string()),
            actor_type: Set("Person".to_string()),
            username: Set(username),
            display_name: Set(None),
            bio: Set(None),
            avatar_url: Set(None),
            public_key: Set(String::new()),
            inbox_url: Set(format!("{base}/inbox")),
            shared_inbox_url: Set(None),
            followers_url: Set(format!("{base}/followers")),
            instance_domain: Set(instance_domain(ap_id)),
            raw_json: Set(serde_json::json!({})),
            fetched_at: Set(chrono::DateTime::<Utc>::UNIX_EPOCH.into()),
            stale_after: Set(ts),
            created_at: Set(ts),
        };
        remote_actors::Entity::insert(placeholder)
            .on_conflict(
                OnConflict::column(remote_actors::Column::ApId)
                    .do_nothing()
                    .to_owned(),
            )
            .exec_without_returning(db)
            .await?;
        Self::find(db, ap_id)
            .await?
            .ok_or_else(|| DbErr::RecordNotFound(format!("remote actor {ap_id}")))
    }
}

pub struct RemotePostModel;

impl RemotePostModel {
    #[instrument(skip(db))]
    pub async fn find(
        db: &DatabaseConnection,
        ap_id: &str,
    ) -> Result<Option<remote_posts::Model>, DbErr> {
        remote_posts::Entity::find_by_id(ap_id.to_string())
            .one(db)
            .await
    }

    /// Stores a remote top-level Note. Returns the row and whether it was new.
    #[instrument(skip(db, note, raw))]
    pub async fn upsert(
        db: &DatabaseConnection,
        note: &Note,
        community_id: Option<Uuid>,
        raw: serde_json::Value,
    ) -> Result<(remote_posts::Model, bool), DbErr> {
        let ap_id = note.id.as_str();
        let existing = Self::find(db, ap_id).await?;
        let is_new = existing.is_none();
        let stale_after: chrono::DateTime<chrono::FixedOffset> =
            (Utc::now() + Duration::days(CONTENT_TTL_DAYS)).into();

        let model = remote_posts::ActiveModel {
            ap_id: Set(ap_id.to_string()),
            community_id: Set(community_id),
            author_ap_id: Set(note.attributed_to.to_string()),
            title: Set(note.name.clone()),
            body: Set(note.content.clone()),
            url: Set(note.url.as_ref().map(Url::to_string)),
            score: Set(existing.as_ref().map(|e| e.score).unwrap_or(0)),
            comment_count: Set(existing.as_ref().map(|e| e.comment_count).unwrap_or(0)),
            instance_domain: Set(instance_domain(ap_id)),
            raw_json: Set(raw),
            published_at: Set(parse_published(note.published.as_deref())),
            fetched_at: Set(now()),
            stale_after: Set(stale_after),
            is_deleted: Set(false),
            is_removed: Set(existing.as_ref().map(|e| e.is_removed).unwrap_or(false)),
        };
        remote_posts::Entity::insert(model)
            .on_conflict(
                OnConflict::column(remote_posts::Column::ApId)
                    .update_columns([
                        remote_posts::Column::Title,
                        remote_posts::Column::Body,
                        remote_posts::Column::Url,
                        remote_posts::Column::RawJson,
                        remote_posts::Column::FetchedAt,
                        remote_posts::Column::StaleAfter,
                    ])
                    .to_owned(),
            )
            .exec_without_returning(db)
            .await?;
        let row = Self::find(db, ap_id)
            .await?
            .ok_or_else(|| DbErr::RecordNotFound(format!("remote post {ap_id}")))?;
        Ok((row, is_new))
    }

    /// Applies an Update{Note} from the post's own author. Returns true if a row changed.
    #[instrument(skip(db, note))]
    pub async fn apply_update(
        db: &DatabaseConnection,
        actor_ap_id: &str,
        note: &Note,
    ) -> Result<bool, DbErr> {
        let mut update = remote_posts::Entity::update_many()
            .filter(remote_posts::Column::ApId.eq(note.id.as_str()))
            .filter(remote_posts::Column::AuthorApId.eq(actor_ap_id))
            .col_expr(remote_posts::Column::FetchedAt, Expr::value(now()));
        if let Some(content) = &note.content {
            update = update.col_expr(remote_posts::Column::Body, Expr::value(content.clone()));
        }
        if let Some(name) = &note.name {
            update = update.col_expr(remote_posts::Column::Title, Expr::value(name.clone()));
        }
        Ok(update.exec(db).await?.rows_affected > 0)
    }

    /// Marks a remote post deleted, only if `actor_ap_id` authored it.
    #[instrument(skip(db))]
    pub async fn mark_deleted(
        db: &DatabaseConnection,
        actor_ap_id: &str,
        ap_id: &str,
    ) -> Result<bool, DbErr> {
        let res = remote_posts::Entity::update_many()
            .filter(remote_posts::Column::ApId.eq(ap_id))
            .filter(remote_posts::Column::AuthorApId.eq(actor_ap_id))
            .col_expr(remote_posts::Column::IsDeleted, Expr::value(true))
            .exec(db)
            .await?;
        Ok(res.rows_affected > 0)
    }
}

pub struct RemoteCommentModel;

impl RemoteCommentModel {
    #[instrument(skip(db))]
    pub async fn find(
        db: &DatabaseConnection,
        ap_id: &str,
    ) -> Result<Option<remote_comments::Model>, DbErr> {
        remote_comments::Entity::find_by_id(ap_id.to_string())
            .one(db)
            .await
    }

    /// Works out which thread `in_reply_to` belongs to: a local post, a local
    /// comment, a cached remote post, or a cached remote comment.
    #[instrument(skip(db))]
    pub async fn resolve_parent(
        db: &DatabaseConnection,
        in_reply_to: &str,
    ) -> Result<Option<ThreadPosition>, DbErr> {
        if let Some(post) = posts::Entity::find()
            .filter(posts::Column::ApId.eq(in_reply_to))
            .one(db)
            .await?
        {
            return Ok(Some(ThreadPosition {
                post_ap_id: post.ap_id,
                local_post_id: Some(post.id),
            }));
        }
        if let Some(comment) = comments::Entity::find()
            .filter(comments::Column::ApId.eq(in_reply_to))
            .one(db)
            .await?
            && let Some(post) = posts::Entity::find_by_id(comment.post_id).one(db).await?
        {
            return Ok(Some(ThreadPosition {
                post_ap_id: post.ap_id,
                local_post_id: Some(post.id),
            }));
        }
        if let Some(rp) = RemotePostModel::find(db, in_reply_to).await? {
            return Ok(Some(ThreadPosition {
                post_ap_id: rp.ap_id,
                local_post_id: None,
            }));
        }
        if let Some(rc) = Self::find(db, in_reply_to).await?
            && let Some(post_ap_id) = rc.post_ap_id
        {
            return Ok(Some(ThreadPosition {
                post_ap_id,
                local_post_id: rc.local_post_id,
            }));
        }
        Ok(None)
    }

    /// Stores a remote reply. Returns the row and whether it was new.
    /// Bumps the comment count of the local or remote post it belongs to on insert.
    #[instrument(skip(db, note, raw))]
    pub async fn upsert(
        db: &DatabaseConnection,
        note: &Note,
        position: &ThreadPosition,
        raw: serde_json::Value,
    ) -> Result<(remote_comments::Model, bool), DbErr> {
        let ap_id = note.id.as_str();
        let existing = Self::find(db, ap_id).await?;
        let is_new = existing.is_none();
        let model = remote_comments::ActiveModel {
            ap_id: Set(ap_id.to_string()),
            post_ap_id: Set(Some(position.post_ap_id.clone())),
            local_post_id: Set(position.local_post_id),
            parent_ap_id: Set(note.in_reply_to.as_ref().map(Url::to_string)),
            author_ap_id: Set(note.attributed_to.to_string()),
            body: Set(note.content.clone().unwrap_or_default()),
            score: Set(existing.as_ref().map(|e| e.score).unwrap_or(0)),
            instance_domain: Set(instance_domain(ap_id)),
            raw_json: Set(raw),
            published_at: Set(parse_published(note.published.as_deref())),
            fetched_at: Set(now()),
            stale_after: Set((Utc::now() + Duration::days(CONTENT_TTL_DAYS)).into()),
            is_deleted: Set(false),
            is_removed: Set(existing.as_ref().map(|e| e.is_removed).unwrap_or(false)),
        };
        remote_comments::Entity::insert(model)
            .on_conflict(
                OnConflict::column(remote_comments::Column::ApId)
                    .update_columns([
                        remote_comments::Column::Body,
                        remote_comments::Column::RawJson,
                        remote_comments::Column::FetchedAt,
                        remote_comments::Column::StaleAfter,
                    ])
                    .to_owned(),
            )
            .exec_without_returning(db)
            .await?;

        if is_new {
            if let Some(local_post_id) = position.local_post_id {
                posts::Entity::update_many()
                    .filter(posts::Column::Id.eq(local_post_id))
                    .col_expr(
                        posts::Column::CommentCount,
                        Expr::col(posts::Column::CommentCount).add(1),
                    )
                    .exec(db)
                    .await?;
            } else {
                remote_posts::Entity::update_many()
                    .filter(remote_posts::Column::ApId.eq(position.post_ap_id.as_str()))
                    .col_expr(
                        remote_posts::Column::CommentCount,
                        Expr::col(remote_posts::Column::CommentCount).add(1),
                    )
                    .exec(db)
                    .await?;
            }
        }

        let row = Self::find(db, ap_id)
            .await?
            .ok_or_else(|| DbErr::RecordNotFound(format!("remote comment {ap_id}")))?;
        Ok((row, is_new))
    }

    /// Applies an Update{Note} from the comment's own author.
    #[instrument(skip(db, note))]
    pub async fn apply_update(
        db: &DatabaseConnection,
        actor_ap_id: &str,
        note: &Note,
    ) -> Result<bool, DbErr> {
        let Some(content) = &note.content else {
            return Ok(false);
        };
        let res = remote_comments::Entity::update_many()
            .filter(remote_comments::Column::ApId.eq(note.id.as_str()))
            .filter(remote_comments::Column::AuthorApId.eq(actor_ap_id))
            .col_expr(remote_comments::Column::Body, Expr::value(content.clone()))
            .col_expr(remote_comments::Column::FetchedAt, Expr::value(now()))
            .exec(db)
            .await?;
        Ok(res.rows_affected > 0)
    }

    /// Marks a remote comment deleted, only if `actor_ap_id` authored it.
    #[instrument(skip(db))]
    pub async fn mark_deleted(
        db: &DatabaseConnection,
        actor_ap_id: &str,
        ap_id: &str,
    ) -> Result<bool, DbErr> {
        let res = remote_comments::Entity::update_many()
            .filter(remote_comments::Column::ApId.eq(ap_id))
            .filter(remote_comments::Column::AuthorApId.eq(actor_ap_id))
            .col_expr(remote_comments::Column::IsDeleted, Expr::value(true))
            .exec(db)
            .await?;
        Ok(res.rows_affected > 0)
    }

    /// Visible remote replies to a local post, oldest first, with author info.
    #[instrument(skip(db))]
    pub async fn list_for_local_post(
        db: &DatabaseConnection,
        post_id: Uuid,
    ) -> Result<Vec<(remote_comments::Model, Option<remote_actors::Model>)>, DbErr> {
        remote_comments::Entity::find()
            .filter(remote_comments::Column::LocalPostId.eq(post_id))
            .filter(remote_comments::Column::IsDeleted.eq(false))
            .filter(remote_comments::Column::IsRemoved.eq(false))
            .order_by_asc(remote_comments::Column::PublishedAt)
            .find_also_related(remote_actors::Entity)
            .all(db)
            .await
    }
}

/// Records an incoming activity id. Returns false if it was already processed
/// (inbox idempotency, ADR-009).
#[instrument(skip(db))]
pub async fn record_received_activity(
    db: &DatabaseConnection,
    activity_ap_id: &str,
) -> Result<bool, DbErr> {
    let res = db
        .execute_raw(Statement::from_sql_and_values(
            db.get_database_backend(),
            "INSERT INTO received_activities (ap_id) VALUES ($1) ON CONFLICT (ap_id) DO NOTHING",
            [activity_ap_id.into()],
        ))
        .await?;
    Ok(res.rows_affected() == 1)
}

/// Removes `activity_ap_id` from the received set, so a retry is processed.
pub async fn forget_received_activity(
    db: &DatabaseConnection,
    activity_ap_id: &str,
) -> Result<(), DbErr> {
    received_activities::Entity::delete_many()
        .filter(received_activities::Column::ApId.eq(activity_ap_id))
        .exec(db)
        .await?;
    Ok(())
}

/// Deletes expired rows from the remote cache. Never touches local tables.
///
/// Returns `(comments, posts, actors, received_activities)` deleted.
#[instrument(skip(db))]
pub async fn expire_remote_cache(db: &DatabaseConnection) -> Result<(u64, u64, u64, u64), DbErr> {
    let cutoff = now();

    let comments_deleted = remote_comments::Entity::delete_many()
        .filter(remote_comments::Column::StaleAfter.lt(cutoff))
        .exec(db)
        .await?
        .rows_affected;

    let posts_deleted = remote_posts::Entity::delete_many()
        .filter(remote_posts::Column::StaleAfter.lt(cutoff))
        .exec(db)
        .await?
        .rows_affected;

    // Actors are shared across content; only remove ones nothing references,
    // including follows and votes (which would otherwise cascade away).
    let actors_deleted = db
        .execute_raw(Statement::from_string(
            db.get_database_backend(),
            r#"
            DELETE FROM remote_actors ra
             WHERE ra.stale_after < NOW()
               AND NOT EXISTS (SELECT 1 FROM remote_posts rp WHERE rp.author_ap_id = ra.ap_id)
               AND NOT EXISTS (SELECT 1 FROM remote_comments rc WHERE rc.author_ap_id = ra.ap_id)
               AND NOT EXISTS (SELECT 1 FROM follows f
                                WHERE f.remote_follower_ap_id = ra.ap_id
                                   OR f.remote_following_ap_id = ra.ap_id)
               AND NOT EXISTS (SELECT 1 FROM votes v WHERE v.remote_actor_ap_id = ra.ap_id)
            "#,
        ))
        .await?
        .rows_affected();

    let received_deleted = db
        .execute_raw(Statement::from_sql_and_values(
            db.get_database_backend(),
            "DELETE FROM received_activities WHERE received_at < $1",
            [(Utc::now() - Duration::days(RECEIVED_ACTIVITY_TTL_DAYS)).into()],
        ))
        .await?
        .rows_affected();

    Ok((
        comments_deleted,
        posts_deleted,
        actors_deleted,
        received_deleted,
    ))
}
