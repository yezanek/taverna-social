pub use super::_entities::follows::{ActiveModel, Column, Entity, Model};
use sea_orm::entity::prelude::*;
use sea_orm::{
    ColumnTrait, DatabaseConnection, DbErr, EntityTrait, PaginatorTrait, QueryFilter, Set,
};
use uuid::Uuid;
pub type Follows = Entity;

#[async_trait::async_trait]
impl ActiveModelBehavior for ActiveModel {
    async fn before_save<C>(self, _db: &C, _insert: bool) -> std::result::Result<Self, DbErr>
    where
        C: ConnectionTrait,
    {
        Ok(self)
    }
}

impl Entity {
    pub async fn count_followers(db: &DatabaseConnection, user_id: Uuid) -> Result<u64, DbErr> {
        Entity::find()
            .filter(Column::FollowingId.eq(user_id))
            .filter(Column::Accepted.eq(true))
            .count(db)
            .await
    }

    pub async fn count_following(db: &DatabaseConnection, user_id: Uuid) -> Result<u64, DbErr> {
        Entity::find()
            .filter(Column::FollowerId.eq(user_id))
            .filter(Column::Accepted.eq(true))
            .filter(Column::FollowingId.is_not_null())
            .count(db)
            .await
    }

    pub async fn find_follow(
        db: &DatabaseConnection,
        follower_id: Uuid,
        following_id: Uuid,
    ) -> Result<Option<Model>, DbErr> {
        Entity::find()
            .filter(Column::FollowerId.eq(follower_id))
            .filter(Column::FollowingId.eq(following_id))
            .one(db)
            .await
    }

    pub async fn create_local_follow(
        db: &DatabaseConnection,
        follower_id: Uuid,
        following_id: Uuid,
    ) -> Result<Model, DbErr> {
        let model = ActiveModel {
            id: Set(Uuid::new_v4()),
            follower_id: Set(follower_id),
            following_id: Set(Some(following_id)),
            community_id: Set(None),
            ap_id: Set(None),
            accepted: Set(true),
            created_at: Set(chrono::Utc::now().into()),
        };
        model.insert(db).await
    }

    pub async fn delete_follow(
        db: &DatabaseConnection,
        follower_id: Uuid,
        following_id: Uuid,
    ) -> Result<(), DbErr> {
        Entity::delete_many()
            .filter(Column::FollowerId.eq(follower_id))
            .filter(Column::FollowingId.eq(following_id))
            .exec(db)
            .await?;
        Ok(())
    }
}
