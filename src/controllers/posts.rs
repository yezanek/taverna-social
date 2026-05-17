use axum::extract::Path;
use axum::http::HeaderMap;
use axum::{Extension, Form};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use loco_rs::prelude::*;
use serde::Deserialize;
use serde_json::json;
use tracing::instrument;
use uuid::Uuid;

use crate::common::config::TavernaConfig;
use crate::common::markdown::render_markdown;
use crate::controllers::auth::validate_csrf;
use crate::initializers::auth_middleware::{AuthUser, OptionalAuthUser};
use crate::initializers::theme_middleware::CurrentTheme;
use crate::models::comments::CommentModel;
use crate::models::communities::CommunityModel;
use crate::models::posts::PostModel;
use crate::models::users::UserModel;
use crate::models::votes::VoteModel;
use crate::views::helpers;

#[derive(Debug, Deserialize)]
pub struct VoteForm {
    pub vote: i16,
    #[serde(default)]
    pub csrf_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CommentForm {
    pub body: String,
    #[serde(default)]
    pub parent_id: Option<Uuid>,
    #[serde(default)]
    pub csrf_token: Option<String>,
}

#[instrument(skip(v, ctx, theme, current_user, jar))]
pub async fn detail(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    OptionalAuthUser(current_user): OptionalAuthUser,
    jar: CookieJar,
    Extension(theme): Extension<CurrentTheme>,
    Path((slug, post_id)): Path<(String, Uuid)>,
) -> Result<Response> {
    let community = CommunityModel::find_by_slug(&ctx.db, &slug)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    let post = PostModel::find_by_id(&ctx.db, post_id)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    if post.community_id != Some(community.id) {
        return Err(Error::NotFound);
    }

    let author = UserModel::find_by_id(&ctx.db, post.author_id)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    let body_rendered = post
        .body
        .as_deref()
        .map(render_markdown)
        .unwrap_or_default();

    let comments = CommentModel::list_for_post(&ctx.db, post_id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    let comments_with_author: Vec<serde_json::Value> = {
        let mut result = Vec::new();
        for comment in &comments {
            let author = UserModel::find_by_id(&ctx.db, comment.author_id)
                .await
                .map_err(|_| Error::InternalServerError)?;
            let depth = comment.path.matches('.').count() + 1;
            result.push(json!({
                "id": comment.id,
                "ap_id": comment.ap_id,
                "post_id": comment.post_id,
                "author_id": comment.author_id,
                "parent_id": comment.parent_id,
                "path": comment.path,
                "body": comment.body,
                "score": comment.score,
                "is_deleted": comment.is_deleted,
                "is_removed": comment.is_removed,
                "published_at": comment.published_at,
                "created_at": comment.created_at,
                "updated_at": comment.updated_at,
                "author_username": author.as_ref().map(|a| a.username.as_str()).unwrap_or("unknown"),
                "depth": depth,
            }));
        }
        result
    };

    let user_vote = if let Some(ref user) = current_user {
        VoteModel::get_user_vote_on_post(&ctx.db, user.id, post_id)
            .await
            .map_err(|_| Error::InternalServerError)?
    } else {
        None
    };

    let score_class = if post.score > 0 {
        "positive"
    } else if post.score < 0 {
        "negative"
    } else {
        ""
    };

    let csrf_token = uuid::Uuid::new_v4().to_string();
    let csrf_cookie = Cookie::build(("csrf_token", csrf_token.clone()))
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(cfg!(not(debug_assertions)))
        .path("/")
        .build();
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
            "user_vote": user_vote,
            "score_class": score_class,
            "target_id": post.id,
            "vote_url": format!("/c/{}/posts/{}/vote", slug, post.id),
            "score": post.score,
            "slug": slug,
            "csrf_token": csrf_token,
        }),
        &theme,
    )?;
    Ok((jar.add(csrf_cookie), response).into_response())
}

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
    let community = CommunityModel::find_by_slug(&ctx.db, &slug)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    let post = PostModel::find_by_id(&ctx.db, post_id)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    if post.community_id != Some(community.id) {
        return Err(Error::NotFound);
    }

    if !validate_csrf(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
    }

    VoteModel::cast_vote(&ctx.db, current_user.id, Some(post_id), None, form.vote)
        .await
        .map_err(|_| Error::InternalServerError)?;

    let post = PostModel::find_by_id(&ctx.db, post_id)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    let user_vote = VoteModel::get_user_vote_on_post(&ctx.db, current_user.id, post_id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    let score_class = if post.score > 0 {
        "positive"
    } else if post.score < 0 {
        "negative"
    } else {
        ""
    };

    helpers::render(
        &v,
        "partials/vote_buttons.html",
        json!({
            "target_type": "post",
            "target_id": post_id,
            "vote_url": format!("/c/{}/posts/{}/vote", slug, post_id),
            "score": post.score,
            "user_vote": user_vote,
            "score_class": score_class,
            "slug": slug,
            "csrf_token": form.csrf_token,
        }),
    )
}

#[instrument(skip(ctx, jar, headers, taverna))]
pub async fn delete(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path((slug, post_id)): Path<(String, Uuid)>,
) -> Result<Response> {
    let community = CommunityModel::find_by_slug(&ctx.db, &slug)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    let post = PostModel::find_by_id(&ctx.db, post_id)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    if post.community_id != Some(community.id) {
        return Err(Error::NotFound);
    }

    if post.author_id != current_user.id && !current_user.is_admin {
        return Err(Error::Unauthorized("Not authorized".to_string()));
    }

    if !validate_csrf(&jar, None, &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
    }

    PostModel::soft_delete(&ctx.db, post_id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    Ok(format::redirect(&format!("/c/{}", slug)).into_response())
}

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
    let community = CommunityModel::find_by_slug(&ctx.db, &slug)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    let post = PostModel::find_by_id(&ctx.db, post_id)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    if post.community_id != Some(community.id) {
        return Err(Error::NotFound);
    }

    if !validate_csrf(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
    }

    CommentModel::create(
        &ctx.db,
        post_id,
        current_user.id,
        form.parent_id,
        &form.body,
        &taverna.base_url,
    )
    .await
    .map_err(|_| Error::InternalServerError)?;

    PostModel::increment_comment_count(&ctx.db, post_id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    Ok(format::redirect(&format!("/c/{}/posts/{}", slug, post_id)).into_response())
}

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
    let _community = CommunityModel::find_by_slug(&ctx.db, &slug)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    let comment = CommentModel::find_by_id(&ctx.db, comment_id)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    if comment.post_id != post_id {
        return Err(Error::NotFound);
    }

    if !validate_csrf(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
    }

    VoteModel::cast_vote(&ctx.db, current_user.id, None, Some(comment_id), form.vote)
        .await
        .map_err(|_| Error::InternalServerError)?;

    let comment = CommentModel::find_by_id(&ctx.db, comment_id)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    let user_vote = VoteModel::get_user_vote_on_comment(&ctx.db, current_user.id, comment_id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    let score_class = if comment.score > 0 {
        "positive"
    } else if comment.score < 0 {
        "negative"
    } else {
        ""
    };

    helpers::render(
        &v,
        "partials/vote_buttons.html",
        json!({
            "target_type": "comment",
            "target_id": comment_id,
            "vote_url": format!("/c/{}/posts/{}/comments/{}/vote", slug, post_id, comment_id),
            "score": comment.score,
            "user_vote": user_vote,
            "score_class": score_class,
            "slug": slug,
            "csrf_token": form.csrf_token,
        }),
    )
}
