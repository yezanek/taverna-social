use crate::models::_entities::{communities, communities::ActiveModel, community_members};
use crate::models::feed::{Cursor, Page};
use activitypub_federation::http_signatures::generate_actor_keypair;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set,
};
use tracing::instrument;
use uuid::Uuid;

/// Errors that can occur during community operations.
#[derive(Debug, thiserror::Error)]
pub enum CommunityError {
    #[error("Validation error: {0}")]
    Validation(String),
    #[error("Slug already taken")]
    SlugTaken,
    #[error("Not found")]
    NotFound,
    #[error("Key generation error: {0}")]
    Keygen(String),
    #[error(transparent)]
    Db(#[from] sea_orm::DbErr),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl From<activitypub_federation::error::Error> for CommunityError {
    fn from(e: activitypub_federation::error::Error) -> Self {
        CommunityError::Keygen(e.to_string())
    }
}

pub struct CommunityModel;

impl CommunityModel {
    /// Creates a new community with the creator as the initial moderator.
    #[instrument(skip(db))]
    pub async fn create(
        db: &DatabaseConnection,
        slug: &str,
        name: &str,
        description: Option<&str>,
        creator_id: Uuid,
        base_url: &str,
    ) -> Result<communities::Model, CommunityError> {
        let slug = slug.trim().to_lowercase();
        if slug.is_empty() || slug.len() > 50 {
            return Err(CommunityError::Validation(
                "Slug must be 1-50 characters".to_string(),
            ));
        }
        if !slug
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(CommunityError::Validation(
                "Slug can only contain letters, numbers, hyphens, and underscores".to_string(),
            ));
        }

        let keypair = generate_actor_keypair()?;
        let base = base_url.trim_end_matches('/');
        let ap_id = format!("{}/c/{}", base, slug);
        let inbox_url = format!("{}/inbox", ap_id);
        let followers_url = format!("{}/followers", ap_id);

        let community = ActiveModel {
            id: Set(Uuid::new_v4()),
            slug: Set(slug.clone()),
            name: Set(name.to_string()),
            description: Set(description.map(|s| s.to_string())),
            ap_id: Set(ap_id),
            public_key: Set(keypair.public_key),
            private_key: Set(Some(keypair.private_key)),
            inbox_url: Set(inbox_url),
            followers_url: Set(followers_url),
            is_local: Set(true),
            is_nsfw: Set(false),
            posting_restricted: Set(false),
            last_fetched_at: Set(None),
            created_at: Set(chrono::Utc::now().into()),
            updated_at: Set(chrono::Utc::now().into()),
        };

        let community = community.insert(db).await.map_err(|e| {
            let err_str = e.to_string();
            if err_str.contains("duplicate key") {
                CommunityError::SlugTaken
            } else {
                CommunityError::Db(e)
            }
        })?;

        community_members::ActiveModel {
            community_id: Set(community.id),
            user_id: Set(creator_id),
            role: Set("moderator".to_string()),
            joined_at: Set(chrono::Utc::now().into()),
        }
        .insert(db)
        .await?;

        Ok(community)
    }

    /// Finds a community by its URL slug (case-insensitive).
    #[instrument(skip(db))]
    pub async fn find_by_slug(
        db: &DatabaseConnection,
        slug: &str,
    ) -> Result<Option<communities::Model>, sea_orm::DbErr> {
        communities::Entity::find()
            .filter(communities::Column::Slug.eq(slug.to_lowercase()))
            .one(db)
            .await
    }

    /// Finds a community by its ActivityPub ID.
    #[instrument(skip(db))]
    pub async fn find_by_ap_id(
        db: &DatabaseConnection,
        ap_id: &str,
    ) -> Result<Option<communities::Model>, sea_orm::DbErr> {
        communities::Entity::find()
            .filter(communities::Column::ApId.eq(ap_id))
            .one(db)
            .await
    }

    /// Lists all communities keyset-paginated, ordered by slug.
    #[instrument(skip(db))]
    pub async fn list_all(
        db: &DatabaseConnection,
        cursor: Option<&Cursor>,
        limit: u64,
    ) -> Result<Page<communities::Model>, sea_orm::DbErr> {
        let mut query = communities::Entity::find();
        if let Some(c) = cursor {
            query = query.filter(communities::Column::Slug.gt(c.key.clone()));
        }
        let rows = query
            .order_by_asc(communities::Column::Slug)
            .limit(limit + 1)
            .all(db)
            .await?;
        Ok(Page::from_rows(rows, limit, |r| Cursor {
            score: None,
            published_at: None,
            key: r.slug.clone(),
        }))
    }

    /// Returns the role of a user in a community, if they are a member.
    #[instrument(skip(db))]
    pub async fn get_member_role(
        db: &DatabaseConnection,
        community_id: Uuid,
        user_id: Uuid,
    ) -> Result<Option<String>, sea_orm::DbErr> {
        community_members::Entity::find()
            .filter(community_members::Column::CommunityId.eq(community_id))
            .filter(community_members::Column::UserId.eq(user_id))
            .one(db)
            .await
            .map(|m| m.map(|m| m.role))
    }

    /// Adds a user as a member of a community with the given role.
    #[instrument(skip(db))]
    pub async fn add_member(
        db: &DatabaseConnection,
        community_id: Uuid,
        user_id: Uuid,
        role: &str,
    ) -> Result<(), sea_orm::DbErr> {
        community_members::ActiveModel {
            community_id: Set(community_id),
            user_id: Set(user_id),
            role: Set(role.to_string()),
            joined_at: Set(chrono::Utc::now().into()),
        }
        .insert(db)
        .await?;
        Ok(())
    }

    /// Removes a user from a community's member list.
    #[instrument(skip(db))]
    pub async fn remove_member(
        db: &DatabaseConnection,
        community_id: Uuid,
        user_id: Uuid,
    ) -> Result<(), sea_orm::DbErr> {
        community_members::Entity::delete_many()
            .filter(community_members::Column::CommunityId.eq(community_id))
            .filter(community_members::Column::UserId.eq(user_id))
            .exec(db)
            .await?;
        Ok(())
    }

    /// Soft-deletes a community by clearing its name and description.
    #[instrument(skip(db))]
    pub async fn soft_delete(
        db: &DatabaseConnection,
        community_id: Uuid,
    ) -> Result<(), sea_orm::DbErr> {
        let mut community: communities::ActiveModel = communities::Entity::find_by_id(community_id)
            .one(db)
            .await?
            .ok_or(sea_orm::DbErr::RecordNotFound(
                "Community not found".to_string(),
            ))?
            .into();
        community.name = Set("[deleted]".to_string());
        community.description = Set(Some(
            "[This community has been deleted by an admin]".to_string(),
        ));
        community.updated_at = Set(chrono::Utc::now().into());
        community.update(db).await?;
        Ok(())
    }

    const MAX_NAME_LEN: usize = 100;
    const MAX_DESCRIPTION_LEN: usize = 5000;

    /// Updates community settings (name, description, NSFW, posting restrictions).
    #[instrument(skip(db))]
    pub async fn update_settings(
        db: &DatabaseConnection,
        community_id: Uuid,
        name: Option<&str>,
        description: Option<&str>,
        is_nsfw: Option<bool>,
        posting_restricted: Option<bool>,
    ) -> Result<communities::Model, CommunityError> {
        let mut community: communities::ActiveModel = communities::Entity::find_by_id(community_id)
            .one(db)
            .await?
            .ok_or(CommunityError::NotFound)?
            .into();

        if let Some(name) = name {
            let name = name.trim();
            if name.is_empty() {
                return Err(CommunityError::Validation(
                    "Name cannot be empty".to_string(),
                ));
            }
            if name.len() > Self::MAX_NAME_LEN {
                return Err(CommunityError::Validation(format!(
                    "Name must be at most {} characters",
                    Self::MAX_NAME_LEN
                )));
            }
            community.name = Set(name.to_string());
        }
        if let Some(desc) = description {
            let desc = desc.trim();
            if desc.len() > Self::MAX_DESCRIPTION_LEN {
                return Err(CommunityError::Validation(format!(
                    "Description must be at most {} characters",
                    Self::MAX_DESCRIPTION_LEN
                )));
            }
            community.description = Set(Some(desc.to_string()));
        }
        if let Some(nsfw) = is_nsfw {
            community.is_nsfw = Set(nsfw);
        }
        if let Some(restricted) = posting_restricted {
            community.posting_restricted = Set(restricted);
        }
        community.updated_at = Set(chrono::Utc::now().into());

        Ok(community.update(db).await?)
    }
}
