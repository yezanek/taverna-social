use crate::models::_entities::votes;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use uuid::Uuid;

/// Describes the result of a vote operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum VoteAction {
    Added,
    Changed,
    Removed,
}

/// Who cast a vote: a local user or a remote actor (Phase 8).
#[derive(Debug, Clone, Copy)]
enum Voter<'a> {
    Local(Uuid),
    Remote(&'a str),
}

impl Voter<'_> {
    fn filter(&self) -> sea_orm::sea_query::SimpleExpr {
        match self {
            Voter::Local(id) => votes::Column::UserId.eq(*id),
            Voter::Remote(ap_id) => votes::Column::RemoteActorApId.eq(*ap_id),
        }
    }

    fn is_local_user(&self, user_id: Uuid) -> bool {
        matches!(self, Voter::Local(id) if *id == user_id)
    }
}

pub struct VoteModel;

impl VoteModel {
    /// Deletes a user's vote on a post or comment.
    #[instrument(skip(db))]
    pub async fn delete_vote(
        db: &DatabaseConnection,
        user_id: Uuid,
        post_id: Option<Uuid>,
        comment_id: Option<Uuid>,
    ) -> Result<(), sea_orm::DbErr> {
        Self::delete_by(db, Voter::Local(user_id), post_id, comment_id).await
    }

    /// Deletes a remote actor's vote (Undo{Like}/Undo{Dislike}).
    #[instrument(skip(db))]
    pub async fn delete_remote_vote(
        db: &DatabaseConnection,
        actor_ap_id: &str,
        post_id: Option<Uuid>,
        comment_id: Option<Uuid>,
    ) -> Result<(), sea_orm::DbErr> {
        Self::delete_by(db, Voter::Remote(actor_ap_id), post_id, comment_id).await
    }

    /// Casts or toggles a vote (upvote/downvote) on a post or comment.
    /// Returns whether the vote was Added, Changed, or Removed.
    #[instrument(skip(db))]
    pub async fn cast_vote(
        db: &DatabaseConnection,
        user_id: Uuid,
        post_id: Option<Uuid>,
        comment_id: Option<Uuid>,
        value: i16,
    ) -> Result<VoteAction, sea_orm::DbErr> {
        Self::cast_by(db, Voter::Local(user_id), post_id, comment_id, value).await
    }

    /// Records a remote actor's Like (+1) or Dislike (-1) on local content.
    /// Unlike local voting, repeating the same vote does not toggle it off.
    #[instrument(skip(db))]
    pub async fn cast_remote_vote(
        db: &DatabaseConnection,
        actor_ap_id: &str,
        post_id: Option<Uuid>,
        comment_id: Option<Uuid>,
        value: i16,
    ) -> Result<VoteAction, sea_orm::DbErr> {
        let voter = Voter::Remote(actor_ap_id);
        if Self::find_existing(db, voter, post_id, comment_id)
            .await?
            .is_some_and(|v| v.value == value)
        {
            return Ok(VoteAction::Changed);
        }
        Self::cast_by(db, voter, post_id, comment_id, value).await
    }

    async fn find_existing(
        db: &DatabaseConnection,
        voter: Voter<'_>,
        post_id: Option<Uuid>,
        comment_id: Option<Uuid>,
    ) -> Result<Option<votes::Model>, sea_orm::DbErr> {
        let query = votes::Entity::find().filter(voter.filter());
        match (post_id, comment_id) {
            (Some(pid), None) => query.filter(votes::Column::PostId.eq(pid)).one(db).await,
            (None, Some(cid)) => query.filter(votes::Column::CommentId.eq(cid)).one(db).await,
            _ => Err(sea_orm::DbErr::Custom(
                "Exactly one of post_id or comment_id must be set".to_string(),
            )),
        }
    }

    async fn delete_by(
        db: &DatabaseConnection,
        voter: Voter<'_>,
        post_id: Option<Uuid>,
        comment_id: Option<Uuid>,
    ) -> Result<(), sea_orm::DbErr> {
        if let Some(vote) = Self::find_existing(db, voter, post_id, comment_id).await? {
            votes::Entity::delete_by_id(vote.id).exec(db).await?;
            Self::recalculate_after_vote(db, post_id, comment_id).await?;
        }
        Ok(())
    }

    async fn cast_by(
        db: &DatabaseConnection,
        voter: Voter<'_>,
        post_id: Option<Uuid>,
        comment_id: Option<Uuid>,
        value: i16,
    ) -> Result<VoteAction, sea_orm::DbErr> {
        if value != 1 && value != -1 {
            return Err(sea_orm::DbErr::Custom("Vote must be 1 or -1".to_string()));
        }
        let existing = Self::find_existing(db, voter, post_id, comment_id).await?;

        let author_id = if let Some(pid) = post_id {
            crate::models::posts::PostModel::find_by_id(db, pid)
                .await
                .ok()
                .flatten()
                .map(|p| p.author_id)
        } else if let Some(cid) = comment_id {
            crate::models::comments::CommentModel::find_by_id(db, cid)
                .await
                .ok()
                .flatten()
                .map(|c| c.author_id)
        } else {
            None
        };

        let (result, old_val) = if let Some(existing_vote) = existing {
            let old = existing_vote.value;
            if old == value {
                votes::Entity::delete_by_id(existing_vote.id)
                    .exec(db)
                    .await?;
                Self::recalculate_after_vote(db, post_id, comment_id).await?;
                (VoteAction::Removed, old)
            } else {
                let mut active: votes::ActiveModel = existing_vote.into();
                active.value = Set(value);
                active.update(db).await?;
                Self::recalculate_after_vote(db, post_id, comment_id).await?;
                (VoteAction::Changed, old)
            }
        } else {
            let (user_id, remote_actor_ap_id) = match voter {
                Voter::Local(id) => (Some(id), None),
                Voter::Remote(ap_id) => (None, Some(ap_id.to_string())),
            };
            votes::ActiveModel {
                id: Set(Uuid::new_v4()),
                user_id: Set(user_id),
                remote_actor_ap_id: Set(remote_actor_ap_id),
                post_id: Set(post_id),
                comment_id: Set(comment_id),
                value: Set(value),
                created_at: Set(chrono::Utc::now().into()),
            }
            .insert(db)
            .await?;

            Self::recalculate_after_vote(db, post_id, comment_id).await?;
            (VoteAction::Added, value)
        };

        if let Some(aid) = author_id
            && !voter.is_local_user(aid)
        {
            let delta = match result {
                VoteAction::Added => value as i32,
                VoteAction::Changed => (value - old_val) as i32,
                VoteAction::Removed => -(old_val as i32),
            };
            let _ = crate::models::users::UserModel::update_standing(db, aid, delta).await;
        }

        Ok(result)
    }

    async fn recalculate_after_vote(
        db: &DatabaseConnection,
        post_id: Option<Uuid>,
        comment_id: Option<Uuid>,
    ) -> Result<(), sea_orm::DbErr> {
        if let Some(pid) = post_id {
            crate::models::posts::PostModel::recalculate_score(db, pid).await?;
        } else if let Some(cid) = comment_id {
            crate::models::comments::CommentModel::recalculate_score(db, cid).await?;
        }
        Ok(())
    }

    /// Returns the current user's vote value on a post, if any.
    #[instrument(skip(db))]
    pub async fn get_user_vote_on_post(
        db: &DatabaseConnection,
        user_id: Uuid,
        post_id: Uuid,
    ) -> Result<Option<i16>, sea_orm::DbErr> {
        votes::Entity::find()
            .filter(votes::Column::UserId.eq(user_id))
            .filter(votes::Column::PostId.eq(post_id))
            .one(db)
            .await
            .map(|v| v.map(|v| v.value))
    }

    /// Returns the current user's vote value on a comment, if any.
    #[instrument(skip(db))]
    pub async fn get_user_vote_on_comment(
        db: &DatabaseConnection,
        user_id: Uuid,
        comment_id: Uuid,
    ) -> Result<Option<i16>, sea_orm::DbErr> {
        votes::Entity::find()
            .filter(votes::Column::UserId.eq(user_id))
            .filter(votes::Column::CommentId.eq(comment_id))
            .one(db)
            .await
            .map(|v| v.map(|v| v.value))
    }
}
