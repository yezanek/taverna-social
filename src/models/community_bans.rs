pub use super::_entities::community_bans::{ActiveModel, Column, Entity, Model};
use sea_orm::entity::prelude::*;
use sea_orm::{ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, Set};
pub type CommunityBans = Entity;

impl Model {}

impl Entity {
    /// Checks whether a user is currently banned from a community.
    pub async fn is_banned(
        db: &DatabaseConnection,
        community_id: Uuid,
        user_id: Uuid,
    ) -> Result<bool, DbErr> {
        let count = Entity::find()
            .filter(Column::CommunityId.eq(community_id))
            .filter(Column::UserId.eq(user_id))
            .count(db)
            .await?;
        Ok(count > 0)
    }

    /// Bans a user from a community, recording the moderator who issued it.
    pub async fn create_ban(
        db: &DatabaseConnection,
        community_id: Uuid,
        user_id: Uuid,
        banned_by: Uuid,
        reason: Option<&str>,
    ) -> Result<(), DbErr> {
        Entity::insert(ActiveModel {
            community_id: Set(community_id),
            user_id: Set(user_id),
            banned_by: Set(banned_by),
            reason: Set(reason.map(|s| s.to_string())),
            created_at: Set(chrono::Utc::now().into()),
        })
        .exec(db)
        .await?;
        Ok(())
    }

    /// Removes a ban, allowing the user to rejoin the community.
    pub async fn remove_ban(
        db: &DatabaseConnection,
        community_id: Uuid,
        user_id: Uuid,
    ) -> Result<(), DbErr> {
        Entity::delete_many()
            .filter(Column::CommunityId.eq(community_id))
            .filter(Column::UserId.eq(user_id))
            .exec(db)
            .await?;
        Ok(())
    }
}
