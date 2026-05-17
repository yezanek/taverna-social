use loco_rs::prelude::*;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter};
use serde_json::json;
use std::sync::OnceLock;
use std::time::Instant;
use tokio::sync::Mutex;
use tracing::instrument;

use crate::models::_entities::{posts, users};

struct NodeInfoCache {
    counts: (usize, usize),
    cached_at: Instant,
}

const CACHE_DURATION_SECS: u64 = 300;

fn cache() -> &'static Mutex<Option<NodeInfoCache>> {
    static CACHE: OnceLock<Mutex<Option<NodeInfoCache>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

#[instrument(skip(_ctx))]
pub async fn well_known_nodeinfo(State(_ctx): State<AppContext>) -> Result<Response> {
    let link = json!({
        "links": [{
            "rel": "http://nodeinfo.diaspora.software/ns/schema/2.0",
            "href": "/nodeinfo/2.0"
        }]
    });
    Ok(axum::Json(link).into_response())
}

#[instrument(skip(ctx))]
pub async fn nodeinfo_v2(State(ctx): State<AppContext>) -> Result<Response> {
    let (total_users, total_posts) = get_cached_counts(&ctx.db).await;

    let info = json!({
        "version": "2.0",
        "software": {
            "name": "taverna",
            "version": "0.1.0"
        },
        "protocols": ["activitypub"],
        "usage": {
            "users": {
                "total": total_users
            },
            "localPosts": total_posts
        },
        "openRegistrations": true
    });
    Ok(axum::Json(info).into_response())
}

async fn get_cached_counts(db: &DatabaseConnection) -> (usize, usize) {
    let mut guard = cache().lock().await;
    if let Some(ref cache_data) = *guard
        && cache_data.cached_at.elapsed().as_secs() < CACHE_DURATION_SECS
    {
        return cache_data.counts;
    }

    let total_users = users::Entity::find()
        .filter(users::Column::IsLocal.eq(true))
        .count(db)
        .await
        .unwrap_or(0) as usize;

    let total_posts = posts::Entity::find()
        .filter(posts::Column::IsDeleted.eq(false))
        .count(db)
        .await
        .unwrap_or(0) as usize;

    let counts = (total_users, total_posts);

    *guard = Some(NodeInfoCache {
        counts,
        cached_at: Instant::now(),
    });

    counts
}
