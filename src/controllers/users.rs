use axum::extract::{Path, Query};
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
use crate::initializers::content_negotiation::RenderMode;
use crate::initializers::theme_middleware::CurrentTheme;
use crate::models::comments::CommentModel;
use crate::models::feed::CursorQuery;
use crate::models::follows::Entity as FollowEntity;
use crate::models::posts::PostModel;
use crate::models::users::UserModel;
use crate::views::helpers;

const PER_PAGE: u64 = 20;

/// Form data for creating a profile post.
#[derive(Debug, Deserialize)]
pub struct CreateProfilePostForm {
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub csrf_token: Option<String>,
}

/// Form data for following/unfollowing a user.
#[derive(Debug, Deserialize)]
pub struct FollowForm {
    #[serde(default)]
    pub csrf_token: Option<String>,
}

/// Form data for updating user profile settings.
#[derive(Debug, Deserialize)]
pub struct SettingsForm {
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub avatar_url: Option<String>,
    pub email: Option<String>,
    #[serde(default)]
    pub csrf_token: Option<String>,
}

/// Form data for changing a user's password.
#[derive(Debug, Deserialize)]
pub struct PasswordForm {
    pub current_password: String,
    pub new_password: String,
    pub new_password_confirm: String,
    #[serde(default)]
    pub csrf_token: Option<String>,
}

/// Shows a user's public profile with their posts and follower counts.
#[allow(clippy::too_many_arguments)]
#[instrument(skip(v, ctx, current_user, jar, taverna, theme, render_mode))]
pub async fn profile(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    OptionalAuthUser(current_user): OptionalAuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    Extension(theme): Extension<CurrentTheme>,
    Extension(render_mode): Extension<RenderMode>,
    Path(username): Path<String>,
) -> Result<Response> {
    let user = UserModel::find_by_username(&ctx.db, &username)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    match render_mode {
        RenderMode::ActivityPub => {
            let actor = serde_json::json!({
                "@context": [
                    "https://www.w3.org/ns/activitystreams",
                    "https://w3id.org/security/v1"
                ],
                "id": user.ap_id,
                "type": "Person",
                "url": user.ap_id,
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
                "manuallyApprovesFollowers": false,
                "published": user.created_at.format("%+").to_string(),
            });
            return Ok(axum::Json(actor).into_response());
        }
        RenderMode::Html => {}
    }

    let follower_count = FollowEntity::count_followers(&ctx.db, user.id)
        .await
        .map_err(|_| Error::InternalServerError)?;
    let following_count = FollowEntity::count_following(&ctx.db, user.id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    let is_following = if let Some(ref cu) = current_user {
        FollowEntity::find_follow(&ctx.db, cu.id, user.id)
            .await
            .map_err(|_| Error::InternalServerError)?
            .is_some()
    } else {
        false
    };

    let bio_rendered = user.bio.as_deref().map(render_markdown).unwrap_or_default();

    let posts = PostModel::list_for_author(&ctx.db, user.id, None, PER_PAGE)
        .await
        .map_err(|_| Error::InternalServerError)?;

    let display_name = user
        .display_name
        .clone()
        .unwrap_or_else(|| user.username.clone());
    let avatar_letter = user.username.chars().next().unwrap_or('?').to_string();

    let csrf_token = Uuid::new_v4().to_string();
    let csrf_cookie = Cookie::build(("csrf_token", csrf_token.clone()))
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(cfg!(not(debug_assertions)))
        .path("/")
        .build();

    let response = helpers::render_with_theme(
        &v,
        "users/profile.html",
        json!({
            "current_user": current_user,
            "profile_user": user,
            "display_name": display_name,
            "bio_rendered": bio_rendered,
            "follower_count": follower_count,
            "following_count": following_count,
            "is_following": is_following,
            "posts": posts.items,
            "next_cursor": posts.next_cursor,
            "active_tab": "posts",
            "csrf_token": csrf_token,
            "avatar_letter": avatar_letter,
            "domain": taverna.domain,
            "username": user.username,
        }),
        &theme,
    )?;
    Ok((jar.add(csrf_cookie), response).into_response())
}

/// Shows a user's profile filtered to the posts tab.
#[allow(clippy::too_many_arguments)]
#[instrument(skip(v, ctx, theme, current_user, jar))]
pub async fn profile_posts(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    OptionalAuthUser(current_user): OptionalAuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    Extension(theme): Extension<CurrentTheme>,
    Path(username): Path<String>,
    Query(page): Query<CursorQuery>,
) -> Result<Response> {
    let user = UserModel::find_by_username(&ctx.db, &username)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    let follower_count = FollowEntity::count_followers(&ctx.db, user.id)
        .await
        .map_err(|_| Error::InternalServerError)?;
    let following_count = FollowEntity::count_following(&ctx.db, user.id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    let is_following = if let Some(ref cu) = current_user {
        FollowEntity::find_follow(&ctx.db, cu.id, user.id)
            .await
            .map_err(|_| Error::InternalServerError)?
            .is_some()
    } else {
        false
    };

    let bio_rendered = user.bio.as_deref().map(render_markdown).unwrap_or_default();

    let posts = PostModel::list_for_author(&ctx.db, user.id, page.cursor().as_ref(), PER_PAGE)
        .await
        .map_err(|_| Error::InternalServerError)?;

    let display_name = user
        .display_name
        .clone()
        .unwrap_or_else(|| user.username.clone());
    let avatar_letter = user.username.chars().next().unwrap_or('?').to_string();

    let csrf_token = Uuid::new_v4().to_string();
    let csrf_cookie = Cookie::build(("csrf_token", csrf_token.clone()))
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(cfg!(not(debug_assertions)))
        .path("/")
        .build();

    let response = helpers::render_with_theme(
        &v,
        "users/profile.html",
        json!({
            "current_user": current_user,
            "profile_user": user,
            "display_name": display_name,
            "bio_rendered": bio_rendered,
            "follower_count": follower_count,
            "following_count": following_count,
            "is_following": is_following,
            "posts": posts.items,
            "next_cursor": posts.next_cursor,
            "active_tab": "posts",
            "csrf_token": csrf_token,
            "avatar_letter": avatar_letter,
            "domain": taverna.domain,
            "username": user.username,
        }),
        &theme,
    )?;

    Ok((jar.add(csrf_cookie), response).into_response())
}

/// Shows a user's profile filtered to the comments tab.
#[allow(clippy::too_many_arguments)]
#[instrument(skip(v, ctx, theme, current_user, jar))]
pub async fn profile_comments(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    OptionalAuthUser(current_user): OptionalAuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    Extension(theme): Extension<CurrentTheme>,
    Path(username): Path<String>,
    Query(page): Query<CursorQuery>,
) -> Result<Response> {
    let user = UserModel::find_by_username(&ctx.db, &username)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    let follower_count = FollowEntity::count_followers(&ctx.db, user.id)
        .await
        .map_err(|_| Error::InternalServerError)?;
    let following_count = FollowEntity::count_following(&ctx.db, user.id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    let is_following = if let Some(ref cu) = current_user {
        FollowEntity::find_follow(&ctx.db, cu.id, user.id)
            .await
            .map_err(|_| Error::InternalServerError)?
            .is_some()
    } else {
        false
    };

    let bio_rendered = user.bio.as_deref().map(render_markdown).unwrap_or_default();

    let comments_with_posts =
        CommentModel::list_for_author(&ctx.db, user.id, page.cursor().as_ref(), PER_PAGE)
            .await
            .map_err(|_| Error::InternalServerError)?;

    let display_name = user
        .display_name
        .clone()
        .unwrap_or_else(|| user.username.clone());
    let avatar_letter = user.username.chars().next().unwrap_or('?').to_string();

    let csrf_token = Uuid::new_v4().to_string();
    let csrf_cookie = Cookie::build(("csrf_token", csrf_token.clone()))
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(cfg!(not(debug_assertions)))
        .path("/")
        .build();

    let response = helpers::render_with_theme(
        &v,
        "users/profile.html",
        json!({
            "current_user": current_user,
            "profile_user": user,
            "display_name": display_name,
            "bio_rendered": bio_rendered,
            "follower_count": follower_count,
            "following_count": following_count,
            "is_following": is_following,
            "comments": comments_with_posts.items,
            "next_cursor": comments_with_posts.next_cursor,
            "active_tab": "comments",
            "csrf_token": csrf_token,
            "avatar_letter": avatar_letter,
            "domain": taverna.domain,
            "username": user.username,
        }),
        &theme,
    )?;
    Ok((jar.add(csrf_cookie), response).into_response())
}

/// Follows a user. Returns either an HTMX partial or a redirect.
#[allow(clippy::too_many_arguments)]
#[instrument(skip(v, ctx, jar, headers, taverna))]
pub async fn follow(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path(username): Path<String>,
    Form(form): Form<FollowForm>,
) -> Result<Response> {
    let target = UserModel::find_by_username(&ctx.db, &username)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    if target.id == current_user.id {
        return Err(Error::Unauthorized("Cannot follow yourself".to_string()));
    }

    if !validate_csrf(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
    }

    FollowEntity::create_local_follow(&ctx.db, current_user.id, target.id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    // Federation: enqueue AP Follow activity if remote
    if taverna.federation.enabled && !target.is_local {
        let ap_context: &[&str] = &[
            "https://www.w3.org/ns/activitystreams",
            "https://w3id.org/security/v1",
        ];
        let activity_id = format!("{}/activities/{}", current_user.ap_id, Uuid::new_v4());
        let follow_activity = serde_json::json!({
            "@context": ap_context,
            "id": activity_id,
            "type": "Follow",
            "actor": current_user.ap_id,
            "object": target.ap_id,
        });
        let inboxes = vec![target.inbox_url.clone()];
        crate::workers::deliver_activity::enqueue_activity(
            &ctx.db,
            &follow_activity,
            &current_user.ap_id,
            inboxes,
        )
        .await
        .map_err(|e| Error::string(&e.to_string()))?;
    }

    let csrf_token = Uuid::new_v4().to_string();
    let csrf_cookie = Cookie::build(("csrf_token", csrf_token.clone()))
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(cfg!(not(debug_assertions)))
        .path("/")
        .build();

    let is_hx = headers
        .get("hx-request")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        == "true";

    if is_hx {
        let html = helpers::render(
            &v,
            "partials/follow_button.html",
            json!({
                "is_following": true,
                "csrf_token": csrf_token,
                "username": username,
            }),
        )?;
        return Ok((jar.add(csrf_cookie), html).into_response());
    }

    Ok((
        jar.add(csrf_cookie),
        format::redirect(&format!("/u/{}", username)),
    )
        .into_response())
}

/// Unfollows a user. Returns either an HTMX partial or a redirect.
#[allow(clippy::too_many_arguments)]
#[instrument(skip(v, ctx, jar, headers, taverna))]
pub async fn unfollow(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path(username): Path<String>,
    Form(form): Form<FollowForm>,
) -> Result<Response> {
    let target = UserModel::find_by_username(&ctx.db, &username)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    if !validate_csrf(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
    }

    FollowEntity::delete_follow(&ctx.db, current_user.id, target.id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    // Federation: enqueue AP Undo{Follow} activity if remote
    if taverna.federation.enabled && !target.is_local {
        let ap_context: &[&str] = &[
            "https://www.w3.org/ns/activitystreams",
            "https://w3id.org/security/v1",
        ];
        let undo_id = format!("{}/activities/{}", current_user.ap_id, Uuid::new_v4());
        let follow_id = format!("{}/follows/{}", current_user.ap_id, target.id);
        let undo_activity = serde_json::json!({
            "@context": ap_context,
            "id": undo_id,
            "type": "Undo",
            "actor": current_user.ap_id,
            "object": {
                "id": follow_id,
                "type": "Follow",
                "actor": current_user.ap_id,
                "object": target.ap_id,
            },
        });
        let inboxes = vec![target.inbox_url.clone()];
        crate::workers::deliver_activity::enqueue_activity(
            &ctx.db,
            &undo_activity,
            &current_user.ap_id,
            inboxes,
        )
        .await
        .map_err(|e| Error::string(&e.to_string()))?;
    }

    let csrf_token = Uuid::new_v4().to_string();
    let csrf_cookie = Cookie::build(("csrf_token", csrf_token.clone()))
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(cfg!(not(debug_assertions)))
        .path("/")
        .build();

    let is_hx = headers
        .get("hx-request")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        == "true";

    if is_hx {
        let html = helpers::render(
            &v,
            "partials/follow_button.html",
            json!({
                "is_following": false,
                "csrf_token": csrf_token,
                "username": username,
            }),
        )?;
        return Ok((jar.add(csrf_cookie), html).into_response());
    }

    Ok((
        jar.add(csrf_cookie),
        format::redirect(&format!("/u/{}", username)),
    )
        .into_response())
}

/// Renders the user settings page.
#[instrument(skip(v, ctx, theme, current_user, jar))]
pub async fn settings_form(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(theme): Extension<CurrentTheme>,
) -> Result<Response> {
    let user = UserModel::find_by_id(&ctx.db, current_user.id)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    let csrf_token = Uuid::new_v4().to_string();
    let csrf_cookie = Cookie::build(("csrf_token", csrf_token.clone()))
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(cfg!(not(debug_assertions)))
        .path("/")
        .build();

    let user_email = user.email.clone();
    let eligibility = crate::models::eligibility::check_eligibility(&ctx.db, current_user.id)
        .await
        .map_err(|_| Error::InternalServerError)?;
    let response = helpers::render_with_theme(
        &v,
        "users/settings.html",
        json!({
            "current_user": current_user,
            "user": user,
            "email": user_email,
            "eligibility": eligibility,
            "csrf_token": csrf_token,
        }),
        &theme,
    )?;
    Ok((jar.add(csrf_cookie), response).into_response())
}

/// Updates user profile settings (display name, bio, avatar, email).
#[instrument(skip(ctx, form, jar, headers, taverna))]
pub async fn update_settings(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Form(form): Form<SettingsForm>,
) -> Result<Response> {
    if !validate_csrf(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
    }

    UserModel::update_profile(
        &ctx.db,
        current_user.id,
        form.display_name.as_deref(),
        form.bio.as_deref(),
        form.avatar_url.as_deref(),
    )
    .await
    .map_err(|_| Error::InternalServerError)?;

    if let Some(ref email) = form.email {
        let trimmed = email.trim();
        if !trimmed.is_empty() {
            UserModel::update_email(&ctx.db, current_user.id, trimmed)
                .await
                .map_err(|e| match e {
                    crate::models::users::UserError::EmailTaken => {
                        Error::BadRequest("Email already taken".to_string())
                    }
                    crate::models::users::UserError::Validation(msg) => Error::BadRequest(msg),
                    _ => Error::InternalServerError,
                })?;
        }
    }

    // Re-fetch persisted user for federation payload
    let persisted_user = UserModel::find_by_id(&ctx.db, current_user.id)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    // Federation: enqueue Update{Person} activity
    if taverna.federation.enabled {
        let ap_context: &[&str] = &[
            "https://www.w3.org/ns/activitystreams",
            "https://w3id.org/security/v1",
        ];
        let activity_id = format!("{}/activities/{}", current_user.ap_id, Uuid::new_v4());
        let person = serde_json::json!({
            "@context": ap_context,
            "id": persisted_user.ap_id,
            "type": "Person",
            "preferredUsername": persisted_user.username,
            "name": persisted_user.display_name,
            "summary": persisted_user.bio,
            "icon": persisted_user.avatar_url,
        });
        let update = serde_json::json!({
            "@context": ap_context,
            "id": activity_id,
            "type": "Update",
            "actor": current_user.ap_id,
            "object": person,
            "to": [current_user.followers_url],
            "published": chrono::Utc::now().to_rfc3339(),
        });
        let inboxes = crate::workers::deliver_activity::get_user_follower_inboxes_deduplicated(
            &ctx.db,
            current_user.id,
        )
        .await
        .map_err(|e| Error::string(&e.to_string()))?;
        if !inboxes.is_empty() {
            crate::workers::deliver_activity::enqueue_activity(
                &ctx.db,
                &update,
                &current_user.ap_id,
                inboxes,
            )
            .await
            .map_err(|e| Error::string(&e.to_string()))?;
        }
    }

    Ok(format::redirect("/settings").into_response())
}

/// Changes the current user's password.
#[instrument(skip(ctx, form, jar, headers))]
pub async fn change_password(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Form(form): Form<PasswordForm>,
) -> Result<Response> {
    if !validate_csrf(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
    }

    if form.new_password != form.new_password_confirm {
        return Err(Error::Unauthorized(
            "New passwords do not match".to_string(),
        ));
    }

    UserModel::change_password(
        &ctx.db,
        current_user.id,
        &form.current_password,
        &form.new_password,
    )
    .await
    .map_err(|e| match e {
        crate::models::users::UserError::Validation(msg) => Error::Unauthorized(msg),
        _ => Error::InternalServerError,
    })?;

    Ok(format::redirect("/settings").into_response())
}

/// Shows the form to create a new profile post.
#[instrument(skip(v, ctx, theme, current_user, jar))]
pub async fn profile_new_post(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(theme): Extension<CurrentTheme>,
    Path(username): Path<String>,
) -> Result<Response> {
    let _user = UserModel::find_by_username(&ctx.db, &username)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    if current_user.username != username {
        return Err(Error::Unauthorized("Not authorized".to_string()));
    }

    let csrf_token = Uuid::new_v4().to_string();
    let csrf_cookie = Cookie::build(("csrf_token", csrf_token.clone()))
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(cfg!(not(debug_assertions)))
        .path("/")
        .build();

    let response = helpers::render_with_theme(
        &v,
        "users/new_post.html",
        json!({
            "current_user": current_user,
            "csrf_token": csrf_token,
            "username": username,
        }),
        &theme,
    )?;
    Ok((jar.add(csrf_cookie), response).into_response())
}

/// Creates a new profile post (not in a community) with federation support.
#[instrument(skip(ctx, form, jar, headers, taverna))]
pub async fn create_profile_post(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path(username): Path<String>,
    Form(form): Form<CreateProfilePostForm>,
) -> Result<Response> {
    if current_user.username != username {
        return Err(Error::Unauthorized("Not authorized".to_string()));
    }

    if !validate_csrf(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
    }

    let post = PostModel::create(
        &ctx.db,
        None,
        current_user.id,
        &form.title,
        form.body.as_deref(),
        form.url.as_deref(),
        &taverna.base_url,
    )
    .await
    .map_err(|_| Error::InternalServerError)?;

    // Federation: enqueue Create activity to user followers
    if taverna.federation.enabled {
        let ap_context: &[&str] = &[
            "https://www.w3.org/ns/activitystreams",
            "https://w3id.org/security/v1",
        ];

        let note = serde_json::json!({
            "@context": ap_context,
            "id": post.ap_id,
            "type": "Note",
            "attributedTo": current_user.ap_id,
            "content": post.body,
            "name": post.title,
            "published": post.published_at.to_rfc3339(),
            "url": post.url,
            "to": [current_user.followers_url],
            "cc": ["https://www.w3.org/ns/activitystreams#Public"],
        });

        let activity_id = format!("{}/activities/{}", current_user.ap_id, Uuid::new_v4());
        let create = serde_json::json!({
            "@context": ap_context,
            "id": activity_id,
            "type": "Create",
            "actor": current_user.ap_id,
            "object": note,
            "to": [current_user.followers_url],
            "cc": ["https://www.w3.org/ns/activitystreams#Public"],
            "published": chrono::Utc::now().to_rfc3339(),
        });

        let inboxes = crate::workers::deliver_activity::get_user_follower_inboxes_deduplicated(
            &ctx.db,
            current_user.id,
        )
        .await
        .map_err(|e| Error::string(&e.to_string()))?;
        if !inboxes.is_empty() {
            crate::workers::deliver_activity::enqueue_activity(
                &ctx.db,
                &create,
                &current_user.ap_id,
                inboxes,
            )
            .await
            .map_err(|e| Error::string(&e.to_string()))?;
        }
    }

    Ok(format::redirect(&format!("/u/{}", username)).into_response())
}
