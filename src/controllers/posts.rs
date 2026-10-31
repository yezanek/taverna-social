use axum::extract::Path;
use axum::http::HeaderMap;
use axum::{Extension, Form};
use axum_extra::extract::cookie::CookieJar;
use loco_rs::controller::ErrorDetail;
use loco_rs::prelude::*;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use tracing::instrument;
use uuid::Uuid;

use crate::common::config::TavernaConfig;
use crate::common::markdown::render_markdown;
use crate::controllers::csrf::{self, CsrfForm};
use crate::federation::outbound;
use crate::initializers::auth_middleware::{AuthUser, OptionalAuthUser};
use crate::initializers::theme_middleware::CurrentTheme;
use crate::models::_entities::{comments, communities, posts, users};
use crate::models::comments::{CommentError, CommentModel};
use crate::models::communities::CommunityModel;
use crate::models::community_bans::Entity as CommunityBansEntity;
use crate::models::posts::{PostError, PostModel};
use crate::models::remote::RemoteCommentModel;
use crate::models::users::UserModel;
use crate::models::votes::{VoteError, VoteModel};
use crate::views::helpers;

/// Form data for editing a post.
#[derive(Debug, Deserialize)]
pub struct EditPostForm {
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub csrf_token: Option<String>,
}

/// Form data for casting a vote (upvote/downvote).
#[derive(Debug, Deserialize)]
pub struct VoteForm {
    pub vote: i16,
    #[serde(default)]
    pub csrf_token: Option<String>,
}

/// Form data for creating a comment.
#[derive(Debug, Deserialize)]
pub struct CommentForm {
    pub body: String,
    #[serde(default)]
    pub parent_id: Option<Uuid>,
    #[serde(default)]
    pub csrf_token: Option<String>,
}

/// Form data for editing a comment.
#[derive(Debug, Deserialize)]
pub struct EditCommentForm {
    pub body: String,
    #[serde(default)]
    pub csrf_token: Option<String>,
}

// ── Shared lookups ──────────────────────────────────────────

fn db_error(e: impl std::fmt::Display) -> Error {
    tracing::error!(error = %e, "database error");
    Error::InternalServerError
}

fn forbidden(reason: &str) -> Error {
    Error::CustomError(
        axum::http::StatusCode::FORBIDDEN,
        ErrorDetail::with_reason(reason),
    )
}

fn invalid_csrf() -> Error {
    Error::Unauthorized("Invalid CSRF token".to_string())
}

/// Loads the community and a post that belongs to it, or 404.
async fn load_post(
    db: &sea_orm::DatabaseConnection,
    slug: &str,
    post_id: Uuid,
) -> Result<(communities::Model, posts::Model)> {
    let community = CommunityModel::find_by_slug(db, slug)
        .await
        .map_err(db_error)?
        .ok_or(Error::NotFound)?;
    let post = PostModel::find_by_id(db, post_id)
        .await
        .map_err(db_error)?
        .filter(|p| p.community_id == Some(community.id))
        .ok_or(Error::NotFound)?;
    Ok((community, post))
}

/// Loads the community, post and a comment on that post, or 404.
async fn load_comment(
    db: &sea_orm::DatabaseConnection,
    slug: &str,
    post_id: Uuid,
    comment_id: Uuid,
) -> Result<(communities::Model, posts::Model, comments::Model)> {
    let (community, post) = load_post(db, slug, post_id).await?;
    let comment = CommentModel::find_by_id(db, comment_id)
        .await
        .map_err(db_error)?
        .filter(|c| c.post_id == post_id)
        .ok_or(Error::NotFound)?;
    Ok((community, post, comment))
}

/// Fails unless `user` moderates `community` (or is an admin).
async fn require_moderator(
    db: &sea_orm::DatabaseConnection,
    community: &communities::Model,
    user: &users::Model,
) -> Result<()> {
    if CommunityModel::can_moderate(db, community.id, user)
        .await
        .map_err(db_error)?
    {
        Ok(())
    } else {
        Err(Error::Unauthorized("Moderator access required".to_string()))
    }
}

fn score_class(score: i32) -> &'static str {
    match score.signum() {
        1 => "positive",
        -1 => "negative",
        _ => "",
    }
}

fn post_url(slug: &str, post_id: Uuid) -> String {
    format!("/c/{slug}/posts/{post_id}")
}

/// Maps a vote failure to a response: self-votes are refused with 403.
fn vote_error(e: VoteError) -> Error {
    match e {
        VoteError::OwnContent => forbidden("You cannot vote on your own content"),
        VoteError::InvalidValue => Error::BadRequest("Vote must be 1 or -1".to_string()),
        VoteError::NotFound => Error::NotFound,
        VoteError::Db(e) => db_error(e),
    }
}

/// Sends a Like/Dislike to the author of remote-authored local content.
/// Votes on content authored here need no delivery.
async fn federate_vote(
    db: &sea_orm::DatabaseConnection,
    voter: &users::Model,
    author_id: Uuid,
    object_ap_id: &str,
    value: i16,
) -> Result<()> {
    let author = UserModel::find_by_id(db, author_id)
        .await
        .map_err(db_error)?
        .ok_or(Error::InternalServerError)?;
    if !author.is_local {
        let activity = outbound::vote(&voter.ap_id, object_ap_id, value);
        outbound::send(db, &activity, &voter.ap_id, vec![author.inbox_url]).await;
    }
    Ok(())
}

// ── Handlers ────────────────────────────────────────────────

/// Shows a single post with its comments, author info, and voting UI.
#[instrument(skip(v, ctx, theme, current_user, jar))]
pub async fn detail(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    OptionalAuthUser(current_user): OptionalAuthUser,
    jar: CookieJar,
    Extension(theme): Extension<CurrentTheme>,
    Path((slug, post_id)): Path<(String, Uuid)>,
) -> Result<Response> {
    let (community, post) = load_post(&ctx.db, &slug, post_id).await?;

    let can_moderate = match &current_user {
        Some(user) => CommunityModel::can_moderate(&ctx.db, community.id, user)
            .await
            .map_err(db_error)?,
        None => false,
    };
    if post.is_deleted || (post.is_removed && !can_moderate) {
        return Err(Error::NotFound);
    }

    let author = UserModel::find_by_id(&ctx.db, post.author_id)
        .await
        .map_err(db_error)?
        .ok_or(Error::NotFound)?;

    let body_rendered = post
        .body
        .as_deref()
        .map(render_markdown)
        .unwrap_or_default();

    let comments = CommentModel::list_for_post(&ctx.db, post_id)
        .await
        .map_err(db_error)?;

    // One query for every comment author instead of one per comment.
    let author_ids: Vec<Uuid> = comments.iter().map(|c| c.author_id).collect();
    let usernames: HashMap<Uuid, String> = UserModel::find_by_ids(&ctx.db, &author_ids)
        .await
        .map_err(db_error)?
        .into_iter()
        .map(|u| (u.id, u.username))
        .collect();

    let comments_with_author: Vec<serde_json::Value> = comments
        .iter()
        .map(|comment| {
            json!({
                "id": comment.id,
                "ap_id": comment.ap_id,
                "post_id": comment.post_id,
                "author_id": comment.author_id,
                "parent_id": comment.parent_id,
                "path": comment.path,
                // Removed comments keep their place in the thread, but only
                // moderators still see what was said.
                "body": if comment.is_removed && !can_moderate {
                    "[removed by a moderator]"
                } else {
                    comment.body.as_str()
                },
                "score": comment.score,
                "is_deleted": comment.is_deleted,
                "is_removed": comment.is_removed,
                "published_at": comment.published_at,
                "created_at": comment.created_at,
                "updated_at": comment.updated_at,
                "author_username": usernames
                    .get(&comment.author_id)
                    .map_or("unknown", String::as_str),
                "depth": comment.path.matches('.').count() + 1,
            })
        })
        .collect();

    let remote_comments: Vec<serde_json::Value> =
        RemoteCommentModel::list_for_local_post(&ctx.db, post_id)
            .await
            .map_err(db_error)?
            .into_iter()
            .map(|(c, author)| {
                let author_name = author
                    .map(|a| format!("{}@{}", a.username, a.instance_domain))
                    .unwrap_or_else(|| c.author_ap_id.clone());
                json!({
                    "ap_id": c.ap_id,
                    "author_ap_id": c.author_ap_id,
                    "author_name": author_name,
                    "body_rendered": render_markdown(&c.body),
                    "score": c.score,
                    "published_at": c.published_at,
                })
            })
            .collect();

    let user_vote = match &current_user {
        Some(user) => VoteModel::get_user_vote_on_post(&ctx.db, user.id, post_id)
            .await
            .map_err(db_error)?,
        None => None,
    };

    let (csrf_token, csrf_cookie) = csrf::issue();
    let response = helpers::render_with_theme(
        &v,
        "communities/post.html",
        json!({
            "current_user": current_user,
            "community": community,
            "post": post,
            "author": author,
            "body_rendered": body_rendered,
            "comments": comments_with_author,
            "remote_comments": remote_comments,
            "user_vote": user_vote,
            "score_class": score_class(post.score),
            "target_id": post.id,
            "vote_url": format!("{}/vote", post_url(&slug, post.id)),
            "score": post.score,
            "slug": slug,
            "can_moderate": can_moderate,
            "csrf_token": csrf_token,
        }),
        &theme,
    )?;
    Ok((jar.add(csrf_cookie), response).into_response())
}

/// Casts or toggles a vote on a post (upvote/downvote). Returns vote button partial.
#[allow(clippy::too_many_arguments)]
#[instrument(skip(v, ctx, jar, headers, taverna, form))]
pub async fn vote(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path((slug, post_id)): Path<(String, Uuid)>,
    Form(form): Form<VoteForm>,
) -> Result<Response> {
    load_post(&ctx.db, &slug, post_id).await?;
    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }

    VoteModel::cast_vote(&ctx.db, current_user.id, Some(post_id), None, form.vote)
        .await
        .map_err(vote_error)?;

    let post = PostModel::find_by_id(&ctx.db, post_id)
        .await
        .map_err(db_error)?
        .ok_or(Error::NotFound)?;

    if taverna.federation.enabled {
        federate_vote(
            &ctx.db,
            &current_user,
            post.author_id,
            &post.ap_id,
            form.vote,
        )
        .await?;
    }

    let user_vote = VoteModel::get_user_vote_on_post(&ctx.db, current_user.id, post_id)
        .await
        .map_err(db_error)?;

    helpers::render(
        &v,
        "partials/vote_buttons.html",
        json!({
            "target_type": "post",
            "target_id": post_id,
            "vote_url": format!("{}/vote", post_url(&slug, post_id)),
            "score": post.score,
            "user_vote": user_vote,
            "score_class": score_class(post.score),
            "slug": slug,
            "csrf_token": form.csrf_token,
        }),
    )
}

/// Renders the edit form for a post (author only).
#[instrument(skip(v, ctx, theme, current_user, jar))]
pub async fn edit_form(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(theme): Extension<CurrentTheme>,
    Path((slug, post_id)): Path<(String, Uuid)>,
) -> Result<Response> {
    let (community, post) = load_post(&ctx.db, &slug, post_id).await?;
    if post.author_id != current_user.id {
        return Err(Error::Unauthorized("Not authorized".to_string()));
    }

    let (csrf_token, csrf_cookie) = csrf::issue();
    let response = helpers::render_with_theme(
        &v,
        "communities/edit_post.html",
        json!({
            "current_user": current_user,
            "community": community,
            "post": post,
            "csrf_token": csrf_token,
        }),
        &theme,
    )?;
    Ok((jar.add(csrf_cookie), response).into_response())
}

/// Updates a post (author only). Enqueues AP Update activity.
#[instrument(skip(ctx, form, jar, headers, taverna))]
pub async fn update_post(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path((slug, post_id)): Path<(String, Uuid)>,
    Form(form): Form<EditPostForm>,
) -> Result<Response> {
    let (community, post) = load_post(&ctx.db, &slug, post_id).await?;
    if post.author_id != current_user.id {
        return Err(Error::Unauthorized("Not authorized".to_string()));
    }
    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }

    let updated = PostModel::update(
        &ctx.db,
        post_id,
        &form.title,
        form.body.as_deref(),
        form.url.as_deref(),
    )
    .await
    .map_err(|e| match e {
        PostError::Validation(msg) => Error::BadRequest(msg),
        PostError::NotFound => Error::NotFound,
        other => db_error(other),
    })?;

    if taverna.federation.enabled {
        let note = outbound::post_note(
            &updated,
            &current_user.ap_id,
            Some(&community),
            &current_user.followers_url,
        );
        let update = outbound::wrap("Update", &current_user.ap_id, note);
        outbound::send_to_community_followers(&ctx.db, &update, &current_user.ap_id, community.id)
            .await;
    }

    Ok(format::redirect(&post_url(&slug, post_id)).into_response())
}

/// Soft-deletes a post (author, moderator, or admin). Enqueues AP Delete activity.
#[instrument(skip(ctx, jar, headers, taverna, form))]
pub async fn delete(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path((slug, post_id)): Path<(String, Uuid)>,
    Form(form): Form<CsrfForm>,
) -> Result<Response> {
    let (community, post) = load_post(&ctx.db, &slug, post_id).await?;
    if post.author_id != current_user.id {
        require_moderator(&ctx.db, &community, &current_user).await?;
    }
    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }

    PostModel::soft_delete(&ctx.db, post_id)
        .await
        .map_err(db_error)?;

    if taverna.federation.enabled {
        let delete = outbound::delete(&current_user.ap_id, &post.ap_id);
        outbound::send_to_community_followers(&ctx.db, &delete, &current_user.ap_id, community.id)
            .await;
    }

    Ok(format::redirect(&format!("/c/{slug}")).into_response())
}

/// Creates a comment on a post with federation support.
#[instrument(skip(ctx, form, jar, headers, taverna))]
pub async fn create_comment(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path((slug, post_id)): Path<(String, Uuid)>,
    Form(form): Form<CommentForm>,
) -> Result<Response> {
    let (community, post) = load_post(&ctx.db, &slug, post_id).await?;
    if post.is_locked {
        return Err(forbidden("Post is locked"));
    }
    if CommunityBansEntity::is_banned(&ctx.db, community.id, current_user.id)
        .await
        .map_err(db_error)?
    {
        return Err(forbidden("You are banned from this community"));
    }
    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }

    // CommentModel::create also bumps the post's comment_count.
    let comment = CommentModel::create(
        &ctx.db,
        post_id,
        current_user.id,
        form.parent_id,
        &form.body,
        &taverna.base_url,
    )
    .await
    .map_err(|e| match e {
        CommentError::Validation(msg) => Error::BadRequest(msg),
        CommentError::PostLocked => forbidden("Post is locked"),
        CommentError::NotFound => Error::NotFound,
        other => db_error(other),
    })?;

    if taverna.federation.enabled {
        let in_reply_to = match form.parent_id {
            Some(parent_id) => CommentModel::find_by_id(&ctx.db, parent_id)
                .await
                .map_err(db_error)?
                .map_or_else(|| post.ap_id.clone(), |p| p.ap_id),
            None => post.ap_id.clone(),
        };
        let note = outbound::comment_note(&comment, &current_user.ap_id, &in_reply_to);
        let create = outbound::wrap("Create", &current_user.ap_id, note);
        outbound::send_to_community_followers(&ctx.db, &create, &current_user.ap_id, community.id)
            .await;
    }

    Ok(format::redirect(&post_url(&slug, post_id)).into_response())
}

/// Casts or toggles a vote on a comment. Returns vote button partial.
#[allow(clippy::too_many_arguments)]
#[instrument(skip(v, ctx, jar, headers, taverna, form))]
pub async fn vote_comment(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path((slug, post_id, comment_id)): Path<(String, Uuid, Uuid)>,
    Form(form): Form<VoteForm>,
) -> Result<Response> {
    load_comment(&ctx.db, &slug, post_id, comment_id).await?;
    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }

    VoteModel::cast_vote(&ctx.db, current_user.id, None, Some(comment_id), form.vote)
        .await
        .map_err(vote_error)?;

    let comment = CommentModel::find_by_id(&ctx.db, comment_id)
        .await
        .map_err(db_error)?
        .ok_or(Error::NotFound)?;

    if taverna.federation.enabled {
        federate_vote(
            &ctx.db,
            &current_user,
            comment.author_id,
            &comment.ap_id,
            form.vote,
        )
        .await?;
    }

    let user_vote = VoteModel::get_user_vote_on_comment(&ctx.db, current_user.id, comment_id)
        .await
        .map_err(db_error)?;

    helpers::render(
        &v,
        "partials/vote_buttons.html",
        json!({
            "target_type": "comment",
            "target_id": comment_id,
            "vote_url": format!("{}/comments/{comment_id}/vote", post_url(&slug, post_id)),
            "score": comment.score,
            "user_vote": user_vote,
            "score_class": score_class(comment.score),
            "slug": slug,
            "csrf_token": form.csrf_token,
        }),
    )
}

/// Removes a post (moderator or admin only).
#[instrument(skip(ctx, jar, headers, taverna, form))]
pub async fn remove(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path((slug, post_id)): Path<(String, Uuid)>,
    Form(form): Form<CsrfForm>,
) -> Result<Response> {
    let (community, _) = load_post(&ctx.db, &slug, post_id).await?;
    require_moderator(&ctx.db, &community, &current_user).await?;
    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }

    PostModel::soft_remove(&ctx.db, post_id)
        .await
        .map_err(db_error)?;
    Ok(format::redirect(&format!("/c/{slug}")).into_response())
}

/// Toggles a post's pinned status (moderator or admin only).
#[instrument(skip(ctx, jar, headers, taverna, form))]
pub async fn pin(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path((slug, post_id)): Path<(String, Uuid)>,
    Form(form): Form<CsrfForm>,
) -> Result<Response> {
    let (community, _) = load_post(&ctx.db, &slug, post_id).await?;
    require_moderator(&ctx.db, &community, &current_user).await?;
    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }

    PostModel::toggle_pin(&ctx.db, post_id)
        .await
        .map_err(db_error)?;
    Ok(format::redirect(&post_url(&slug, post_id)).into_response())
}

/// Toggles a post's locked status (moderator or admin only).
#[instrument(skip(ctx, jar, headers, taverna, form))]
pub async fn lock(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path((slug, post_id)): Path<(String, Uuid)>,
    Form(form): Form<CsrfForm>,
) -> Result<Response> {
    let (community, _) = load_post(&ctx.db, &slug, post_id).await?;
    require_moderator(&ctx.db, &community, &current_user).await?;
    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }

    PostModel::toggle_lock(&ctx.db, post_id)
        .await
        .map_err(db_error)?;
    Ok(format::redirect(&post_url(&slug, post_id)).into_response())
}

/// Renders the edit form for a comment (author only).
#[instrument(skip(v, ctx, theme, current_user, jar))]
pub async fn edit_comment_form(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(theme): Extension<CurrentTheme>,
    Path((slug, post_id, comment_id)): Path<(String, Uuid, Uuid)>,
) -> Result<Response> {
    let (community, post, comment) = load_comment(&ctx.db, &slug, post_id, comment_id).await?;
    if comment.author_id != current_user.id {
        return Err(Error::Unauthorized("Not authorized".to_string()));
    }

    let (csrf_token, csrf_cookie) = csrf::issue();
    let response = helpers::render_with_theme(
        &v,
        "communities/edit_comment.html",
        json!({
            "current_user": current_user,
            "community": community,
            "post": post,
            "comment": comment,
            "csrf_token": csrf_token,
        }),
        &theme,
    )?;
    Ok((jar.add(csrf_cookie), response).into_response())
}

/// Updates a comment body (author only). Enqueues AP Update activity.
#[instrument(skip(ctx, form, jar, headers, taverna))]
pub async fn update_comment(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path((slug, post_id, comment_id)): Path<(String, Uuid, Uuid)>,
    Form(form): Form<EditCommentForm>,
) -> Result<Response> {
    let (community, post, comment) = load_comment(&ctx.db, &slug, post_id, comment_id).await?;
    if comment.author_id != current_user.id {
        return Err(Error::Unauthorized("Not authorized".to_string()));
    }
    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }

    let updated = CommentModel::update(&ctx.db, comment_id, &form.body)
        .await
        .map_err(|e| match e {
            CommentError::Validation(msg) => Error::BadRequest(msg),
            CommentError::NotFound => Error::NotFound,
            other => db_error(other),
        })?;

    if taverna.federation.enabled {
        let note = outbound::comment_note(&updated, &current_user.ap_id, &post.ap_id);
        let update = outbound::wrap("Update", &current_user.ap_id, note);
        outbound::send_to_community_followers(&ctx.db, &update, &current_user.ap_id, community.id)
            .await;
    }

    Ok(format::redirect(&post_url(&slug, post_id)).into_response())
}

/// Deletes a comment (its author only). Enqueues AP Delete activity.
#[instrument(skip(ctx, jar, headers, taverna, form))]
pub async fn delete_comment(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path((slug, post_id, comment_id)): Path<(String, Uuid, Uuid)>,
    Form(form): Form<CsrfForm>,
) -> Result<Response> {
    let (community, _, comment) = load_comment(&ctx.db, &slug, post_id, comment_id).await?;
    if comment.author_id != current_user.id {
        return Err(Error::Unauthorized("Not authorized".to_string()));
    }
    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }

    CommentModel::soft_delete(&ctx.db, comment_id)
        .await
        .map_err(db_error)?;

    if taverna.federation.enabled {
        let delete = outbound::delete(&current_user.ap_id, &comment.ap_id);
        outbound::send_to_community_followers(&ctx.db, &delete, &current_user.ap_id, community.id)
            .await;
    }

    Ok(format::redirect(&post_url(&slug, post_id)).into_response())
}

/// Removes a comment (moderator or admin only). The author's standing drops
/// by the removal penalty (SPEC §3.7).
#[instrument(skip(ctx, jar, headers, taverna, form))]
pub async fn remove_comment(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path((slug, post_id, comment_id)): Path<(String, Uuid, Uuid)>,
    Form(form): Form<CsrfForm>,
) -> Result<Response> {
    let (community, _, _) = load_comment(&ctx.db, &slug, post_id, comment_id).await?;
    require_moderator(&ctx.db, &community, &current_user).await?;
    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }

    CommentModel::soft_remove(&ctx.db, comment_id)
        .await
        .map_err(db_error)?;

    Ok(format::redirect(&post_url(&slug, post_id)).into_response())
}
