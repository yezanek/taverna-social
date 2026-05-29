use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "instance_settings")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: bool,
    pub community_creation_open: bool,
    pub require_min_account_age: bool,
    pub min_account_age_days: i32,
    pub require_email_verified: bool,
    pub require_min_standing: bool,
    pub min_standing_score: i32,
    pub disqualify_if_content_removed: bool,
    pub disqualify_if_suspended: bool,
    #[sea_orm(column_type = "Text")]
    pub instance_name: String,
    pub open_registrations: bool,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
