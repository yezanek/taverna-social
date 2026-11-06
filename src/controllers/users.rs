use crate::controllers::errors::{forbidden, invalid_csrf};
use axum::extract::{Path, Query};
use axum::http::HeaderMap;
use axum::{Extension, Form};
use axum_extra::extract::cookie::CookieJar;
use loco_rs::prelude::*;
use serde::Deserialize;
use serde_json::json;
use tracing::instrument;

use crate::common::accent::Rgb;
use crate::common::config::TavernaConfig;
use crate::common::markdown::render_markdown;
use crate::controllers::csrf;
use crate::controllers::theme::{accent_cookie, theme_cookie};
use crate::federation::{self, outbound, traits::DbUser};
use crate::initializers::auth_middleware::{AuthUser, OptionalAuthUser};
use crate::initializers::content_negotiation::RenderMode;
use crate::initializers::theme_middleware::Theme;
use crate::models::comments::CommentModel;
use crate::models::feed::CursorQuery;
use crate::models::follows::Entity as FollowEntity;
use crate::models::posts::PostModel;
use crate::models::users::{UserError, UserModel};
use crate::views::Views;
use activitypub_federation::{config::Data, traits::Object};
use sea_orm::DatabaseConnection;

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

/// Form data for the appearance settings. `accent_reset` is set by the
/// "Reset to default" button.
#[derive(Debug, Deserialize)]
pub struct AppearanceForm {
    pub theme: String,
    #[serde(default)]
    pub accent: Option<String>,
    #[serde(default)]
    pub accent_reset: Option<String>,
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
#[instrument(skip(views, ctx, current_user, jar, taverna, render_mode, data))]
pub async fn profile(
    views: Views,
    State(ctx): State<AppContext>,
    OptionalAuthUser(current_user): OptionalAuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    Extension(render_mode): Extension<RenderMode>,
    data: Data<DatabaseConnection>,
    Path(username): Path<String>,
) -> Result<Response> {
    let user = UserModel::find_by_username(&ctx.db, &username)
        .await?
        .ok_or_else(|| Error::NotFound)?;

    match render_mode {
        RenderMode::ActivityPub => {
            let person = DbUser::new(user)
                .map_err(Error::wrap)?
                .into_json(&data)
                .await
                .map_err(federation::internal)?;
            return Ok(federation::ap_response(person));
        }
        RenderMode::Html => {}
    }

    let follower_count = FollowEntity::count_followers(&ctx.db, user.id).await?;
    let following_count = FollowEntity::count_following(&ctx.db, user.id).await?;

    let is_following = if let Some(ref cu) = current_user {
        FollowEntity::find_follow(&ctx.db, cu.id, user.id)
            .await?
            .is_some()
    } else {
        false
    };

    let bio_rendered = user.bio.as_deref().map(render_markdown).unwrap_or_default();

    let posts = PostModel::list_for_author(&ctx.db, user.id, None, PER_PAGE).await?;

    let display_name = user
        .display_name
        .clone()
        .unwrap_or_else(|| user.username.clone());
    let avatar_letter = user.username.chars().next().unwrap_or('?').to_string();

    let (csrf_token, csrf_cookie) = csrf::issue();

    let response = views.render(
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
    )?;
    Ok((jar.add(csrf_cookie), response).into_response())
}

/// Shows a user's profile filtered to the posts tab.
#[allow(clippy::too_many_arguments)]
#[instrument(skip(views, ctx, current_user, jar))]
pub async fn profile_posts(
    views: Views,
    State(ctx): State<AppContext>,
    OptionalAuthUser(current_user): OptionalAuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    Path(username): Path<String>,
    Query(page): Query<CursorQuery>,
) -> Result<Response> {
    let user = UserModel::find_by_username(&ctx.db, &username)
        .await?
        .ok_or_else(|| Error::NotFound)?;

    let follower_count = FollowEntity::count_followers(&ctx.db, user.id).await?;
    let following_count = FollowEntity::count_following(&ctx.db, user.id).await?;

    let is_following = if let Some(ref cu) = current_user {
        FollowEntity::find_follow(&ctx.db, cu.id, user.id)
            .await?
            .is_some()
    } else {
        false
    };

    let bio_rendered = user.bio.as_deref().map(render_markdown).unwrap_or_default();

    let posts =
        PostModel::list_for_author(&ctx.db, user.id, page.cursor().as_ref(), PER_PAGE).await?;

    let display_name = user
        .display_name
        .clone()
        .unwrap_or_else(|| user.username.clone());
    let avatar_letter = user.username.chars().next().unwrap_or('?').to_string();

    let (csrf_token, csrf_cookie) = csrf::issue();

    let response = views.render(
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
    )?;

    Ok((jar.add(csrf_cookie), response).into_response())
}

/// Shows a user's profile filtered to the comments tab.
#[allow(clippy::too_many_arguments)]
#[instrument(skip(views, ctx, current_user, jar))]
pub async fn profile_comments(
    views: Views,
    State(ctx): State<AppContext>,
    OptionalAuthUser(current_user): OptionalAuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    Path(username): Path<String>,
    Query(page): Query<CursorQuery>,
) -> Result<Response> {
    let user = UserModel::find_by_username(&ctx.db, &username)
        .await?
        .ok_or_else(|| Error::NotFound)?;

    let follower_count = FollowEntity::count_followers(&ctx.db, user.id).await?;
    let following_count = FollowEntity::count_following(&ctx.db, user.id).await?;

    let is_following = if let Some(ref cu) = current_user {
        FollowEntity::find_follow(&ctx.db, cu.id, user.id)
            .await?
            .is_some()
    } else {
        false
    };

    let bio_rendered = user.bio.as_deref().map(render_markdown).unwrap_or_default();

    let comments_with_posts =
        CommentModel::list_for_author(&ctx.db, user.id, page.cursor().as_ref(), PER_PAGE).await?;

    let display_name = user
        .display_name
        .clone()
        .unwrap_or_else(|| user.username.clone());
    let avatar_letter = user.username.chars().next().unwrap_or('?').to_string();

    let (csrf_token, csrf_cookie) = csrf::issue();

    let response = views.render(
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
    )?;
    Ok((jar.add(csrf_cookie), response).into_response())
}

/// Follows a user. Returns either an HTMX partial or a redirect.
#[allow(clippy::too_many_arguments)]
#[instrument(skip(views, ctx, jar, headers, taverna))]
pub async fn follow(
    views: Views,
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path(username): Path<String>,
    Form(form): Form<FollowForm>,
) -> Result<Response> {
    let target = UserModel::find_by_username(&ctx.db, &username)
        .await?
        .ok_or_else(|| Error::NotFound)?;

    if target.id == current_user.id {
        return Err(Error::BadRequest("Cannot follow yourself".to_string()));
    }

    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }

    FollowEntity::create_local_follow(&ctx.db, current_user.id, target.id).await?;

    // `users` holds local accounts only (ADR-017), so a user-to-user follow
    // never needs federating; remote follows arrive through the inbox.
    let (csrf_token, csrf_cookie) = csrf::issue();

    let is_hx = headers
        .get("hx-request")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        == "true";

    if is_hx {
        let html = views.render(
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
#[instrument(skip(views, ctx, jar, headers, taverna))]
pub async fn unfollow(
    views: Views,
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path(username): Path<String>,
    Form(form): Form<FollowForm>,
) -> Result<Response> {
    let target = UserModel::find_by_username(&ctx.db, &username)
        .await?
        .ok_or_else(|| Error::NotFound)?;

    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }

    FollowEntity::delete_follow(&ctx.db, current_user.id, target.id).await?;

    // `users` holds local accounts only (ADR-017), so a user-to-user follow
    // never needs federating; remote follows arrive through the inbox.
    let (csrf_token, csrf_cookie) = csrf::issue();

    let is_hx = headers
        .get("hx-request")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        == "true";

    if is_hx {
        let html = views.render(
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
#[instrument(skip(views, ctx, current_user, jar))]
pub async fn settings_form(
    views: Views,
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
) -> Result<Response> {
    let user = UserModel::find_by_id(&ctx.db, current_user.id)
        .await?
        .ok_or_else(|| Error::NotFound)?;

    let (csrf_token, csrf_cookie) = csrf::issue();

    let user_email = user.email.clone();
    let eligibility =
        crate::models::eligibility::check_eligibility(&ctx.db, current_user.id).await?;
    let response = views.render(
        "users/settings.html",
        json!({
            "current_user": current_user,
            "user": user,
            "email": user_email,
            "eligibility": eligibility,
            "csrf_token": csrf_token,
            "saved_theme": user.theme.as_deref().and_then(Theme::parse).unwrap_or_default().as_str(),
            "saved_accent": user.accent_color,
            "default_accent": DEFAULT_ACCENT_SWATCH,
        }),
    )?;
    Ok((jar.add(csrf_cookie), response).into_response())
}

/// What the colour input shows when no custom accent is saved (the amber
/// of the built-in themes; a `<input type="color">` always needs a value).
const DEFAULT_ACCENT_SWATCH: &str = "#d9782a";

/// `POST /settings/appearance`: saves the default theme and accent colour to
/// the account and applies them to this browser's cookies at once.
#[instrument(skip(ctx, form, jar, headers, taverna))]
pub async fn update_appearance(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Form(form): Form<AppearanceForm>,
) -> Result<Response> {
    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }
    let theme =
        Theme::parse(&form.theme).ok_or_else(|| Error::BadRequest("Unknown theme".to_string()))?;
    // The colour input always submits a value; the default swatch means
    // "no custom accent", so saving the form untouched keeps the default.
    let accent =
        if form.accent_reset.is_some() {
            None
        } else {
            match form
                .accent
                .as_deref()
                .map(str::trim)
                .filter(|a| !a.is_empty())
            {
                None => None,
                Some(value) => Some(Rgb::parse(value).ok_or_else(|| {
                    Error::BadRequest("Accent must be a #rrggbb colour".to_string())
                })?),
            }
            .filter(|rgb| rgb.to_hex() != DEFAULT_ACCENT_SWATCH)
        };

    UserModel::update_appearance(&ctx.db, current_user.id, Some(theme), accent)
        .await
        .map_err(Error::wrap)?;

    let jar = jar.add(theme_cookie(theme)).add(accent_cookie(accent));
    Ok((jar, format::redirect("/settings")).into_response())
}

/// Updates user profile settings (display name, bio, avatar, email).
#[instrument(skip(ctx, form, jar, headers, taverna, data))]
pub async fn update_settings(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    data: Data<DatabaseConnection>,
    Form(form): Form<SettingsForm>,
) -> Result<Response> {
    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }

    UserModel::update_profile(
        &ctx.db,
        current_user.id,
        form.display_name.as_deref(),
        form.bio.as_deref(),
        form.avatar_url.as_deref(),
    )
    .await
    .map_err(|e| match e {
        UserError::Validation(msg) => Error::BadRequest(msg),
        e => Error::wrap(e),
    })?;

    if let Some(email) = form
        .email
        .as_deref()
        .map(str::trim)
        .filter(|e| !e.is_empty())
    {
        let changed = UserModel::update_email(&ctx.db, current_user.id, email)
            .await
            .map_err(|e| match e {
                UserError::EmailTaken => Error::BadRequest("Email already taken".to_string()),
                UserError::Validation(msg) => Error::BadRequest(msg),
                e => Error::wrap(e),
            })?;
        if changed {
            send_new_verification(&ctx, &current_user, email, &taverna.base_url).await;
        }
    }

    // Re-fetch persisted user for federation payload
    let persisted_user = UserModel::find_by_id(&ctx.db, current_user.id)
        .await?
        .ok_or_else(|| Error::NotFound)?;

    if taverna.federation.enabled {
        let followers_url = persisted_user.followers_url.clone();
        let person = DbUser::new(persisted_user)
            .map_err(Error::wrap)?
            .into_json(&data)
            .await
            .map_err(federation::internal)?;
        let mut person = serde_json::to_value(person)?;
        person["to"] = json!([outbound::AP_PUBLIC]);
        person["cc"] = json!([followers_url]);
        let update = outbound::wrap("Update", &current_user.ap_id, person);
        outbound::send_to_user_followers(&ctx.db, &update, current_user.id, &current_user.ap_id)
            .await;
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
    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }

    if form.new_password != form.new_password_confirm {
        return Err(Error::BadRequest("New passwords do not match".to_string()));
    }

    UserModel::change_password(
        &ctx.db,
        current_user.id,
        &form.current_password,
        &form.new_password,
    )
    .await
    .map_err(|e| match e {
        UserError::Validation(msg) => Error::BadRequest(msg),
        e => Error::wrap(e),
    })?;

    Ok(format::redirect("/settings").into_response())
}

/// Shows the form to create a new profile post.
#[instrument(skip(views, ctx, current_user, jar))]
pub async fn profile_new_post(
    views: Views,
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Path(username): Path<String>,
) -> Result<Response> {
    let _user = UserModel::find_by_username(&ctx.db, &username)
        .await?
        .ok_or_else(|| Error::NotFound)?;

    if current_user.username != username {
        return Err(forbidden("Not authorized"));
    }

    let (csrf_token, csrf_cookie) = csrf::issue();

    let response = views.render(
        "users/new_post.html",
        json!({
            "current_user": current_user,
            "csrf_token": csrf_token,
            "username": username,
        }),
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
        return Err(forbidden("Not authorized"));
    }

    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
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
    .map_err(Error::wrap)?;

    if taverna.federation.enabled {
        let note = outbound::post_note(
            &post,
            &current_user.ap_id,
            None,
            &current_user.followers_url,
        );
        let create = outbound::wrap("Create", &current_user.ap_id, note);
        outbound::send_to_user_followers(&ctx.db, &create, current_user.id, &current_user.ap_id)
            .await;
    }

    Ok(format::redirect(&format!("/u/{}", username)).into_response())
}

/// Issues and mails a verification link after an email change. Failures are
/// logged: the user can resend from the settings page.
async fn send_new_verification(
    ctx: &AppContext,
    user: &crate::models::_entities::users::Model,
    email: &str,
    base_url: &str,
) {
    let token = match crate::models::email_verifications::issue(&ctx.db, user.id).await {
        Ok(token) => token,
        Err(e) => {
            tracing::warn!(user_id = %user.id, error = %e, "failed to store verification token");
            return;
        }
    };
    if let Err(e) = crate::mailers::verification::send_verification_email(
        ctx,
        email,
        &user.username,
        &token,
        base_url,
    )
    .await
    {
        tracing::warn!(user_id = %user.id, error = %e, "failed to send verification email");
    }
}
