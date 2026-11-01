pub use super::_entities::community_bans::{ActiveModel, Column, Entity, Model};
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::OnConflict;
use sea_orm::{ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder, Set};

use super::_entities::users;
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

    /// Lists a community's bans, newest first, with each banned username.
    pub async fn list_for_community(
        db: &DatabaseConnection,
        community_id: Uuid,
    ) -> Result<Vec<(Model, String)>, DbErr> {
        let rows = Entity::find()
            .filter(Column::CommunityId.eq(community_id))
            .find_also_related(users::Entity)
            .order_by_desc(Column::CreatedAt)
            .all(db)
            .await?;
        Ok(rows
            .into_iter()
            .filter_map(|(ban, user)| user.map(|u| (ban, u.username)))
            .collect())
    }

    /// Bans a user from a community, recording the moderator who issued it.
    /// Banning an already banned user updates the reason and moderator.
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
        .on_conflict(
            OnConflict::columns([Column::CommunityId, Column::UserId])
                .update_columns([Column::BannedBy, Column::Reason])
                .to_owned(),
        )
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
