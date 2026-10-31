//! `SeaORM` Entity for the `remote_actors` cache table (Phase 8).

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "remote_actors")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false, column_type = "Text")]
    pub ap_id: String,
    #[sea_orm(column_type = "Text")]
    pub actor_type: String,
    #[sea_orm(column_type = "Text")]
    pub username: String,
    #[sea_orm(column_type = "Text", nullable)]
    pub display_name: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    pub bio: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    pub avatar_url: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub public_key: String,
    #[sea_orm(column_type = "Text")]
    pub inbox_url: String,
    #[sea_orm(column_type = "Text", nullable)]
    pub shared_inbox_url: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub followers_url: String,
    #[sea_orm(column_type = "Text")]
    pub instance_domain: String,
    #[sea_orm(column_type = "JsonBinary")]
    pub raw_json: Json,
    pub fetched_at: DateTimeWithTimeZone,
    pub stale_after: DateTimeWithTimeZone,
    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::remote_posts::Entity")]
    RemotePosts,
    #[sea_orm(has_many = "super::remote_comments::Entity")]
    RemoteComments,
}

impl Related<super::remote_posts::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::RemotePosts.def()
    }
}

impl Related<super::remote_comments::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::RemoteComments.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
