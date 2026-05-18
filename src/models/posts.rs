use crate::models::_entities::{posts, posts::ActiveModel, votes};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, Set,
};
use tracing::instrument;
use url::Url;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum PostError {
    #[error("Validation error: {0}")]
    Validation(String),
    #[error("Not found")]
    NotFound,
    #[error(transparent)]
    Db(#[from] sea_orm::DbErr),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub struct PostModel;

impl PostModel {
    #[instrument(skip(db))]
    pub async fn create(
        db: &DatabaseConnection,
        community_id: Option<Uuid>,
        author_id: Uuid,
        title: &str,
        body: Option<&str>,
        url: Option<&str>,
        base_url: &str,
    ) -> Result<posts::Model, PostError> {
        let title = title.trim();
        if title.is_empty() || title.len() > 300 {
            return Err(PostError::Validation(
                "Title must be 1-300 characters".to_string(),
            ));
        }
        let body_effective = body.map(|s| s.trim()).filter(|s| !s.is_empty());
        let url_effective = url.map(|s| s.trim()).filter(|s| !s.is_empty());
        if let Some(u) = &url_effective {
            let parsed = Url::parse(u)
                .map_err(|_| PostError::Validation("Invalid URL format".to_string()))?;
            let scheme = parsed.scheme();
            if scheme != "http" && scheme != "https" {
                return Err(PostError::Validation(
                    "URL must use http or https scheme".to_string(),
                ));
            }
        }
        if body_effective.is_none() && url_effective.is_none() {
            return Err(PostError::Validation(
                "Post must have body or URL".to_string(),
            ));
        }

        let id = Uuid::new_v4();
        let base = base_url.trim_end_matches('/');
        let ap_id = format!("{}/posts/{}", base, id);

        let post = ActiveModel {
            id: Set(id),
            ap_id: Set(ap_id),
            community_id: Set(community_id),
            author_id: Set(author_id),
            title: Set(title.to_string()),
            body: Set(body.map(|s| s.to_string())),
            url: Set(url.map(|s| s.to_string())),
            score: Set(0),
            comment_count: Set(0),
            is_deleted: Set(false),
            is_removed: Set(false),
            is_locked: Set(false),
            is_pinned: Set(false),
            published_at: Set(chrono::Utc::now().into()),
            created_at: Set(chrono::Utc::now().into()),
            updated_at: Set(chrono::Utc::now().into()),
        };

        Ok(post.insert(db).await?)
    }

    #[instrument(skip(db))]
    pub async fn find_by_id(
        db: &DatabaseConnection,
        id: Uuid,
    ) -> Result<Option<posts::Model>, sea_orm::DbErr> {
        posts::Entity::find_by_id(id).one(db).await
    }

    #[instrument(skip(db))]
    pub async fn find_by_ap_id(
        db: &DatabaseConnection,
        ap_id: &str,
    ) -> Result<Option<posts::Model>, sea_orm::DbErr> {
        posts::Entity::find()
            .filter(posts::Column::ApId.eq(ap_id))
            .one(db)
            .await
    }

    #[instrument(skip(db))]
    pub async fn list_for_community(
        db: &DatabaseConnection,
        community_id: Uuid,
        page: u64,
        per_page: u64,
    ) -> Result<(Vec<posts::Model>, u64), sea_orm::DbErr> {
        let paginator = posts::Entity::find()
            .filter(posts::Column::CommunityId.eq(community_id))
            .filter(posts::Column::IsDeleted.eq(false))
            .order_by_desc(posts::Column::Score)
            .order_by_desc(posts::Column::PublishedAt)
            .paginate(db, per_page);
        let num_pages = paginator.num_pages().await?;
        let posts = paginator.fetch_page(page).await?;
        Ok((posts, num_pages))
    }

    #[instrument(skip(db))]
    pub async fn list_for_author(
        db: &DatabaseConnection,
        author_id: Uuid,
        page: u64,
        per_page: u64,
    ) -> Result<(Vec<posts::Model>, u64), sea_orm::DbErr> {
        let paginator = posts::Entity::find()
            .filter(posts::Column::AuthorId.eq(author_id))
            .filter(posts::Column::IsDeleted.eq(false))
            .order_by_desc(posts::Column::PublishedAt)
            .paginate(db, per_page);
        let num_pages = paginator.num_pages().await?;
        let posts = paginator.fetch_page(page).await?;
        Ok((posts, num_pages))
    }

    #[instrument(skip(db))]
    pub async fn list_all(
        db: &DatabaseConnection,
        page: u64,
        per_page: u64,
    ) -> Result<(Vec<posts::Model>, u64), sea_orm::DbErr> {
        let paginator = posts::Entity::find()
            .filter(posts::Column::IsDeleted.eq(false))
            .order_by_desc(posts::Column::Score)
            .order_by_desc(posts::Column::PublishedAt)
            .paginate(db, per_page);
        let num_pages = paginator.num_pages().await?;
        let posts = paginator.fetch_page(page).await?;
        Ok((posts, num_pages))
    }

    #[instrument(skip(db))]
    pub async fn recalculate_score(
        db: &DatabaseConnection,
        post_id: Uuid,
    ) -> Result<i32, sea_orm::DbErr> {
        let all_votes = votes::Entity::find()
            .filter(votes::Column::PostId.eq(post_id))
            .all(db)
            .await?;
        let score: i32 = all_votes.iter().map(|v| v.value as i32).sum();

        let mut post: posts::ActiveModel = posts::Entity::find_by_id(post_id)
            .one(db)
            .await?
            .ok_or(sea_orm::DbErr::RecordNotFound("Post not found".to_string()))?
            .into();
        post.score = Set(score);
        post.update(db).await?;

        Ok(score)
    }

    #[instrument(skip(db))]
    pub async fn soft_delete(db: &DatabaseConnection, post_id: Uuid) -> Result<(), sea_orm::DbErr> {
        let mut post: posts::ActiveModel = posts::Entity::find_by_id(post_id)
            .one(db)
            .await?
            .ok_or(sea_orm::DbErr::RecordNotFound("Post not found".to_string()))?
            .into();
        post.is_deleted = Set(true);
        post.updated_at = Set(chrono::Utc::now().into());
        post.update(db).await?;
        Ok(())
    }

    #[instrument(skip(db))]
    pub async fn soft_remove(db: &DatabaseConnection, post_id: Uuid) -> Result<(), sea_orm::DbErr> {
        let mut post: posts::ActiveModel = posts::Entity::find_by_id(post_id)
            .one(db)
            .await?
            .ok_or(sea_orm::DbErr::RecordNotFound("Post not found".to_string()))?
            .into();
        post.is_removed = Set(true);
        post.updated_at = Set(chrono::Utc::now().into());
        post.update(db).await?;
        Ok(())
    }

    #[instrument(skip(db))]
    pub async fn toggle_pin(
        db: &DatabaseConnection,
        post_id: Uuid,
    ) -> Result<bool, sea_orm::DbErr> {
        let post: posts::Model = posts::Entity::find_by_id(post_id)
            .one(db)
            .await?
            .ok_or(sea_orm::DbErr::RecordNotFound("Post not found".to_string()))?;
        let mut active: posts::ActiveModel = post.into();
        let new_pinned = !active.is_pinned.take().unwrap_or(false);
        active.is_pinned = Set(new_pinned);
        active.updated_at = Set(chrono::Utc::now().into());
        active.update(db).await?;
        Ok(new_pinned)
    }

    #[instrument(skip(db))]
    pub async fn toggle_lock(
        db: &DatabaseConnection,
        post_id: Uuid,
    ) -> Result<bool, sea_orm::DbErr> {
        let post: posts::Model = posts::Entity::find_by_id(post_id)
            .one(db)
            .await?
            .ok_or(sea_orm::DbErr::RecordNotFound("Post not found".to_string()))?;
        let mut active: posts::ActiveModel = post.into();
        let new_locked = !active.is_locked.take().unwrap_or(false);
        active.is_locked = Set(new_locked);
        active.updated_at = Set(chrono::Utc::now().into());
        active.update(db).await?;
        Ok(new_locked)
    }

    #[instrument(skip(db))]
    pub async fn increment_comment_count(
        db: &DatabaseConnection,
        post_id: Uuid,
    ) -> Result<(), sea_orm::DbErr> {
        let mut post: posts::ActiveModel = posts::Entity::find_by_id(post_id)
            .one(db)
            .await?
            .ok_or(sea_orm::DbErr::RecordNotFound("Post not found".to_string()))?
            .into();
        let current_count = post
            .comment_count
            .take()
            .ok_or(sea_orm::DbErr::RecordNotFound(
                "Comment count missing".to_string(),
            ))?;
        post.comment_count = Set(current_count + 1);
        post.updated_at = Set(chrono::Utc::now().into());
        post.update(db).await?;
        Ok(())
    }
}
