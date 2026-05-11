use crate::models::_entities::{users, users::ActiveModel};
use activitypub_federation::http_signatures::generate_actor_keypair;
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use tracing::instrument;

#[derive(Debug, thiserror::Error)]
pub enum UserError {
    #[error("Invalid username: {0}")]
    InvalidUsername(String),
    #[error("Username already taken")]
    UsernameTaken,
    #[error("Email already taken")]
    EmailTaken,
    #[error("Validation error: {0}")]
    Validation(String),
    #[error(transparent)]
    Db(#[from] sea_orm::DbErr),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub struct UserModel;

impl UserModel {
    #[instrument(skip(db, password))]
    pub async fn create_local(
        db: &DatabaseConnection,
        username: &str,
        email: &str,
        password: &str,
        base_url: &str,
    ) -> Result<users::Model, UserError> {
        let username = username.trim().to_lowercase();

        if username.len() < 3 || username.len() > 30 {
            return Err(UserError::InvalidUsername(
                "Username must be between 3 and 30 characters".to_string(),
            ));
        }

        if !username.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Err(UserError::InvalidUsername(
                "Username can only contain letters, numbers, and underscores".to_string(),
            ));
        }

        let existing = users::Entity::find()
            .filter(users::Column::Username.eq(&username))
            .one(db)
            .await?;

        if existing.is_some() {
            return Err(UserError::UsernameTaken);
        }

        let existing_email = users::Entity::find()
            .filter(users::Column::Email.eq(email))
            .one(db)
            .await?;

        if existing_email.is_some() {
            return Err(UserError::EmailTaken);
        }

        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        let password_hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| UserError::Other(anyhow::anyhow!("Password hashing failed: {}", e)))?
            .to_string();

        let keypair = Self::generate_keypair()
            .map_err(|e| UserError::Other(anyhow::anyhow!("Keypair generation failed: {}", e)))?;
        let (public_key, private_key) = keypair;

        let ap_id = format!("{}/u/{}", base_url.trim_end_matches('/'), &username);
        let inbox_url = format!("{}/inbox", &ap_id);
        let followers_url = format!("{}/followers", &ap_id);

        let user = ActiveModel {
            id: Set(uuid::Uuid::new_v4()),
            username: Set(username.clone()),
            email: Set(Some(email.to_string())),
            password_hash: Set(Some(password_hash)),
            display_name: Set(Some(username.clone())),
            bio: Set(None),
            avatar_url: Set(None),
            ap_id: Set(ap_id),
            public_key: Set(public_key),
            private_key: Set(Some(private_key)),
            is_local: Set(true),
            is_admin: Set(false),
            is_banned: Set(false),
            inbox_url: Set(inbox_url),
            shared_inbox_url: Set(None),
            followers_url: Set(followers_url),
            last_fetched_at: Set(None),
            created_at: Set(chrono::Utc::now().into()),
            updated_at: Set(chrono::Utc::now().into()),
        };

        let user = user.insert(db).await?;
        Ok(user)
    }

    #[instrument(skip(db))]
    pub async fn find_by_email(
        db: &DatabaseConnection,
        email: &str,
    ) -> Result<Option<users::Model>, sea_orm::DbErr> {
        users::Entity::find()
            .filter(users::Column::Email.eq(email))
            .one(db)
            .await
    }

    #[instrument(skip(db))]
    pub async fn find_by_username(
        db: &DatabaseConnection,
        username: &str,
    ) -> Result<Option<users::Model>, sea_orm::DbErr> {
        users::Entity::find()
            .filter(users::Column::Username.eq(username))
            .one(db)
            .await
    }

    #[instrument(skip(db))]
    pub async fn find_by_id(
        db: &DatabaseConnection,
        id: uuid::Uuid,
    ) -> Result<Option<users::Model>, sea_orm::DbErr> {
        users::Entity::find()
            .filter(users::Column::Id.eq(id))
            .one(db)
            .await
    }

    #[instrument(skip(db))]
    pub async fn find_by_ap_id(
        db: &DatabaseConnection,
        ap_id: &str,
    ) -> Result<Option<users::Model>, sea_orm::DbErr> {
        users::Entity::find()
            .filter(users::Column::ApId.eq(ap_id))
            .one(db)
            .await
    }

    pub fn generate_keypair() -> Result<(String, String), anyhow::Error> {
        let keypair = generate_actor_keypair()?;
        Ok((keypair.public_key, keypair.private_key))
    }

    pub fn verify_password(model: &users::Model, password: &str) -> bool {
        match &model.password_hash {
            Some(hash) => {
                let parsed = PasswordHash::new(hash);
                match parsed {
                    Ok(parsed) => Argon2::default()
                        .verify_password(password.as_bytes(), &parsed)
                        .is_ok(),
                    Err(_) => false,
                }
            }
            None => false,
        }
    }
}
