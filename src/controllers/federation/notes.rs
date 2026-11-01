use activitypub_federation::axum::json::FederationJson;
use axum::{extract::Path, response::IntoResponse};
use loco_rs::prelude::*;
use sea_orm::EntityTrait;
use tracing::instrument;
use uuid::Uuid;

use crate::federation::outbound;
use crate::models::_entities::{communities, posts, users};
use crate::models::comments::CommentModel;

/// A Note that already carries its `@context`, as `application/activity+json`.
fn federation_json(object: serde_json::Value) -> Response {
    FederationJson(object).into_response()
}

/// `GET /posts/:id`: a local post as an ActivityPub Note.
#[instrument(skip(ctx))]
pub async fn post_note(State(ctx): State<AppContext>, Path(id): Path<Uuid>) -> Result<Response> {
    let post = posts::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .filter(|p| !p.is_deleted && !p.is_removed)
        .ok_or(Error::NotFound)?;
    let author = users::Entity::find_by_id(post.author_id)
        .one(&ctx.db)
        .await?
        .ok_or(Error::NotFound)?;
    let community = match post.community_id {
        Some(cid) => communities::Entity::find_by_id(cid).one(&ctx.db).await?,
        None => None,
    };

    Ok(federation_json(outbound::post_note(
        &post,
        &author.ap_id,
        community.as_ref(),
        &author.followers_url,
    )))
}

/// `GET /comments/:id`: a local comment as an ActivityPub Note.
#[instrument(skip(ctx))]
pub async fn comment_note(State(ctx): State<AppContext>, Path(id): Path<Uuid>) -> Result<Response> {
    let comment = CommentModel::find_by_id(&ctx.db, id)
        .await?
        .filter(|c| !c.is_deleted && !c.is_removed)
        .ok_or(Error::NotFound)?;
    let author = users::Entity::find_by_id(comment.author_id)
        .one(&ctx.db)
        .await?
        .ok_or(Error::NotFound)?;
    let in_reply_to = match comment.parent_id {
        Some(parent_id) => CommentModel::find_by_id(&ctx.db, parent_id)
            .await?
            .map(|p| p.ap_id),
        None => None,
    };
    let in_reply_to = match in_reply_to {
        Some(ap_id) => ap_id,
        None => {
            posts::Entity::find_by_id(comment.post_id)
                .one(&ctx.db)
                .await?
                .ok_or(Error::NotFound)?
                .ap_id
        }
    };

    Ok(federation_json(outbound::comment_note(
        &comment,
        &author.ap_id,
        &in_reply_to,
    )))
}
