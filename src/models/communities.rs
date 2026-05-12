use crate::models::_entities::{communities, communities::ActiveModel, community_members};
use activitypub_federation::http_signatures::generate_actor_keypair;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, Set,
};
use tracing::instrument;
use uuid::Uuid;

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
        let ap_id = format!("{}/c/{}", base, &slug);
        let inbox_url = format!("{}/inbox", &ap_id);
        let followers_url = format!("{}/followers", &ap_id);

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

    #[instrument(skip(db))]
    pub async fn list_all(
        db: &DatabaseConnection,
        page: u64,
        per_page: u64,
    ) -> Result<(Vec<communities::Model>, u64), sea_orm::DbErr> {
        let paginator = communities::Entity::find()
            .order_by_asc(communities::Column::Slug)
            .paginate(db, per_page);
        let num_pages = paginator.num_pages().await?;
        let communities = paginator.fetch_page(page).await?;
        Ok((communities, num_pages))
    }

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
            community.name = Set(name.to_string());
        }
        if let Some(desc) = description {
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
