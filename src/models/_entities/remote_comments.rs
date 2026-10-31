//! `SeaORM` Entity for the `remote_comments` cache table (Phase 8).

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "remote_comments")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false, column_type = "Text")]
    pub ap_id: String,
    #[sea_orm(column_type = "Text", nullable)]
    pub post_ap_id: Option<String>,
    pub local_post_id: Option<Uuid>,
    #[sea_orm(column_type = "Text", nullable)]
    pub parent_ap_id: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub author_ap_id: String,
    #[sea_orm(column_type = "Text")]
    pub body: String,
    pub score: i32,
    #[sea_orm(column_type = "Text")]
    pub instance_domain: String,
    #[sea_orm(column_type = "JsonBinary")]
    pub raw_json: Json,
    pub published_at: DateTimeWithTimeZone,
    pub fetched_at: DateTimeWithTimeZone,
    pub stale_after: DateTimeWithTimeZone,
    pub is_deleted: bool,
    pub is_removed: bool,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::remote_actors::Entity",
        from = "Column::AuthorApId",
        to = "super::remote_actors::Column::ApId",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    RemoteActors,
    #[sea_orm(
        belongs_to = "super::posts::Entity",
        from = "Column::LocalPostId",
        to = "super::posts::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    Posts,
}

impl Related<super::remote_actors::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::RemoteActors.def()
    }
}

impl Related<super::posts::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Posts.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
