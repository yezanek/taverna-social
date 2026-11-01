use crate::models::_entities::{comments, communities, posts};
use crate::models::feed::{Cursor, Page};
use sea_orm::sea_query::{Expr, ExprTrait, Value};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DbBackend, EntityTrait,
    QueryFilter, QueryOrder, QueryResult, QuerySelect, Set, Statement, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::instrument;
use uuid::Uuid;

const COMMENT_SELECT: &str = "SELECT id, ap_id, post_id, author_id, parent_id, CAST(path AS TEXT) AS path, body, score, is_deleted, is_removed, published_at, created_at, updated_at FROM comments";

fn row_to_comment(row: &QueryResult) -> Result<comments::Model, sea_orm::DbErr> {
    Ok(comments::Model {
        id: row.try_get("", "id")?,
        ap_id: row.try_get("", "ap_id")?,
        post_id: row.try_get("", "post_id")?,
        author_id: row.try_get("", "author_id")?,
        parent_id: row.try_get("", "parent_id")?,
        path: row.try_get("", "path")?,
        body: row.try_get("", "body")?,
        score: row.try_get("", "score")?,
        is_deleted: row.try_get("", "is_deleted")?,
        is_removed: row.try_get("", "is_removed")?,
        published_at: row
            .try_get::<chrono::DateTime<chrono::Utc>>("", "published_at")?
            .into(),
        created_at: row
            .try_get::<chrono::DateTime<chrono::Utc>>("", "created_at")?
            .into(),
        updated_at: row
            .try_get::<chrono::DateTime<chrono::Utc>>("", "updated_at")?
            .into(),
    })
}

fn rows_to_comments(rows: Vec<QueryResult>) -> Result<Vec<comments::Model>, sea_orm::DbErr> {
    rows.iter().map(row_to_comment).collect()
}

fn opt_row_to_comment(rows: Vec<QueryResult>) -> Result<Option<comments::Model>, sea_orm::DbErr> {
    if let Some(row) = rows.into_iter().next() {
        row_to_comment(&row).map(Some)
    } else {
        Ok(None)
    }
}

/// Errors that can occur during comment operations.
#[derive(Debug, thiserror::Error)]
pub enum CommentError {
    #[error("Validation error: {0}")]
    Validation(String),
    #[error("Not found")]
    NotFound,
    #[error("Post is locked")]
    PostLocked,
    #[error(transparent)]
    Db(#[from] sea_orm::DbErr),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// A comment paired with its parent post title and community slug for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommentWithPost {
    pub comment: comments::Model,
    pub post_title: String,
    pub post_community_slug: Option<String>,
}

pub struct CommentModel;

impl CommentModel {
    /// Creates a new comment on a post, optionally as a reply to another comment.
    /// Uses ltree paths for threaded ordering.
    #[instrument(skip(db))]
    pub async fn create(
        db: &DatabaseConnection,
        post_id: Uuid,
        author_id: Uuid,
        parent_id: Option<Uuid>,
        body: &str,
        base_url: &str,
    ) -> Result<comments::Model, CommentError> {
        let body = body.trim();
        if body.is_empty() {
            return Err(CommentError::Validation(
                "Comment body cannot be empty".to_string(),
            ));
        }

        let post = posts::Entity::find_by_id(post_id)
            .one(db)
            .await?
            .ok_or(CommentError::NotFound)?;
        if post.is_locked {
            return Err(CommentError::PostLocked);
        }

        let id = Uuid::new_v4();
        let base = base_url.trim_end_matches('/');
        let ap_id = format!("{}/comments/{}", base, id);

        let post_seg = format!("c{}", &post_id.to_string().replace('-', "")[..8]);
        let comment_seg = format!("c{}", &id.to_string().replace('-', "")[..8]);

        let path = if let Some(pid) = parent_id {
            let parent = Self::raw_find_by_id(db, pid)
                .await?
                .ok_or(CommentError::NotFound)?;
            if parent.post_id != post_id {
                return Err(CommentError::NotFound);
            }
            format!("{}.{}", parent.path, comment_seg)
        } else {
            format!("{}.{}", post_seg, comment_seg)
        };

        let now = chrono::Utc::now();
        let txn = db.begin().await?;

        let stmt = Statement::from_sql_and_values(
            DbBackend::Postgres,
            r#"INSERT INTO comments (id, ap_id, post_id, author_id, parent_id, path, body, score, is_deleted, is_removed, published_at, created_at, updated_at)
               VALUES ($1, $2, $3, $4, $5, CAST($6 AS ltree), $7, $8, $9, $10, $11, $12, $13)"#,
            vec![
                Value::from(id),
                Value::from(ap_id),
                Value::from(post_id),
                Value::from(author_id),
                Value::from(parent_id),
                Value::from(path),
                Value::from(body.to_string()),
                Value::Int(Some(0)),
                Value::Bool(Some(false)),
                Value::Bool(Some(false)),
                Value::from(now),
                Value::from(now),
                Value::from(now),
            ],
        );

        txn.execute_raw(stmt).await?;

        // Increment in SQL so concurrent comments cannot lose an update.
        posts::Entity::update_many()
            .col_expr(
                posts::Column::CommentCount,
                Expr::col(posts::Column::CommentCount).add(1),
            )
            .filter(posts::Column::Id.eq(post_id))
            .exec(&txn)
            .await?;

        txn.commit().await?;

        let result = Self::raw_find_by_id(db, id)
            .await?
            .ok_or(CommentError::NotFound)?;

        Ok(result)
    }

    /// Lists non-deleted comments for a post in thread order.
    #[instrument(skip(db))]
    pub async fn list_for_post(
        db: &DatabaseConnection,
        post_id: Uuid,
    ) -> Result<Vec<comments::Model>, sea_orm::DbErr> {
        let stmt = Statement::from_sql_and_values(
            DbBackend::Postgres,
            format!(
                "{} WHERE post_id = $1 AND is_deleted = false ORDER BY path ASC",
                COMMENT_SELECT
            )
            .as_str(),
            vec![Value::from(post_id)],
        );

        let rows = db.query_all_raw(stmt).await?;
        rows_to_comments(rows)
    }

    /// Finds a comment by its UUID primary key.
    #[instrument(skip(db))]
    pub async fn find_by_id(
        db: &DatabaseConnection,
        id: Uuid,
    ) -> Result<Option<comments::Model>, sea_orm::DbErr> {
        Self::raw_find_by_id(db, id).await
    }

    async fn raw_find_by_id(
        db: &DatabaseConnection,
        id: Uuid,
    ) -> Result<Option<comments::Model>, sea_orm::DbErr> {
        let stmt = Statement::from_sql_and_values(
            DbBackend::Postgres,
            format!("{} WHERE id = $1", COMMENT_SELECT).as_str(),
            vec![Value::from(id)],
        );

        let rows = db.query_all_raw(stmt).await?;
        opt_row_to_comment(rows)
    }

    /// Recalculates the score of a comment from its votes in one statement.
    #[instrument(skip(db))]
    pub async fn recalculate_score(
        db: &DatabaseConnection,
        comment_id: Uuid,
    ) -> Result<(), sea_orm::DbErr> {
        let result = comments::Entity::update_many()
            .col_expr(
                comments::Column::Score,
                Expr::cust_with_values(
                    "(SELECT COALESCE(SUM(value), 0) FROM votes WHERE comment_id = $1)",
                    [comment_id],
                ),
            )
            .filter(comments::Column::Id.eq(comment_id))
            .exec(db)
            .await?;
        if result.rows_affected == 0 {
            return Err(sea_orm::DbErr::RecordNotFound(
                "Comment not found".to_string(),
            ));
        }
        Ok(())
    }

    /// Lists non-deleted comments by a specific author, newest first,
    /// keyset-paginated, enriched with post titles and community slugs.
    #[instrument(skip(db))]
    pub async fn list_for_author(
        db: &DatabaseConnection,
        author_id: Uuid,
        cursor: Option<&Cursor>,
        limit: u64,
    ) -> Result<Page<CommentWithPost>, sea_orm::DbErr> {
        let mut query = comments::Entity::find()
            .filter(comments::Column::AuthorId.eq(author_id))
            .filter(comments::Column::IsDeleted.eq(false));
        if let Some(c) = cursor
            && let (Some(published_at), Ok(id)) = (c.published_at, Uuid::parse_str(&c.key))
        {
            query = query.filter(Expr::cust_with_values(
                "(published_at, id) < ($1, $2)",
                [Value::from(published_at), Value::from(id)],
            ));
        }
        let mut models = query
            .order_by_desc(comments::Column::PublishedAt)
            .order_by_desc(comments::Column::Id)
            .limit(limit + 1)
            .all(db)
            .await?;
        let next_cursor = if models.len() as u64 > limit {
            models.truncate(limit as usize);
            models.last().map(|c| {
                Cursor {
                    score: None,
                    published_at: Some(c.published_at),
                    key: c.id.to_string(),
                }
                .encode()
            })
        } else {
            None
        };

        let post_ids: Vec<Uuid> = models.iter().map(|c| c.post_id).collect();
        let posts_map: HashMap<Uuid, posts::Model> = posts::Entity::find()
            .filter(posts::Column::Id.is_in(post_ids.clone()))
            .all(db)
            .await?
            .into_iter()
            .map(|p| (p.id, p))
            .collect();

        let community_ids: Vec<Uuid> = posts_map.values().filter_map(|p| p.community_id).collect();
        let communities_map: HashMap<Uuid, communities::Model> = if community_ids.is_empty() {
            HashMap::new()
        } else {
            communities::Entity::find()
                .filter(communities::Column::Id.is_in(community_ids))
                .all(db)
                .await?
                .into_iter()
                .map(|c| (c.id, c))
                .collect()
        };

        let results: Vec<CommentWithPost> = models
            .into_iter()
            .map(|c| {
                let post = posts_map.get(&c.post_id);
                let slug = post
                    .and_then(|p| p.community_id)
                    .and_then(|cid| communities_map.get(&cid))
                    .map(|comm| comm.slug.clone());
                CommentWithPost {
                    comment: c,
                    post_title: post.map(|p| p.title.clone()).unwrap_or_default(),
                    post_community_slug: slug,
                }
            })
            .collect();

        Ok(Page {
            items: results,
            next_cursor,
        })
    }

    /// Updates the body of a comment (author edit).
    #[instrument(skip(db))]
    pub async fn update(
        db: &DatabaseConnection,
        comment_id: Uuid,
        body: &str,
    ) -> Result<comments::Model, CommentError> {
        let body = body.trim();
        if body.is_empty() {
            return Err(CommentError::Validation(
                "Comment body cannot be empty".to_string(),
            ));
        }

        let mut comment: comments::ActiveModel = comments::Entity::find_by_id(comment_id)
            .one(db)
            .await?
            .ok_or(CommentError::NotFound)?
            .into();
        comment.body = Set(body.to_string());
        comment.updated_at = Set(chrono::Utc::now().into());
        Ok(comment.update(db).await?)
    }

    /// Marks a comment as removed by a moderator (soft removal).
    #[instrument(skip(db))]
    pub async fn soft_remove(
        db: &DatabaseConnection,
        comment_id: Uuid,
    ) -> Result<(), sea_orm::DbErr> {
        let found = comments::Entity::find_by_id(comment_id)
            .one(db)
            .await?
            .ok_or(sea_orm::DbErr::RecordNotFound(
                "Comment not found".to_string(),
            ))?;
        let author_id = found.author_id;
        let mut comment: comments::ActiveModel = found.into();
        comment.is_removed = Set(true);
        comment.updated_at = Set(chrono::Utc::now().into());
        comment.update(db).await?;

        crate::models::users::UserModel::update_standing(
            db,
            author_id,
            crate::models::posts::REMOVAL_STANDING_PENALTY,
        )
        .await?;

        Ok(())
    }

    /// Marks a comment as deleted (soft delete — retains the row).
    #[instrument(skip(db))]
    pub async fn soft_delete(
        db: &DatabaseConnection,
        comment_id: Uuid,
    ) -> Result<(), sea_orm::DbErr> {
        let mut comment: comments::ActiveModel = comments::Entity::find_by_id(comment_id)
            .one(db)
            .await?
            .ok_or(sea_orm::DbErr::RecordNotFound(
                "Comment not found".to_string(),
            ))?
            .into();
        comment.is_deleted = Set(true);
        comment.updated_at = Set(chrono::Utc::now().into());
        comment.update(db).await?;
        Ok(())
    }
}
