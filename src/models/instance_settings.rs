//! The single-row `instance_settings` table (SPEC §4.11).
use crate::models::_entities::instance_settings;
use sea_orm::{DatabaseConnection, EntityTrait};

/// Defaults matching the column defaults of `instance_settings`, used when
/// the row is missing (it is inserted by migration, so this is a fallback).
#[must_use]
pub fn defaults() -> instance_settings::Model {
    instance_settings::Model {
        id: true,
        community_creation_open: true,
        require_min_account_age: false,
        min_account_age_days: 30,
        require_email_verified: false,
        require_min_standing: false,
        min_standing_score: 0,
        disqualify_if_content_removed: false,
        disqualify_if_suspended: false,
        instance_name: "Taverna".to_string(),
        open_registrations: true,
        updated_at: chrono::Utc::now().into(),
    }
}

/// Loads the instance settings, falling back to [`defaults`].
pub async fn load(db: &DatabaseConnection) -> Result<instance_settings::Model, sea_orm::DbErr> {
    Ok(instance_settings::Entity::find()
        .one(db)
        .await?
        .unwrap_or_else(defaults))
}
