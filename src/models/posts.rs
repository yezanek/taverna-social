use crate::models::_entities::{posts, posts::ActiveModel};
use crate::models::community_bans::Entity as CommunityBans;
use crate::models::feed::{Cursor, Page};
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set,
};
use tracing::instrument;
use url::Url;
use uuid::Uuid;

/// Errors that can occur during post operations.
#[derive(Debug, thiserror::Error)]
pub enum PostError {
    #[error("Validation error: {0}")]
    Validation(String),
    #[error("Not found")]
    NotFound,
    #[error("You are banned from this community")]
    Banned,
    #[error(transparent)]
    Db(#[from] sea_orm::DbErr),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Maximum post title length, in characters.
const MAX_TITLE_CHARS: usize = 300;

/// Standing penalty applied when a moderator removes content (SPEC §3.7).
pub const REMOVAL_STANDING_PENALTY: i32 = -5;

/// Trims `s`, treating a blank string as absent.
fn non_blank(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

/// Title, body and link of a post after trimming and validation.
struct PostContent {
    title: String,
    body: Option<String>,
    url: Option<String>,
}

impl PostContent {
    /// Trims the fields, treats blank body/URL as absent, and enforces
    /// SPEC §3.2: a 1-300 character title, an http(s) link if any, and at
    /// least one of body or link.
    fn validate(title: &str, body: Option<&str>, url: Option<&str>) -> Result<Self, PostError> {
        let title = title.trim();
        if title.is_empty() || title.chars().count() > MAX_TITLE_CHARS {
            return Err(PostError::Validation(
                "Title must be 1-300 characters".to_string(),
            ));
        }
        let body = non_blank(body);
        let url = non_blank(url);
        if let Some(u) = url {
            let parsed = Url::parse(u)
                .map_err(|_| PostError::Validation("Invalid URL format".to_string()))?;
            if !matches!(parsed.scheme(), "http" | "https") {
                return Err(PostError::Validation(
                    "URL must use http or https scheme".to_string(),
                ));
            }
        }
        if body.is_none() && url.is_none() {
            return Err(PostError::Validation(
                "Post must have body or URL".to_string(),
            ));
        }
        Ok(Self {
            title: title.to_string(),
            body: body.map(str::to_string),
            url: url.map(str::to_string),
        })
    }
}

pub struct PostModel;

impl PostModel {
    /// Creates a new post, optionally within a community.
    /// Validates title length, ensures body or URL is present, and checks
    /// that the author is not banned from the target community.
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
        let content = PostContent::validate(title, body, url)?;

        if let Some(cid) = community_id {
            let banned = CommunityBans::is_banned(db, cid, author_id).await?;
            if banned {
                return Err(PostError::Banned);
            }
        }

        let id = Uuid::new_v4();
        let base = base_url.trim_end_matches('/');
        let ap_id = format!("{}/posts/{}", base, id);

        let post = ActiveModel {
            id: Set(id),
            ap_id: Set(ap_id),
            community_id: Set(community_id),
            author_id: Set(author_id),
            title: Set(content.title),
            body: Set(content.body),
            url: Set(content.url),
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

    /// Finds a post by its UUID primary key.
    #[instrument(skip(db))]
    pub async fn find_by_id(
        db: &DatabaseConnection,
        id: Uuid,
    ) -> Result<Option<posts::Model>, sea_orm::DbErr> {
        posts::Entity::find_by_id(id).one(db).await
    }

    /// Finds a post by its ActivityPub ID.
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

    /// Lists non-deleted posts by a specific author, newest first, keyset-paginated.
    #[instrument(skip(db))]
    pub async fn list_for_author(
        db: &DatabaseConnection,
        author_id: Uuid,
        cursor: Option<&Cursor>,
        limit: u64,
    ) -> Result<Page<posts::Model>, sea_orm::DbErr> {
        let mut query = posts::Entity::find()
            .filter(posts::Column::AuthorId.eq(author_id))
            .filter(posts::Column::IsDeleted.eq(false));
        if let Some(c) = cursor
            && let (Some(published_at), Ok(id)) = (c.published_at, Uuid::parse_str(&c.key))
        {
            query = query.filter(Expr::cust_with_values(
                "(published_at, id) < ($1, $2)",
                [sea_orm::Value::from(published_at), sea_orm::Value::from(id)],
            ));
        }
        let rows = query
            .order_by_desc(posts::Column::PublishedAt)
            .order_by_desc(posts::Column::Id)
            .limit(limit + 1)
            .all(db)
            .await?;
        Ok(Page::from_rows(rows, limit, |p| Cursor {
            score: None,
            published_at: Some(p.published_at),
            key: p.id.to_string(),
        }))
    }

    /// Recalculates the score of a post from its votes, in one statement so
    /// concurrent votes cannot overwrite each other's result.
    #[instrument(skip(db))]
    pub async fn recalculate_score(
        db: &DatabaseConnection,
        post_id: Uuid,
    ) -> Result<i32, sea_orm::DbErr> {
        posts::Entity::update_many()
            .col_expr(
                posts::Column::Score,
                Expr::cust_with_values(
                    "(SELECT COALESCE(SUM(value), 0) FROM votes WHERE post_id = $1)",
                    [post_id],
                ),
            )
            .filter(posts::Column::Id.eq(post_id))
            .exec_with_returning(db)
            .await?
            .first()
            .map(|p| p.score)
            .ok_or_else(|| sea_orm::DbErr::RecordNotFound("Post not found".to_string()))
    }

    /// Marks a post as deleted (soft delete — retains the row).
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

    /// Marks a post as removed by a moderator (soft removal).
    #[instrument(skip(db))]
    pub async fn soft_remove(db: &DatabaseConnection, post_id: Uuid) -> Result<(), sea_orm::DbErr> {
        let found = posts::Entity::find_by_id(post_id)
            .one(db)
            .await?
            .ok_or(sea_orm::DbErr::RecordNotFound("Post not found".to_string()))?;
        let author_id = found.author_id;
        let mut post: posts::ActiveModel = found.into();
        post.is_removed = Set(true);
        post.updated_at = Set(chrono::Utc::now().into());
        post.update(db).await?;

        crate::models::users::UserModel::update_standing(db, author_id, REMOVAL_STANDING_PENALTY)
            .await?;

        Ok(())
    }

    /// Toggles the pinned status of a post. Returns the new pinned value.
    #[instrument(skip(db))]
    pub async fn toggle_pin(
        db: &DatabaseConnection,
        post_id: Uuid,
    ) -> Result<bool, sea_orm::DbErr> {
        let post: posts::Model = posts::Entity::find_by_id(post_id)
            .one(db)
            .await?
            .ok_or(sea_orm::DbErr::RecordNotFound("Post not found".to_string()))?;
        let new_pinned = !post.is_pinned;
        let mut active: posts::ActiveModel = post.into();
        active.is_pinned = Set(new_pinned);
        active.updated_at = Set(chrono::Utc::now().into());
        active.update(db).await?;
        Ok(new_pinned)
    }

    /// Toggles the locked status of a post (disables new comments when locked).
    #[instrument(skip(db))]
    pub async fn toggle_lock(
        db: &DatabaseConnection,
        post_id: Uuid,
    ) -> Result<bool, sea_orm::DbErr> {
        let post: posts::Model = posts::Entity::find_by_id(post_id)
            .one(db)
            .await?
            .ok_or(sea_orm::DbErr::RecordNotFound("Post not found".to_string()))?;
        let new_locked = !post.is_locked;
        let mut active: posts::ActiveModel = post.into();
        active.is_locked = Set(new_locked);
        active.updated_at = Set(chrono::Utc::now().into());
        active.update(db).await?;
        Ok(new_locked)
    }

    /// Updates a post's title, body, and URL with validation.
    #[instrument(skip(db))]
    pub async fn update(
        db: &DatabaseConnection,
        post_id: Uuid,
        title: &str,
        body: Option<&str>,
        url: Option<&str>,
    ) -> Result<posts::Model, PostError> {
        let content = PostContent::validate(title, body, url)?;

        let mut post: posts::ActiveModel = posts::Entity::find_by_id(post_id)
            .one(db)
            .await?
            .ok_or(PostError::NotFound)?
            .into();
        post.title = Set(content.title);
        post.body = Set(content.body);
        post.url = Set(content.url);
        post.updated_at = Set(chrono::Utc::now().into());
        Ok(post.update(db).await?)
    }
}
