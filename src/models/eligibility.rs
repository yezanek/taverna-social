use crate::models::_entities::users;
use sea_orm::{DatabaseConnection, EntityTrait};
use serde::{Deserialize, Serialize};
use tracing::instrument;

/// The result of an eligibility check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EligibilityResult {
    pub eligible: bool,
    pub reasons: Vec<String>,
}

/// Checks whether a user can create a community, based on instance settings.
/// All enabled checks must pass.
#[instrument(skip(db))]
pub async fn check_eligibility(
    db: &DatabaseConnection,
    user_id: uuid::Uuid,
) -> Result<EligibilityResult, sea_orm::DbErr> {
    let settings = crate::models::instance_settings::load(db).await?;

    if settings.community_creation_open {
        return Ok(EligibilityResult {
            eligible: true,
            reasons: vec![],
        });
    }

    let user = users::Entity::find_by_id(user_id)
        .one(db)
        .await?
        .ok_or_else(|| sea_orm::DbErr::RecordNotFound("User not found".to_string()))?;

    let mut reasons: Vec<String> = Vec::new();

    if settings.require_min_account_age {
        let created_at: chrono::DateTime<chrono::Utc> = user.created_at.into();
        let account_age = (chrono::Utc::now().signed_duration_since(created_at)).num_days() as i32;
        if account_age < settings.min_account_age_days {
            reasons.push(format!(
                "Account age ({}) is below minimum ({})",
                account_age, settings.min_account_age_days
            ));
        }
    }

    if settings.require_email_verified && !user.email_verified {
        reasons.push("Email not verified".to_string());
    }

    if settings.require_min_standing && user.standing_score < settings.min_standing_score {
        reasons.push(format!(
            "Standing score ({}) is below minimum ({})",
            user.standing_score, settings.min_standing_score
        ));
    }

    if settings.disqualify_if_content_removed
        && crate::models::users::UserModel::has_removed_content(db, user_id).await?
    {
        reasons.push("User has removed content".to_string());
    }

    if settings.disqualify_if_suspended && user.suspended_at.is_some() {
        reasons.push("Account is suspended".to_string());
    }

    let eligible = reasons.is_empty();

    Ok(EligibilityResult { eligible, reasons })
}
