use axum::{Extension, extract::Path, response::IntoResponse};
use futures::future::join_all;
use loco_rs::prelude::*;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use serde_json::json;
use tracing::instrument;

use crate::initializers::content_negotiation::RenderMode;
use crate::models::_entities::{communities, follows, posts, users};

#[instrument(skip(ctx))]
pub async fn user_actor(
    State(ctx): State<AppContext>,
    Extension(render_mode): Extension<RenderMode>,
    Path(username): Path<String>,
) -> Result<Response> {
    let user = users::Entity::find()
        .filter(users::Column::Username.eq(username.to_lowercase()))
        .one(&ctx.db)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    match render_mode {
        RenderMode::ActivityPub => {
            let actor = json!({
                "@context": [
                    "https://www.w3.org/ns/activitystreams",
                    "https://w3id.org/security/v1"
                ],
                "id": user.ap_id,
                "type": "Person",
                "preferredUsername": user.username,
                "name": user.display_name,
                "summary": user.bio,
                "icon": user.avatar_url,
                "inbox": format!("{}/inbox", user.ap_id),
                "outbox": format!("{}/outbox", user.ap_id),
                "followers": format!("{}/followers", user.ap_id),
                "following": format!("{}/following", user.ap_id),
                "publicKey": {
                    "id": format!("{}#main-key", user.ap_id),
                    "owner": user.ap_id,
                    "publicKeyPem": user.public_key,
                },
                "published": user.created_at.format("%+").to_string(),
            });
            Ok(axum::Json(actor).into_response())
        }
        RenderMode::Html => {
            Ok(axum::response::Redirect::to(&format!("/u/{}/posts", username)).into_response())
        }
    }
}

#[instrument(skip(ctx))]
pub async fn user_followers(
    State(ctx): State<AppContext>,
    Path(username): Path<String>,
) -> Result<Response> {
    let user = users::Entity::find()
        .filter(users::Column::Username.eq(username.to_lowercase()))
        .one(&ctx.db)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    let follows = follows::Entity::find()
        .filter(follows::Column::FollowingId.eq(user.id))
        .all(&ctx.db)
        .await
        .map_err(|_| Error::InternalServerError)?;

    let follower_futures: Vec<_> = follows
        .iter()
        .map(|f| users::Entity::find_by_id(f.follower_id).one(&ctx.db))
        .collect();
    let follower_results = join_all(follower_futures).await;
    let follower_ids: Vec<String> = follower_results
        .into_iter()
        .filter_map(|r| r.ok().flatten())
        .map(|u| u.ap_id)
        .collect();

    let collection = json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{}/followers", user.ap_id),
        "type": "OrderedCollection",
        "totalItems": follower_ids.len(),
        "orderedItems": follower_ids,
    });

    Ok(axum::Json(collection).into_response())
}

#[instrument(skip(ctx))]
pub async fn user_following(
    State(ctx): State<AppContext>,
    Path(username): Path<String>,
) -> Result<Response> {
    let user = users::Entity::find()
        .filter(users::Column::Username.eq(username.to_lowercase()))
        .one(&ctx.db)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    let follows = follows::Entity::find()
        .filter(follows::Column::FollowerId.eq(user.id))
        .filter(follows::Column::Accepted.eq(true))
        .all(&ctx.db)
        .await
        .map_err(|_| Error::InternalServerError)?;

    let following_futures: Vec<_> = follows
        .iter()
        .filter_map(|f| {
            f.following_id
                .map(|id| users::Entity::find_by_id(id).one(&ctx.db))
        })
        .collect();
    let following_results = join_all(following_futures).await;
    let following_ids: Vec<String> = following_results
        .into_iter()
        .filter_map(|r| r.ok().flatten())
        .map(|u| u.ap_id)
        .collect();

    let collection = json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{}/following", user.ap_id),
        "type": "OrderedCollection",
        "totalItems": following_ids.len(),
        "orderedItems": following_ids,
    });

    Ok(axum::Json(collection).into_response())
}

#[instrument(skip(ctx))]
pub async fn user_outbox(
    State(ctx): State<AppContext>,
    Path(username): Path<String>,
) -> Result<Response> {
    let user = users::Entity::find()
        .filter(users::Column::Username.eq(username.to_lowercase()))
        .one(&ctx.db)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    let recent_posts: Vec<posts::Model> = posts::Entity::find()
        .filter(posts::Column::AuthorId.eq(user.id))
        .filter(posts::Column::IsDeleted.eq(false))
        .order_by_desc(posts::Column::PublishedAt)
        .all(&ctx.db)
        .await
        .map_err(|_| Error::InternalServerError)?;

    let items: Vec<serde_json::Value> = recent_posts
        .into_iter()
        .take(20)
        .map(|p| {
            json!({
                "id": format!("{}/activity/create/{}", user.ap_id, p.id),
                "type": "Create",
                "actor": user.ap_id,
                "object": {
                    "id": p.ap_id,
                    "type": "Note",
                    "attributedTo": user.ap_id,
                    "content": p.body,
                    "published": p.published_at.format("%+").to_string(),
                },
                "published": p.published_at.format("%+").to_string(),
            })
        })
        .collect();

    let collection = json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{}/outbox", user.ap_id),
        "type": "OrderedCollection",
        "totalItems": items.len(),
        "orderedItems": items,
    });

    Ok(axum::Json(collection).into_response())
}

#[instrument(skip(ctx))]
pub async fn community_followers(
    State(ctx): State<AppContext>,
    Path(slug): Path<String>,
) -> Result<Response> {
    let community = communities::Entity::find()
        .filter(communities::Column::Slug.eq(slug.to_lowercase()))
        .one(&ctx.db)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    let follows = follows::Entity::find()
        .filter(follows::Column::CommunityId.eq(community.id))
        .filter(follows::Column::Accepted.eq(true))
        .all(&ctx.db)
        .await
        .map_err(|_| Error::InternalServerError)?;

    let follower_futures: Vec<_> = follows
        .iter()
        .map(|f| users::Entity::find_by_id(f.follower_id).one(&ctx.db))
        .collect();
    let follower_results = join_all(follower_futures).await;
    let follower_ids: Vec<String> = follower_results
        .into_iter()
        .filter_map(|r| r.ok().flatten())
        .map(|u| u.ap_id)
        .collect();

    let collection = json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{}/followers", community.ap_id),
        "type": "OrderedCollection",
        "totalItems": follower_ids.len(),
        "orderedItems": follower_ids,
    });

    Ok(axum::Json(collection).into_response())
}

#[instrument(skip(ctx))]
pub async fn community_outbox(
    State(ctx): State<AppContext>,
    Path(slug): Path<String>,
) -> Result<Response> {
    let community = communities::Entity::find()
        .filter(communities::Column::Slug.eq(slug.to_lowercase()))
        .one(&ctx.db)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    let recent_posts: Vec<posts::Model> = posts::Entity::find()
        .filter(posts::Column::CommunityId.eq(Some(community.id)))
        .filter(posts::Column::IsDeleted.eq(false))
        .order_by_desc(posts::Column::PublishedAt)
        .all(&ctx.db)
        .await
        .map_err(|_| Error::InternalServerError)?;

    let items: Vec<serde_json::Value> = recent_posts
        .into_iter()
        .take(20)
        .map(|p| {
            json!({
                "id": format!("{}/activity/announce/{}", community.ap_id, p.id),
                "type": "Announce",
                "actor": community.ap_id,
                "object": {
                    "id": p.ap_id,
                    "type": "Note",
                    "attributedTo": community.ap_id,
                    "content": p.body,
                    "published": p.published_at.format("%+").to_string(),
                },
                "published": p.published_at.format("%+").to_string(),
            })
        })
        .collect();

    let collection = json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{}/outbox", community.ap_id),
        "type": "OrderedCollection",
        "totalItems": items.len(),
        "orderedItems": items,
    });

    Ok(axum::Json(collection).into_response())
}
