//! Feed queries over the `community_feed` view and keyset pagination helpers
//! (PHASE_8.md Part A view, Part B4, ADR-018).
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sea_orm::{DatabaseConnection, DbErr, FromQueryResult, Statement, Value};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use uuid::Uuid;

/// Opaque keyset cursor, passed as `?cursor=...`.
///
/// Holds the sort key of the last row on the previous page. Which fields are
/// set depends on the listing: feeds use `(score, published_at, key)`,
/// history listings `(published_at, key)`, name listings just `key`. The
/// `key` (row id or unique name) breaks ties so no row is skipped or repeated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cursor {
    #[serde(rename = "s", skip_serializing_if = "Option::is_none", default)]
    pub score: Option<i32>,
    #[serde(rename = "t", skip_serializing_if = "Option::is_none", default)]
    pub published_at: Option<chrono::DateTime<chrono::FixedOffset>>,
    #[serde(rename = "k")]
    pub key: String,
}

impl Cursor {
    pub fn encode(&self) -> String {
        // Serialising this struct cannot fail; fall back to an empty cursor anyway.
        let json = serde_json::to_vec(self).unwrap_or_default();
        URL_SAFE_NO_PAD.encode(json)
    }

    /// Decodes a cursor; an invalid or tampered cursor yields `None` (first page).
    pub fn decode(raw: &str) -> Option<Self> {
        let bytes = URL_SAFE_NO_PAD.decode(raw.trim()).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Decodes an optional query parameter.
    pub fn from_param(raw: Option<&str>) -> Option<Self> {
        raw.filter(|s| !s.is_empty()).and_then(Self::decode)
    }
}

/// Query string for paginated pages: `?cursor=...`.
#[derive(Debug, Default, Deserialize)]
pub struct CursorQuery {
    pub cursor: Option<String>,
}

impl CursorQuery {
    pub fn cursor(&self) -> Option<Cursor> {
        Cursor::from_param(self.cursor.as_deref())
    }
}

/// One page of results plus the cursor for the next page, if there is one.
#[derive(Debug, Clone, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

impl<T> Page<T> {
    /// Builds a page from `limit + 1` fetched rows.
    pub fn from_rows(mut rows: Vec<T>, limit: u64, cursor_of: impl Fn(&T) -> Cursor) -> Self {
        let has_more = rows.len() as u64 > limit;
        rows.truncate(limit as usize);
        let next_cursor = if has_more {
            rows.last().map(|r| cursor_of(r).encode())
        } else {
            None
        };
        Page {
            items: rows,
            next_cursor,
        }
    }
}

/// A row of the `community_feed` view (local and remote posts).
#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct FeedItem {
    pub id: String,
    pub ap_id: String,
    pub community_id: Option<Uuid>,
    pub community_slug: Option<String>,
    pub author_ap_id: String,
    pub author_name: String,
    pub title: String,
    pub body: Option<String>,
    pub url: Option<String>,
    pub score: i32,
    pub comment_count: i32,
    pub published_at: chrono::DateTime<chrono::FixedOffset>,
    pub is_locked: bool,
    pub is_pinned: bool,
    pub origin: String,
}

impl FeedItem {
    pub fn is_remote(&self) -> bool {
        self.origin == "remote"
    }

    fn cursor(&self) -> Cursor {
        Cursor {
            score: Some(self.score),
            published_at: Some(self.published_at),
            key: self.id.clone(),
        }
    }
}

const FEED_SELECT: &str = r#"
    SELECT f.id, f.ap_id, f.community_id, c.slug AS community_slug,
           f.author_ap_id, f.author_name, f.title, f.body, f.url, f.score,
           f.comment_count, f.published_at, f.is_locked, f.is_pinned, f.origin
      FROM community_feed f
      LEFT JOIN communities c ON c.id = f.community_id
"#;

/// Appends the feed keyset condition and ordering, returning the SQL.
fn feed_sql(
    where_clauses: &mut Vec<String>,
    values: &mut Vec<Value>,
    cursor: Option<&Cursor>,
    limit: u64,
) -> String {
    if let Some(c) = cursor
        && let (Some(score), Some(published_at)) = (c.score, c.published_at)
    {
        let n = values.len();
        where_clauses.push(format!(
            "(f.score, f.published_at, f.id) < (${}, ${}, ${})",
            n + 1,
            n + 2,
            n + 3
        ));
        values.push(score.into());
        values.push(published_at.into());
        values.push(c.key.clone().into());
    }
    let where_sql = if where_clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", where_clauses.join(" AND "))
    };
    format!(
        "{FEED_SELECT}{where_sql} ORDER BY f.score DESC, f.published_at DESC, f.id DESC LIMIT {}",
        limit + 1
    )
}

async fn run_feed(
    db: &DatabaseConnection,
    sql: String,
    values: Vec<Value>,
    limit: u64,
) -> Result<Page<FeedItem>, DbErr> {
    let rows = FeedItem::find_by_statement(Statement::from_sql_and_values(
        db.get_database_backend(),
        sql,
        values,
    ))
    .all(db)
    .await?;
    Ok(Page::from_rows(rows, limit, FeedItem::cursor))
}

/// Local and remote posts in one community, best first.
#[instrument(skip(db))]
pub async fn community_feed(
    db: &DatabaseConnection,
    community_id: Uuid,
    cursor: Option<&Cursor>,
    limit: u64,
) -> Result<Page<FeedItem>, DbErr> {
    let mut where_clauses = vec!["f.community_id = $1".to_string()];
    let mut values: Vec<Value> = vec![community_id.into()];
    let sql = feed_sql(&mut where_clauses, &mut values, cursor, limit);
    run_feed(db, sql, values, limit).await
}

/// Local and remote posts across the instance, best first.
#[instrument(skip(db))]
pub async fn home_feed(
    db: &DatabaseConnection,
    cursor: Option<&Cursor>,
    limit: u64,
) -> Result<Page<FeedItem>, DbErr> {
    let mut where_clauses = Vec::new();
    let mut values: Vec<Value> = Vec::new();
    let sql = feed_sql(&mut where_clauses, &mut values, cursor, limit);
    run_feed(db, sql, values, limit).await
}
