use axum::Extension;
use loco_rs::prelude::*;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter};
use serde_json::json;
use std::time::Duration;
use tracing::instrument;

use crate::common::config::TavernaConfig;
use crate::models::_entities::{posts, users};
use crate::models::instance_settings;

/// How long the user and post counts are cached in `ctx.cache`.
const COUNTS_TTL: Duration = Duration::from_secs(300);
const COUNTS_CACHE_KEY: &str = "nodeinfo:counts";

/// Serves the NodeInfo well-known endpoint pointing to the schema URL.
#[instrument(skip(taverna))]
pub async fn well_known_nodeinfo(Extension(taverna): Extension<TavernaConfig>) -> Result<Response> {
    let href = format!("{}/nodeinfo/2.0", taverna.base_url.trim_end_matches('/'));
    let link = json!({
        "links": [{
            "rel": "http://nodeinfo.diaspora.software/ns/schema/2.0",
            "href": href
        }]
    });
    Ok(axum::Json(link).into_response())
}

/// Serves the NodeInfo 2.0 document with usage statistics.
#[instrument(skip(ctx))]
pub async fn nodeinfo_v2(State(ctx): State<AppContext>) -> Result<Response> {
    let (total_users, total_posts) = ctx
        .cache
        .get_or_insert_with_expiry(COUNTS_CACHE_KEY, COUNTS_TTL, async {
            Ok(count_users_and_posts(&ctx.db).await?)
        })
        .await?;
    let settings = instance_settings::load(&ctx.db).await?;

    let info = json!({
        "version": "2.0",
        "software": {
            "name": "taverna",
            "version": env!("CARGO_PKG_VERSION")
        },
        "protocols": ["activitypub"],
        "usage": {
            "users": {
                "total": total_users
            },
            "localPosts": total_posts
        },
        "openRegistrations": settings.open_registrations
    });
    Ok(axum::Json(info).into_response())
}

async fn count_users_and_posts(db: &DatabaseConnection) -> Result<(u64, u64), sea_orm::DbErr> {
    let total_users = users::Entity::find()
        .filter(users::Column::IsLocal.eq(true))
        .count(db)
        .await?;

    let total_posts = posts::Entity::find()
        .filter(posts::Column::IsDeleted.eq(false))
        .count(db)
        .await?;

    Ok((total_users, total_posts))
}
