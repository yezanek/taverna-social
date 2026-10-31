//! Email verification tokens (SPEC §3.6): 24h, single use.
use crate::models::_entities::{email_verifications, users};
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set,
    TransactionTrait,
};
use uuid::Uuid;

/// How long a verification link stays valid.
pub const TOKEN_LIFETIME: chrono::Duration = chrono::Duration::hours(24);

/// Outcome of presenting a verification token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyOutcome {
    Verified,
    AlreadyVerified,
    Expired,
    Unknown,
}

/// Creates a new verification token for `user_id` and returns it.
pub async fn issue(db: &DatabaseConnection, user_id: Uuid) -> Result<String, sea_orm::DbErr> {
    let token = Uuid::new_v4().to_string();
    let now = chrono::Utc::now();
    email_verifications::ActiveModel {
        id: Set(Uuid::new_v4()),
        user_id: Set(user_id),
        token: Set(token.clone()),
        expires_at: Set((now + TOKEN_LIFETIME).into()),
        verified_at: Set(None),
        created_at: Set(now.into()),
    }
    .insert(db)
    .await?;
    Ok(token)
}

/// Consumes `token`: marks it used and the user's email verified, atomically.
pub async fn verify(db: &DatabaseConnection, token: &str) -> Result<VerifyOutcome, sea_orm::DbErr> {
    let Some(verification) = email_verifications::Entity::find()
        .filter(email_verifications::Column::Token.eq(token))
        .one(db)
        .await?
    else {
        return Ok(VerifyOutcome::Unknown);
    };
    if verification.verified_at.is_some() {
        return Ok(VerifyOutcome::AlreadyVerified);
    }
    if verification.expires_at < chrono::Utc::now() {
        return Ok(VerifyOutcome::Expired);
    }

    let now = chrono::Utc::now();
    let user_id = verification.user_id;
    let txn = db.begin().await?;
    let mut active: email_verifications::ActiveModel = verification.into();
    active.verified_at = Set(Some(now.into()));
    active.update(&txn).await?;
    users::Entity::update_many()
        .col_expr(users::Column::EmailVerified, Expr::value(true))
        .col_expr(users::Column::UpdatedAt, Expr::value(now))
        .filter(users::Column::Id.eq(user_id))
        .exec(&txn)
        .await?;
    txn.commit().await?;
    Ok(VerifyOutcome::Verified)
}
