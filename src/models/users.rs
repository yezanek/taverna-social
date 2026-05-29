use crate::models::_entities::{users, users::ActiveModel};
use activitypub_federation::http_signatures::generate_actor_keypair;
use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, Set,
};
use tracing::instrument;

/// Errors that can occur during user operations.
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
    /// Creates a new local user with hashed password, keypair, and AP IDs.
    #[instrument(skip(db, password))]
    pub async fn create_local(
        db: &DatabaseConnection,
        username: &str,
        email: &str,
        password: &str,
        base_url: &str,
    ) -> Result<users::Model, UserError> {
        let username = username.trim().to_lowercase();
        let email = email.trim().to_lowercase();

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

        let salt = SaltString::generate(&mut OsRng);
        let password = password.to_string();
        let password_hash = tokio::task::spawn_blocking(move || {
            Argon2::default()
                .hash_password(password.as_bytes(), &salt)
                .map_err(|e| UserError::Other(anyhow::anyhow!("Password hashing failed: {}", e)))
                .map(|h| h.to_string())
        })
        .await
        .map_err(|e| UserError::Other(anyhow::anyhow!("Blocking task failed: {}", e)))??;

        let keypair = Self::generate_keypair()
            .map_err(|e| UserError::Other(anyhow::anyhow!("Keypair generation failed: {}", e)))?;
        let (public_key, private_key) = keypair;

        let ap_id = format!("{}/u/{}", base_url.trim_end_matches('/'), &username);
        let inbox_url = format!("{}/inbox", &ap_id);
        let followers_url = format!("{}/followers", &ap_id);

        let user = ActiveModel {
            id: Set(uuid::Uuid::new_v4()),
            username: Set(username.clone()),
            email: Set(Some(email.clone())),
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
            email_verified: Set(false),
            standing_score: Set(0),
            suspended_at: Set(None),
            inbox_url: Set(inbox_url),
            shared_inbox_url: Set(None),
            followers_url: Set(followers_url),
            last_fetched_at: Set(None),
            created_at: Set(chrono::Utc::now().into()),
            updated_at: Set(chrono::Utc::now().into()),
        };

        let user = user.insert(db).await.map_err(|e| {
            let error_str = e.to_string();
            if error_str.contains("duplicate key value") {
                if error_str.contains("username") {
                    return UserError::UsernameTaken;
                }
                if error_str.contains("email") {
                    return UserError::EmailTaken;
                }
            }
            UserError::Db(e)
        })?;
        Ok(user)
    }

    /// Finds a user by their email address.
    #[instrument(skip(db))]
    pub async fn find_by_email(
        db: &DatabaseConnection,
        email: &str,
    ) -> Result<Option<users::Model>, sea_orm::DbErr> {
        let email = email.trim().to_lowercase();
        users::Entity::find()
            .filter(users::Column::Email.eq(&email))
            .one(db)
            .await
    }

    /// Finds a user by their username (case-insensitive lookup).
    #[instrument(skip(db))]
    pub async fn find_by_username(
        db: &DatabaseConnection,
        username: &str,
    ) -> Result<Option<users::Model>, sea_orm::DbErr> {
        let username = username.to_lowercase();
        users::Entity::find()
            .filter(users::Column::Username.eq(&username))
            .one(db)
            .await
    }

    /// Finds a user by their UUID primary key.
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

    /// Finds a user by their ActivityPub ID.
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

    /// Generates an RSA keypair for ActivityPub HTTP signatures.
    pub fn generate_keypair() -> Result<(String, String), anyhow::Error> {
        let keypair = generate_actor_keypair()?;
        Ok((keypair.public_key, keypair.private_key))
    }

    /// Updates a user's display name, bio, and avatar URL.
    #[instrument(skip(db))]
    pub async fn update_profile(
        db: &DatabaseConnection,
        user_id: uuid::Uuid,
        display_name: Option<&str>,
        bio: Option<&str>,
        avatar_url: Option<&str>,
    ) -> Result<users::Model, UserError> {
        let user = users::Entity::find_by_id(user_id)
            .one(db)
            .await?
            .ok_or(UserError::Validation("User not found".to_string()))?;

        let mut active: users::ActiveModel = user.into();
        if let Some(name) = display_name {
            let name = name.trim();
            active.display_name = Set(if name.is_empty() {
                None
            } else {
                Some(name.to_string())
            });
        }
        if let Some(b) = bio {
            let b = b.trim();
            active.bio = Set(if b.is_empty() {
                None
            } else {
                Some(b.to_string())
            });
        }
        if let Some(url) = avatar_url {
            let url = url.trim();
            if !url.is_empty() {
                let parsed = url::Url::parse(url)
                    .map_err(|_| UserError::Validation("Invalid avatar URL".to_string()))?;
                if parsed.scheme() != "https" {
                    return Err(UserError::Validation(
                        "Avatar URL must use HTTPS".to_string(),
                    ));
                }
            }
            active.avatar_url = Set(if url.is_empty() {
                None
            } else {
                Some(url.to_string())
            });
        }
        active.updated_at = Set(chrono::Utc::now().into());
        let result = active.update(db).await?;
        Ok(result)
    }

    /// Updates a user's email address, checking for duplicates.
    #[instrument(skip(db))]
    pub async fn update_email(
        db: &DatabaseConnection,
        user_id: uuid::Uuid,
        email: &str,
    ) -> Result<(), UserError> {
        let email = email.trim().to_lowercase();
        if !email.contains('@') {
            return Err(UserError::Validation("Invalid email".to_string()));
        }

        let existing = Self::find_by_email(db, &email).await?;
        if let Some(other) = existing
            && other.id != user_id
        {
            return Err(UserError::EmailTaken);
        }

        let mut user: users::ActiveModel = users::Entity::find_by_id(user_id)
            .one(db)
            .await?
            .ok_or(UserError::Validation("User not found".to_string()))?
            .into();
        user.email = Set(Some(email.to_string()));
        user.updated_at = Set(chrono::Utc::now().into());
        user.update(db).await?;
        Ok(())
    }

    /// Changes a user's password after verifying the current password.
    #[instrument(skip(db))]
    pub async fn change_password(
        db: &DatabaseConnection,
        user_id: uuid::Uuid,
        current_password: &str,
        new_password: &str,
    ) -> Result<(), UserError> {
        let user = users::Entity::find_by_id(user_id)
            .one(db)
            .await?
            .ok_or(UserError::Validation("User not found".to_string()))?;

        if !Self::verify_password(&user, current_password).await {
            return Err(UserError::Validation(
                "Current password is incorrect".to_string(),
            ));
        }

        if new_password.len() < 8 {
            return Err(UserError::Validation(
                "New password must be at least 8 characters".to_string(),
            ));
        }

        let salt = argon2::password_hash::SaltString::generate(&mut OsRng);
        let password = new_password.to_string();
        let password_hash = tokio::task::spawn_blocking(move || {
            Argon2::default()
                .hash_password(password.as_bytes(), &salt)
                .map_err(|e| UserError::Other(anyhow::anyhow!("Password hashing failed: {}", e)))
                .map(|h| h.to_string())
        })
        .await
        .map_err(|e| UserError::Other(anyhow::anyhow!("Blocking task failed: {}", e)))??;

        let mut active: users::ActiveModel = users::Entity::find_by_id(user_id)
            .one(db)
            .await?
            .ok_or(UserError::Validation("User not found".to_string()))?
            .into();
        active.password_hash = Set(Some(password_hash));
        active.updated_at = Set(chrono::Utc::now().into());
        active.update(db).await?;
        Ok(())
    }

    /// Sets or clears the banned flag on a user account.
    #[instrument(skip(db))]
    pub async fn set_banned(
        db: &DatabaseConnection,
        user_id: uuid::Uuid,
        banned: bool,
    ) -> Result<(), UserError> {
        let mut user: users::ActiveModel = users::Entity::find_by_id(user_id)
            .one(db)
            .await?
            .ok_or(UserError::Validation("User not found".to_string()))?
            .into();
        user.is_banned = Set(banned);
        user.updated_at = Set(chrono::Utc::now().into());
        user.update(db).await?;
        Ok(())
    }

    /// Atomically updates a user's standing score with a floor of -100.
    #[instrument(skip(db))]
    pub async fn update_standing(
        db: &DatabaseConnection,
        user_id: uuid::Uuid,
        delta: i32,
    ) -> Result<(), UserError> {
        use sea_orm::{ConnectionTrait, Statement};
        let stmt = Statement::from_sql_and_values(
            sea_orm::DbBackend::Postgres,
            r#"UPDATE users SET standing_score = GREATEST(-100, standing_score + $1) WHERE id = $2"#,
            vec![
                sea_orm::Value::Int(Some(delta)),
                sea_orm::Value::Uuid(Some(Box::new(user_id))),
            ],
        );
        db.execute(stmt).await?;
        Ok(())
    }

    /// Checks if a user has any post or comment removed by a moderator.
    #[instrument(skip(db))]
    pub async fn has_removed_content(
        db: &DatabaseConnection,
        user_id: uuid::Uuid,
    ) -> Result<bool, sea_orm::DbErr> {
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
        let post_removed = crate::models::_entities::posts::Entity::find()
            .filter(crate::models::_entities::posts::Column::AuthorId.eq(user_id))
            .filter(crate::models::_entities::posts::Column::IsRemoved.eq(true))
            .count(db)
            .await?
            > 0;
        if post_removed {
            return Ok(true);
        }
        let comment_removed = crate::models::_entities::comments::Entity::find()
            .filter(crate::models::_entities::comments::Column::AuthorId.eq(user_id))
            .filter(crate::models::_entities::comments::Column::IsRemoved.eq(true))
            .count(db)
            .await?
            > 0;
        Ok(comment_removed)
    }

    /// Sets or clears the suspended_at timestamp on a user.
    #[instrument(skip(db))]
    pub async fn set_suspended(
        db: &DatabaseConnection,
        user_id: uuid::Uuid,
        suspended: bool,
    ) -> Result<(), UserError> {
        let mut user: users::ActiveModel = users::Entity::find_by_id(user_id)
            .one(db)
            .await?
            .ok_or(UserError::Validation("User not found".to_string()))?
            .into();
        user.suspended_at = Set(if suspended {
            Some(chrono::Utc::now().into())
        } else {
            None
        });
        user.updated_at = Set(chrono::Utc::now().into());
        user.update(db).await?;
        Ok(())
    }

    /// Forces email verification for a user (admin action, no token required).
    #[instrument(skip(db))]
    pub async fn force_verify_email(
        db: &DatabaseConnection,
        user_id: uuid::Uuid,
    ) -> Result<(), UserError> {
        let mut user: users::ActiveModel = users::Entity::find_by_id(user_id)
            .one(db)
            .await?
            .ok_or(UserError::Validation("User not found".to_string()))?
            .into();
        user.email_verified = Set(true);
        user.updated_at = Set(chrono::Utc::now().into());
        user.update(db).await?;
        Ok(())
    }

    /// Lists all users with pagination, ordered by username.
    #[instrument(skip(db))]
    pub async fn list_all(
        db: &DatabaseConnection,
        page: u64,
        per_page: u64,
    ) -> Result<(Vec<users::Model>, u64), sea_orm::DbErr> {
        let paginator = users::Entity::find()
            .order_by_asc(users::Column::Username)
            .paginate(db, per_page);
        let num_pages = paginator.num_pages().await?;
        let users = paginator.fetch_page(page).await?;
        Ok((users, num_pages))
    }

    /// Verifies a plaintext password against the stored argon2 hash.
    pub async fn verify_password(model: &users::Model, password: &str) -> bool {
        let password_hash = match &model.password_hash {
            Some(h) => h.clone(),
            None => return false,
        };
        let password = password.to_string();
        tokio::task::spawn_blocking(move || match PasswordHash::new(&password_hash) {
            Ok(parsed) => Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok(),
            Err(_) => false,
        })
        .await
        .unwrap_or(false)
    }
}
