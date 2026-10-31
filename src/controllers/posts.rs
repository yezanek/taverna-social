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
use crate::models::community_bans::Entity as CommunityBansEntity;
use crate::models::posts::PostModel;
use crate::models::remote::RemoteCommentModel;
use crate::models::users::UserModel;
use crate::models::votes::VoteModel;
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

    let remote_comments: Vec<serde_json::Value> =
        RemoteCommentModel::list_for_local_post(&ctx.db, post_id)
            .await
            .map_err(|_| Error::InternalServerError)?
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
            "remote_comments": remote_comments,
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

    // Federation: enqueue Like/Dislike activity
    if taverna.federation.enabled {
        let activity_id = format!("{}/activities/{}", current_user.ap_id, uuid::Uuid::new_v4());
        let ap_context: &[&str] = &[
            "https://www.w3.org/ns/activitystreams",
            "https://w3id.org/security/v1",
        ];
        let vote_activity = if form.vote > 0 {
            serde_json::json!({
                "@context": ap_context,
                "id": activity_id,
                "type": "Like",
                "actor": current_user.ap_id,
                "object": post.ap_id,
                "to": ["https://www.w3.org/ns/activitystreams#Public"],
                "published": chrono::Utc::now().to_rfc3339(),
            })
        } else {
            serde_json::json!({
                "@context": ap_context,
                "id": activity_id,
                "type": "Dislike",
                "actor": current_user.ap_id,
                "object": post.ap_id,
                "to": ["https://www.w3.org/ns/activitystreams#Public"],
                "published": chrono::Utc::now().to_rfc3339(),
            })
        };
        let post_author = UserModel::find_by_id(&ctx.db, post.author_id)
            .await
            .map_err(|_| Error::InternalServerError)?
            .ok_or_else(|| Error::InternalServerError)?;
        let inboxes = if post_author.is_local {
            Vec::new()
        } else {
            vec![post_author.inbox_url.clone()]
        };
        if !inboxes.is_empty() {
            let _ = crate::workers::deliver_activity::enqueue_activity(
                &ctx.db,
                &vote_activity,
                &current_user.ap_id,
                inboxes,
            )
            .await;
        }
    }

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

/// Renders the edit form for a post (author only).
#[allow(clippy::too_many_arguments)]
#[instrument(skip(v, ctx, theme, current_user, jar))]
pub async fn edit_form(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
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

    if post.author_id != current_user.id {
        return Err(Error::Unauthorized("Not authorized".to_string()));
    }

    let csrf_token = uuid::Uuid::new_v4().to_string();
    let csrf_cookie = Cookie::build(("csrf_token", csrf_token.clone()))
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(cfg!(not(debug_assertions)))
        .path("/")
        .build();

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
#[allow(clippy::too_many_arguments)]
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

    if post.author_id != current_user.id {
        return Err(Error::Unauthorized("Not authorized".to_string()));
    }

    if !validate_csrf(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
    }

    let updated = PostModel::update(
        &ctx.db,
        post_id,
        &form.title,
        form.body.as_deref(),
        form.url.as_deref(),
    )
    .await
    .map_err(|_| Error::InternalServerError)?;

    // Federation: enqueue Update{Note} activity
    if taverna.federation.enabled {
        let ap_context: &[&str] = &[
            "https://www.w3.org/ns/activitystreams",
            "https://w3id.org/security/v1",
        ];
        let activity_id = format!("{}/activities/{}", current_user.ap_id, uuid::Uuid::new_v4());
        let note = serde_json::json!({
            "@context": ap_context,
            "id": updated.ap_id,
            "type": "Note",
            "attributedTo": community.ap_id,
            "content": updated.body,
            "name": updated.title,
            "published": updated.published_at.to_rfc3339(),
            "url": updated.url,
            "to": ["https://www.w3.org/ns/activitystreams#Public"],
            "cc": [community.followers_url],
        });
        let update = serde_json::json!({
            "@context": ap_context,
            "id": activity_id,
            "type": "Update",
            "actor": current_user.ap_id,
            "object": note,
            "to": ["https://www.w3.org/ns/activitystreams#Public"],
            "cc": [community.followers_url],
            "published": chrono::Utc::now().to_rfc3339(),
        });

        let inboxes = crate::workers::deliver_activity::get_follower_inboxes_deduplicated(
            &ctx.db,
            community.id,
        )
        .await
        .unwrap_or_default();
        if !inboxes.is_empty() {
            let _ = crate::workers::deliver_activity::enqueue_activity(
                &ctx.db,
                &update,
                &current_user.ap_id,
                inboxes,
            )
            .await;
        }
    }

    Ok(format::redirect(&format!("/c/{}/posts/{}", slug, post_id)).into_response())
}

/// Soft-deletes a post (author, moderator, or admin). Enqueues AP Delete activity.
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

    let role = CommunityModel::get_member_role(&ctx.db, community.id, current_user.id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    let is_mod = role.as_deref() == Some("moderator");

    if post.author_id != current_user.id && !is_mod && !current_user.is_admin {
        return Err(Error::Unauthorized("Not authorized".to_string()));
    }

    if !validate_csrf(&jar, None, &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
    }

    PostModel::soft_delete(&ctx.db, post_id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    // Federation: enqueue Delete activity
    if taverna.federation.enabled {
        let activity_id = format!("{}/activities/{}", current_user.ap_id, uuid::Uuid::new_v4());
        let ap_context: &[&str] = &[
            "https://www.w3.org/ns/activitystreams",
            "https://w3id.org/security/v1",
        ];
        let delete = serde_json::json!({
            "@context": ap_context,
            "id": activity_id,
            "type": "Delete",
            "actor": current_user.ap_id,
            "object": post.ap_id,
            "to": ["https://www.w3.org/ns/activitystreams#Public"],
            "published": chrono::Utc::now().to_rfc3339(),
        });
        if let Some(community_id) = post.community_id {
            let inboxes = crate::workers::deliver_activity::get_follower_inboxes_deduplicated(
                &ctx.db,
                community_id,
            )
            .await
            .unwrap_or_default();
            if !inboxes.is_empty() {
                let _ = crate::workers::deliver_activity::enqueue_activity(
                    &ctx.db,
                    &delete,
                    &current_user.ap_id,
                    inboxes,
                )
                .await;
            }
        }
    }

    Ok(format::redirect(&format!("/c/{}", slug)).into_response())
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

    if post.is_locked {
        return Err(Error::CustomError(
            axum::http::StatusCode::FORBIDDEN,
            loco_rs::controller::ErrorDetail::with_reason("Post is locked"),
        ));
    }

    if let Some(community_id) = post.community_id
        && CommunityBansEntity::is_banned(&ctx.db, community_id, current_user.id)
            .await
            .map_err(|_| Error::InternalServerError)?
    {
        return Err(Error::CustomError(
            axum::http::StatusCode::FORBIDDEN,
            loco_rs::controller::ErrorDetail::with_reason("You are banned from this community"),
        ));
    }

    if !validate_csrf(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
    }

    let comment = CommentModel::create(
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

    // Federation: enqueue Create activity for comment
    if taverna.federation.enabled {
        let ap_context: &[&str] = &[
            "https://www.w3.org/ns/activitystreams",
            "https://w3id.org/security/v1",
        ];
        let note = serde_json::json!({
            "@context": ap_context,
            "id": comment.ap_id,
            "type": "Note",
            "attributedTo": current_user.ap_id,
            "content": comment.body,
            "inReplyTo": post.ap_id,
            "published": comment.published_at.to_rfc3339(),
            "url": comment.ap_id,
            "to": ["https://www.w3.org/ns/activitystreams#Public"],
            "cc": [],
        });
        let activity_id = format!("{}/activities/{}", current_user.ap_id, uuid::Uuid::new_v4());
        let create = serde_json::json!({
            "@context": ap_context,
            "id": activity_id,
            "type": "Create",
            "actor": current_user.ap_id,
            "object": note,
            "to": ["https://www.w3.org/ns/activitystreams#Public"],
            "cc": [],
            "published": chrono::Utc::now().to_rfc3339(),
        });
        if let Some(community_id) = post.community_id {
            let inboxes = crate::workers::deliver_activity::get_follower_inboxes_deduplicated(
                &ctx.db,
                community_id,
            )
            .await
            .unwrap_or_default();
            if !inboxes.is_empty() {
                let _ = crate::workers::deliver_activity::enqueue_activity(
                    &ctx.db,
                    &create,
                    &current_user.ap_id,
                    inboxes,
                )
                .await;
            }
        }
    }

    Ok(format::redirect(&format!("/c/{}/posts/{}", slug, post_id)).into_response())
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

    // Federation: enqueue Like/Dislike activity for comment
    if taverna.federation.enabled {
        let activity_id = format!("{}/activities/{}", current_user.ap_id, uuid::Uuid::new_v4());
        let ap_context: &[&str] = &[
            "https://www.w3.org/ns/activitystreams",
            "https://w3id.org/security/v1",
        ];
        let vote_activity = if form.vote > 0 {
            serde_json::json!({
                "@context": ap_context,
                "id": activity_id,
                "type": "Like",
                "actor": current_user.ap_id,
                "object": comment.ap_id,
                "to": ["https://www.w3.org/ns/activitystreams#Public"],
                "published": chrono::Utc::now().to_rfc3339(),
            })
        } else {
            serde_json::json!({
                "@context": ap_context,
                "id": activity_id,
                "type": "Dislike",
                "actor": current_user.ap_id,
                "object": comment.ap_id,
                "to": ["https://www.w3.org/ns/activitystreams#Public"],
                "published": chrono::Utc::now().to_rfc3339(),
            })
        };
        let comment_author = UserModel::find_by_id(&ctx.db, comment.author_id)
            .await
            .map_err(|_| Error::InternalServerError)?
            .ok_or_else(|| Error::InternalServerError)?;
        let inboxes = if !comment_author.is_local {
            vec![comment_author.inbox_url.clone()]
        } else {
            Vec::new()
        };
        if !inboxes.is_empty() {
            let _ = crate::workers::deliver_activity::enqueue_activity(
                &ctx.db,
                &vote_activity,
                &current_user.ap_id,
                inboxes,
            )
            .await;
        }
    }

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

/// Removes a post (moderator or admin only).
#[instrument(skip(ctx, jar, headers, taverna))]
pub async fn remove(
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

    let role = CommunityModel::get_member_role(&ctx.db, community.id, current_user.id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    if role.as_deref() != Some("moderator") && !current_user.is_admin {
        return Err(Error::Unauthorized("Moderator access required".to_string()));
    }

    if !validate_csrf(&jar, None, &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
    }

    let post = PostModel::find_by_id(&ctx.db, post_id)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    if post.community_id != Some(community.id) {
        return Err(Error::NotFound);
    }

    PostModel::soft_remove(&ctx.db, post_id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    Ok(format::redirect(&format!("/c/{}", slug)).into_response())
}

/// Toggles a post's pinned status (moderator or admin only).
#[instrument(skip(ctx, jar, headers, taverna))]
pub async fn pin(
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

    let role = CommunityModel::get_member_role(&ctx.db, community.id, current_user.id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    if role.as_deref() != Some("moderator") && !current_user.is_admin {
        return Err(Error::Unauthorized("Moderator access required".to_string()));
    }

    if !validate_csrf(&jar, None, &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
    }

    let post = PostModel::find_by_id(&ctx.db, post_id)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    if post.community_id != Some(community.id) {
        return Err(Error::NotFound);
    }

    PostModel::toggle_pin(&ctx.db, post_id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    Ok(format::redirect(&format!("/c/{}/posts/{}", slug, post_id)).into_response())
}

/// Toggles a post's locked status (moderator or admin only).
#[instrument(skip(ctx, jar, headers, taverna))]
pub async fn lock(
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

    let role = CommunityModel::get_member_role(&ctx.db, community.id, current_user.id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    if role.as_deref() != Some("moderator") && !current_user.is_admin {
        return Err(Error::Unauthorized("Moderator access required".to_string()));
    }

    if !validate_csrf(&jar, None, &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
    }

    let post = PostModel::find_by_id(&ctx.db, post_id)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    if post.community_id != Some(community.id) {
        return Err(Error::NotFound);
    }

    PostModel::toggle_lock(&ctx.db, post_id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    Ok(format::redirect(&format!("/c/{}/posts/{}", slug, post_id)).into_response())
}

/// Renders the edit form for a comment (author only).
#[allow(clippy::too_many_arguments)]
#[instrument(skip(v, ctx, theme, current_user, jar))]
pub async fn edit_comment_form(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(theme): Extension<CurrentTheme>,
    Path((slug, post_id, comment_id)): Path<(String, Uuid, Uuid)>,
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

    let comment = CommentModel::find_by_id(&ctx.db, comment_id)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    if comment.post_id != post_id {
        return Err(Error::NotFound);
    }

    if comment.author_id != current_user.id {
        return Err(Error::Unauthorized("Not authorized".to_string()));
    }

    let csrf_token = uuid::Uuid::new_v4().to_string();
    let csrf_cookie = Cookie::build(("csrf_token", csrf_token.clone()))
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(cfg!(not(debug_assertions)))
        .path("/")
        .build();

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
#[allow(clippy::too_many_arguments)]
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

    let comment = CommentModel::find_by_id(&ctx.db, comment_id)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    if comment.post_id != post_id {
        return Err(Error::NotFound);
    }

    if comment.author_id != current_user.id {
        return Err(Error::Unauthorized("Not authorized".to_string()));
    }

    if !validate_csrf(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
    }

    let updated = CommentModel::update(&ctx.db, comment_id, &form.body)
        .await
        .map_err(|_| Error::InternalServerError)?;

    // Federation: enqueue Update{Note} activity
    if taverna.federation.enabled {
        let ap_context: &[&str] = &[
            "https://www.w3.org/ns/activitystreams",
            "https://w3id.org/security/v1",
        ];
        let activity_id = format!("{}/activities/{}", current_user.ap_id, uuid::Uuid::new_v4());
        let note = serde_json::json!({
            "@context": ap_context,
            "id": updated.ap_id,
            "type": "Note",
            "attributedTo": current_user.ap_id,
            "content": updated.body,
            "inReplyTo": post.ap_id,
            "published": updated.published_at.to_rfc3339(),
            "url": updated.ap_id,
            "to": ["https://www.w3.org/ns/activitystreams#Public"],
            "cc": [],
        });
        let update = serde_json::json!({
            "@context": ap_context,
            "id": activity_id,
            "type": "Update",
            "actor": current_user.ap_id,
            "object": note,
            "to": ["https://www.w3.org/ns/activitystreams#Public"],
            "cc": [],
            "published": chrono::Utc::now().to_rfc3339(),
        });
        if let Some(community_id) = post.community_id {
            let inboxes = crate::workers::deliver_activity::get_follower_inboxes_deduplicated(
                &ctx.db,
                community_id,
            )
            .await
            .unwrap_or_default();
            if !inboxes.is_empty() {
                let _ = crate::workers::deliver_activity::enqueue_activity(
                    &ctx.db,
                    &update,
                    &current_user.ap_id,
                    inboxes,
                )
                .await;
            }
        }
    }

    Ok(format::redirect(&format!("/c/{}/posts/{}", slug, post_id)).into_response())
}

/// Removes a comment (moderator or admin only).
#[allow(clippy::too_many_arguments)]
#[instrument(skip(ctx, jar, headers, taverna))]
pub async fn remove_comment(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path((slug, post_id, comment_id)): Path<(String, Uuid, Uuid)>,
) -> Result<Response> {
    let community = CommunityModel::find_by_slug(&ctx.db, &slug)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    let role = CommunityModel::get_member_role(&ctx.db, community.id, current_user.id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    if role.as_deref() != Some("moderator") && !current_user.is_admin {
        return Err(Error::Unauthorized("Moderator access required".to_string()));
    }

    if !validate_csrf(&jar, None, &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
    }

    let post = PostModel::find_by_id(&ctx.db, post_id)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    if post.community_id != Some(community.id) {
        return Err(Error::NotFound);
    }

    let comment = CommentModel::find_by_id(&ctx.db, comment_id)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    if comment.post_id != post_id {
        return Err(Error::NotFound);
    }

    CommentModel::soft_delete(&ctx.db, comment_id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    // Federation: enqueue Delete activity for comment
    if taverna.federation.enabled {
        let ap_context: &[&str] = &[
            "https://www.w3.org/ns/activitystreams",
            "https://w3id.org/security/v1",
        ];
        let activity_id = format!("{}/activities/{}", current_user.ap_id, uuid::Uuid::new_v4());
        let delete = serde_json::json!({
            "@context": ap_context,
            "id": activity_id,
            "type": "Delete",
            "actor": current_user.ap_id,
            "object": comment.ap_id,
            "to": ["https://www.w3.org/ns/activitystreams#Public"],
            "published": chrono::Utc::now().to_rfc3339(),
        });
        if let Some(community_id) = post.community_id {
            let inboxes = crate::workers::deliver_activity::get_follower_inboxes_deduplicated(
                &ctx.db,
                community_id,
            )
            .await
            .unwrap_or_default();
            if !inboxes.is_empty() {
                let _ = crate::workers::deliver_activity::enqueue_activity(
                    &ctx.db,
                    &delete,
                    &current_user.ap_id,
                    inboxes,
                )
                .await;
            }
        }
    }

    Ok(format::redirect(&format!("/c/{}/posts/{}", slug, post_id)).into_response())
}
