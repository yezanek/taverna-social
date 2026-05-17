use activitypub_federation::FEDERATION_CONTENT_TYPE;
use axum::{extract::Path, http::header::CONTENT_TYPE, response::IntoResponse};
use loco_rs::prelude::*;
use sea_orm::EntityTrait;
use serde_json::json;
use tracing::instrument;
use uuid::Uuid;

use crate::models::_entities::{posts, users};
use crate::models::comments::CommentModel;

#[instrument(skip(ctx))]
pub async fn post_note(State(ctx): State<AppContext>, Path(id): Path<Uuid>) -> Result<Response> {
    let post = posts::Entity::find_by_id(id)
        .one(&ctx.db)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    let author = users::Entity::find_by_id(post.author_id)
        .one(&ctx.db)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    let note = json!({
        "@context": [
            "https://www.w3.org/ns/activitystreams",
            "https://w3id.org/security/v1"
        ],
        "id": post.ap_id,
        "type": "Note",
        "attributedTo": author.ap_id,
        "name": post.title,
        "content": post.body,
        "published": post.published_at.format("%+").to_string(),
        "url": post.ap_id,
        "to": ["https://www.w3.org/ns/activitystreams#Public"],
        "cc": [],
    });

    Ok(([(CONTENT_TYPE, FEDERATION_CONTENT_TYPE)], axum::Json(note)).into_response())
}

#[instrument(skip(ctx))]
pub async fn comment_note(State(ctx): State<AppContext>, Path(id): Path<Uuid>) -> Result<Response> {
    let comment = CommentModel::find_by_id(&ctx.db, id)
        .await
        .map_err(|e| {
            tracing::error!("Comment note: DB error finding comment {}: {}", id, e);
            Error::InternalServerError
        })?
        .ok_or_else(|| {
            tracing::error!("Comment note: comment not found: {}", id);
            Error::NotFound
        })?;

    let author = users::Entity::find_by_id(comment.author_id)
        .one(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!(
                "Comment note: DB error finding author {}: {}",
                comment.author_id,
                e
            );
            Error::InternalServerError
        })?
        .ok_or_else(|| {
            tracing::error!("Comment note: author not found: {}", comment.author_id);
            Error::NotFound
        })?;

    let post = posts::Entity::find_by_id(comment.post_id)
        .one(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!(
                "Comment note: DB error finding post {}: {}",
                comment.post_id,
                e
            );
            Error::InternalServerError
        })?
        .ok_or_else(|| {
            tracing::error!("Comment note: post not found: {}", comment.post_id);
            Error::NotFound
        })?;

    let note = json!({
        "@context": [
            "https://www.w3.org/ns/activitystreams",
            "https://w3id.org/security/v1"
        ],
        "id": comment.ap_id,
        "type": "Note",
        "attributedTo": author.ap_id,
        "content": comment.body,
        "inReplyTo": post.ap_id,
        "published": comment.published_at.format("%+").to_string(),
        "url": comment.ap_id,
        "to": ["https://www.w3.org/ns/activitystreams#Public"],
        "cc": [],
    });

    Ok(([(CONTENT_TYPE, FEDERATION_CONTENT_TYPE)], axum::Json(note)).into_response())
}
