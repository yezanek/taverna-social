//! ActivityPub collections of local actors: followers, following and outbox.
use std::collections::HashMap;

use axum::extract::Path;
use loco_rs::prelude::*;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde_json::{Value, json};
use tracing::instrument;
use uuid::Uuid;

use crate::federation::{ap_response, outbound};
use crate::models::_entities::{communities, follows, posts, users};
use crate::models::users::UserModel;

/// How many recent activities an outbox lists.
const OUTBOX_LIMIT: u64 = 20;

/// An `OrderedCollection` of `items`, served as `application/activity+json`.
fn collection(id: String, items: Vec<Value>) -> Response {
    ap_response(json!({
        "id": id,
        "type": "OrderedCollection",
        "totalItems": items.len(),
        "orderedItems": items,
    }))
}

async fn find_user(db: &DatabaseConnection, username: &str) -> Result<users::Model> {
    UserModel::find_by_username(db, username)
        .await?
        .ok_or(Error::NotFound)
}

async fn find_community(db: &DatabaseConnection, slug: &str) -> Result<communities::Model> {
    communities::Entity::find()
        .filter(communities::Column::Slug.eq(slug.to_lowercase()))
        .one(db)
        .await?
        .ok_or(Error::NotFound)
}

/// The most recent visible posts matching `filter`.
async fn recent_posts(
    db: &DatabaseConnection,
    filter: sea_orm::Condition,
) -> Result<Vec<posts::Model>> {
    Ok(posts::Entity::find()
        .filter(filter)
        .filter(posts::Column::IsDeleted.eq(false))
        .filter(posts::Column::IsRemoved.eq(false))
        .order_by_desc(posts::Column::PublishedAt)
        .limit(OUTBOX_LIMIT)
        .all(db)
        .await?)
}

/// Returns a user's followers collection (OrderedCollection).
#[instrument(skip(ctx))]
pub async fn user_followers(
    State(ctx): State<AppContext>,
    Path(username): Path<String>,
) -> Result<Response> {
    let user = find_user(&ctx.db, &username).await?;
    let follows = follows::Entity::find()
        .filter(follows::Column::FollowingId.eq(user.id))
        .filter(follows::Column::Accepted.eq(true))
        .all(&ctx.db)
        .await?;
    let ids = follower_ap_ids(&ctx.db, &follows).await?;
    Ok(collection(user.followers_url, ids))
}

/// Returns who a user is following (OrderedCollection).
#[instrument(skip(ctx))]
pub async fn user_following(
    State(ctx): State<AppContext>,
    Path(username): Path<String>,
) -> Result<Response> {
    let user = find_user(&ctx.db, &username).await?;
    let follows = follows::Entity::find()
        .filter(follows::Column::FollowerId.eq(user.id))
        .filter(follows::Column::Accepted.eq(true))
        .all(&ctx.db)
        .await?;

    let mut ids: Vec<Value> = follows
        .iter()
        .filter_map(|f| f.remote_following_ap_id.clone().map(Value::from))
        .collect();
    let local: Vec<Uuid> = follows.iter().filter_map(|f| f.following_id).collect();
    ids.extend(local_ap_ids(&ctx.db, &local).await?);
    Ok(collection(
        format!("{}/following", user.ap_id.trim_end_matches('/')),
        ids,
    ))
}

/// Returns a user's outbox: a `Create` for each recent post.
#[instrument(skip(ctx))]
pub async fn user_outbox(
    State(ctx): State<AppContext>,
    Path(username): Path<String>,
) -> Result<Response> {
    let user = find_user(&ctx.db, &username).await?;
    let posts = recent_posts(
        &ctx.db,
        sea_orm::Condition::all().add(posts::Column::AuthorId.eq(user.id)),
    )
    .await?;

    let community_ids: Vec<Uuid> = posts.iter().filter_map(|p| p.community_id).collect();
    let communities: HashMap<Uuid, communities::Model> = if community_ids.is_empty() {
        HashMap::new()
    } else {
        communities::Entity::find()
            .filter(communities::Column::Id.is_in(community_ids))
            .all(&ctx.db)
            .await?
            .into_iter()
            .map(|c| (c.id, c))
            .collect()
    };

    let items = posts
        .iter()
        .map(|p| {
            let community = p.community_id.and_then(|id| communities.get(&id));
            let note = outbound::post_note(p, &user.ap_id, community, &user.followers_url);
            with_id(
                outbound::wrap("Create", &user.ap_id, note),
                format!("{}/activity/create/{}", user.ap_id, p.id),
            )
        })
        .collect();
    Ok(collection(format!("{}/outbox", user.ap_id), items))
}

/// Returns a community's followers collection (OrderedCollection).
#[instrument(skip(ctx))]
pub async fn community_followers(
    State(ctx): State<AppContext>,
    Path(slug): Path<String>,
) -> Result<Response> {
    let community = find_community(&ctx.db, &slug).await?;
    let follows = follows::Entity::find()
        .filter(follows::Column::CommunityId.eq(community.id))
        .filter(follows::Column::Accepted.eq(true))
        .all(&ctx.db)
        .await?;
    let ids = follower_ap_ids(&ctx.db, &follows).await?;
    Ok(collection(community.followers_url, ids))
}

/// Returns a community's outbox: an `Announce` of each recent post, which
/// stays attributed to its author.
#[instrument(skip(ctx))]
pub async fn community_outbox(
    State(ctx): State<AppContext>,
    Path(slug): Path<String>,
) -> Result<Response> {
    let community = find_community(&ctx.db, &slug).await?;
    let posts = recent_posts(
        &ctx.db,
        sea_orm::Condition::all().add(posts::Column::CommunityId.eq(community.id)),
    )
    .await?;

    let author_ids: Vec<Uuid> = posts.iter().map(|p| p.author_id).collect();
    let authors: HashMap<Uuid, users::Model> = UserModel::find_by_ids(&ctx.db, &author_ids)
        .await?
        .into_iter()
        .map(|u| (u.id, u))
        .collect();

    let items = posts
        .iter()
        .filter_map(|p| {
            let author = authors.get(&p.author_id)?;
            let note =
                outbound::post_note(p, &author.ap_id, Some(&community), &author.followers_url);
            Some(with_id(
                outbound::wrap("Announce", &community.ap_id, note),
                format!("{}/activity/announce/{}", community.ap_id, p.id),
            ))
        })
        .collect();
    Ok(collection(format!("{}/outbox", community.ap_id), items))
}

/// Gives an outbox activity a stable id (the `Note` already carries the
/// `@context`, so the embedded copy is dropped).
fn with_id(mut activity: Value, id: String) -> Value {
    activity["id"] = Value::from(id);
    if let Some(obj) = activity.as_object_mut() {
        obj.remove("@context");
    }
    if let Some(note) = activity.get_mut("object").and_then(Value::as_object_mut) {
        note.remove("@context");
    }
    activity
}

/// AP ids of local users, fetched in one query.
async fn local_ap_ids(db: &DatabaseConnection, ids: &[Uuid]) -> Result<Vec<Value>> {
    Ok(UserModel::find_by_ids(db, ids)
        .await?
        .into_iter()
        .map(|u| Value::from(u.ap_id))
        .collect())
}

/// AP ids of the followers in `follows`: remote followers directly, local
/// followers looked up in one query.
async fn follower_ap_ids(
    db: &DatabaseConnection,
    follows: &[follows::Model],
) -> Result<Vec<Value>> {
    let mut ids: Vec<Value> = follows
        .iter()
        .filter_map(|f| f.remote_follower_ap_id.clone().map(Value::from))
        .collect();
    let local: Vec<Uuid> = follows.iter().filter_map(|f| f.follower_id).collect();
    ids.extend(local_ap_ids(db, &local).await?);
    Ok(ids)
}
