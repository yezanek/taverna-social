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

/// Errors from casting or removing a vote.
#[derive(Debug, thiserror::Error)]
pub enum VoteError {
    #[error("Vote must be 1 or -1")]
    InvalidValue,
    #[error("Users cannot vote on their own content")]
    OwnContent,
    #[error("Vote target not found")]
    NotFound,
    #[error(transparent)]
    Db(#[from] sea_orm::DbErr),
}

pub struct VoteModel;

impl VoteModel {
    /// Deletes a remote actor's vote (Undo{Like}/Undo{Dislike}).
    #[instrument(skip(db))]
    pub async fn delete_remote_vote(
        db: &DatabaseConnection,
        actor_ap_id: &str,
        post_id: Option<Uuid>,
        comment_id: Option<Uuid>,
    ) -> Result<(), VoteError> {
        Self::delete_by(db, Voter::Remote(actor_ap_id), post_id, comment_id).await
    }

    /// Casts or toggles a vote (upvote/downvote) on a post or comment.
    /// Returns whether the vote was Added, Changed, or Removed. Users cannot
    /// vote on their own posts or comments (SPEC §3.2).
    #[instrument(skip(db))]
    pub async fn cast_vote(
        db: &DatabaseConnection,
        user_id: Uuid,
        post_id: Option<Uuid>,
        comment_id: Option<Uuid>,
        value: i16,
    ) -> Result<VoteAction, VoteError> {
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
    ) -> Result<VoteAction, VoteError> {
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
    ) -> Result<Option<votes::Model>, VoteError> {
        let query = votes::Entity::find().filter(voter.filter());
        let query = match (post_id, comment_id) {
            (Some(pid), None) => query.filter(votes::Column::PostId.eq(pid)),
            (None, Some(cid)) => query.filter(votes::Column::CommentId.eq(cid)),
            _ => {
                return Err(sea_orm::DbErr::Custom(
                    "Exactly one of post_id or comment_id must be set".to_string(),
                )
                .into());
            }
        };
        Ok(query.one(db).await?)
    }

    /// The author of the voted-on post or comment.
    async fn target_author(
        db: &DatabaseConnection,
        post_id: Option<Uuid>,
        comment_id: Option<Uuid>,
    ) -> Result<Uuid, VoteError> {
        let author = match (post_id, comment_id) {
            (Some(pid), None) => crate::models::posts::PostModel::find_by_id(db, pid)
                .await?
                .map(|p| p.author_id),
            (None, Some(cid)) => crate::models::comments::CommentModel::find_by_id(db, cid)
                .await?
                .map(|c| c.author_id),
            _ => None,
        };
        author.ok_or(VoteError::NotFound)
    }

    async fn delete_by(
        db: &DatabaseConnection,
        voter: Voter<'_>,
        post_id: Option<Uuid>,
        comment_id: Option<Uuid>,
    ) -> Result<(), VoteError> {
        if let Some(vote) = Self::find_existing(db, voter, post_id, comment_id).await? {
            let author_id = Self::target_author(db, post_id, comment_id).await?;
            votes::Entity::delete_by_id(vote.id).exec(db).await?;
            Self::recalculate_after_vote(db, post_id, comment_id).await?;
            // SPEC §3.7: standing changes are reversed when a vote is removed.
            Self::adjust_standing(db, voter, author_id, -i32::from(vote.value)).await?;
        }
        Ok(())
    }

    /// Applies a vote's effect on the author's standing score. Votes a user
    /// casts on their own content never count.
    async fn adjust_standing(
        db: &DatabaseConnection,
        voter: Voter<'_>,
        author_id: Uuid,
        delta: i32,
    ) -> Result<(), VoteError> {
        if delta != 0 && !voter.is_local_user(author_id) {
            crate::models::users::UserModel::update_standing(db, author_id, delta).await?;
        }
        Ok(())
    }

    async fn cast_by(
        db: &DatabaseConnection,
        voter: Voter<'_>,
        post_id: Option<Uuid>,
        comment_id: Option<Uuid>,
        value: i16,
    ) -> Result<VoteAction, VoteError> {
        if value != 1 && value != -1 {
            return Err(VoteError::InvalidValue);
        }
        let author_id = Self::target_author(db, post_id, comment_id).await?;
        if voter.is_local_user(author_id) {
            return Err(VoteError::OwnContent);
        }

        let (result, delta) = match Self::find_existing(db, voter, post_id, comment_id).await? {
            Some(existing) if existing.value == value => {
                votes::Entity::delete_by_id(existing.id).exec(db).await?;
                (VoteAction::Removed, -i32::from(value))
            }
            Some(existing) => {
                let delta = i32::from(value - existing.value);
                let mut active: votes::ActiveModel = existing.into();
                active.value = Set(value);
                active.update(db).await?;
                (VoteAction::Changed, delta)
            }
            None => {
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
                (VoteAction::Added, i32::from(value))
            }
        };

        Self::recalculate_after_vote(db, post_id, comment_id).await?;
        Self::adjust_standing(db, voter, author_id, delta).await?;
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
