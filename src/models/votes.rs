use crate::models::_entities::votes;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum VoteAction {
    Added,
    Changed,
    Removed,
}

pub struct VoteModel;

impl VoteModel {
    #[instrument(skip(db))]
    pub async fn cast_vote(
        db: &DatabaseConnection,
        user_id: Uuid,
        post_id: Option<Uuid>,
        comment_id: Option<Uuid>,
        value: i16,
    ) -> Result<VoteAction, sea_orm::DbErr> {
        if value != 1 && value != -1 {
            return Err(sea_orm::DbErr::Custom("Vote must be 1 or -1".to_string()));
        }
        if post_id.is_some() == comment_id.is_some() {
            return Err(sea_orm::DbErr::Custom(
                "Exactly one of post_id or comment_id must be set".to_string(),
            ));
        }

        let existing = if let Some(pid) = post_id {
            votes::Entity::find()
                .filter(votes::Column::UserId.eq(user_id))
                .filter(votes::Column::PostId.eq(pid))
                .one(db)
                .await?
        } else {
            let cid = comment_id.unwrap();
            votes::Entity::find()
                .filter(votes::Column::UserId.eq(user_id))
                .filter(votes::Column::CommentId.eq(cid))
                .one(db)
                .await?
        };

        if let Some(existing_vote) = existing {
            if existing_vote.value == value {
                votes::Entity::delete_by_id(existing_vote.id)
                    .exec(db)
                    .await?;
                Self::recalculate_after_vote(db, post_id, comment_id).await?;
                return Ok(VoteAction::Removed);
            } else {
                let mut active: votes::ActiveModel = existing_vote.into();
                active.value = Set(value);
                active.update(db).await?;
                Self::recalculate_after_vote(db, post_id, comment_id).await?;
                return Ok(VoteAction::Changed);
            }
        }

        votes::ActiveModel {
            id: Set(Uuid::new_v4()),
            user_id: Set(user_id),
            post_id: Set(post_id),
            comment_id: Set(comment_id),
            value: Set(value),
            created_at: Set(chrono::Utc::now().into()),
        }
        .insert(db)
        .await?;

        Self::recalculate_after_vote(db, post_id, comment_id).await?;
        Ok(VoteAction::Added)
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
