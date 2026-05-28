use axum::extract::Path;
use axum::http::HeaderMap;
use axum::{Extension, Form};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use loco_rs::prelude::*;
use serde::Deserialize;
use serde_json::json;
use tracing::instrument;

use crate::common::config::TavernaConfig;
use crate::controllers::auth::validate_csrf;
use crate::initializers::auth_middleware::AuthUser;
use crate::initializers::theme_middleware::CurrentTheme;
use crate::models::communities::CommunityModel;
use crate::models::users::UserModel;
use crate::views::helpers;

/// Generic admin action form (used for CSRF-protected POST actions).
#[derive(Debug, Deserialize)]
pub struct AdminForm {
    #[serde(default)]
    pub csrf_token: Option<String>,
}

/// Admin dashboard showing communities and users for moderation.
#[instrument(skip(v, ctx, theme, current_user, jar))]
pub async fn dashboard(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(theme): Extension<CurrentTheme>,
) -> Result<Response> {
    if !current_user.is_admin {
        return Err(Error::Unauthorized("Admin access required".to_string()));
    }

    let (communities, _) = CommunityModel::list_all(&ctx.db, 0, 50)
        .await
        .map_err(|_| Error::InternalServerError)?;

    let (all_users, _) = UserModel::list_all(&ctx.db, 0, 50)
        .await
        .map_err(|_| Error::InternalServerError)?;

    let users: Vec<serde_json::Value> = all_users
        .into_iter()
        .map(|u| {
            json!({
                "username": u.username,
                "email": u.email,
                "is_admin": u.is_admin,
                "is_banned": u.is_banned,
                "email_verified": u.email_verified,
                "standing_score": u.standing_score,
                "suspended_at": u.suspended_at,
            })
        })
        .collect();

    let csrf_token = uuid::Uuid::new_v4().to_string();
    let csrf_cookie = Cookie::build(("csrf_token", csrf_token.clone()))
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(cfg!(not(debug_assertions)))
        .path("/")
        .build();

    let response = helpers::render_with_theme(
        &v,
        "admin/dashboard.html",
        json!({
            "current_user": current_user,
            "communities": communities,
            "users": users,
            "csrf_token": csrf_token,
        }),
        &theme,
    )?;
    Ok((jar.add(csrf_cookie), response).into_response())
}

/// Bans a user account (admin only).
#[instrument(skip(ctx, jar, headers, taverna))]
pub async fn ban_user(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path(username): Path<String>,
    Form(form): Form<AdminForm>,
) -> Result<Response> {
    if !current_user.is_admin {
        return Err(Error::Unauthorized("Admin access required".to_string()));
    }

    if !validate_csrf(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
    }

    let user = UserModel::find_by_username(&ctx.db, &username)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    if user.id == current_user.id {
        return Err(Error::BadRequest("Cannot ban yourself".to_string()));
    }

    if user.is_admin {
        return Err(Error::BadRequest("Cannot ban another admin".to_string()));
    }

    UserModel::set_banned(&ctx.db, user.id, true)
        .await
        .map_err(|_| Error::InternalServerError)?;

    Ok(format::redirect("/admin").into_response())
}

/// Unbans a user account (admin only).
#[instrument(skip(ctx, jar, headers, taverna))]
pub async fn unban_user(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path(username): Path<String>,
    Form(form): Form<AdminForm>,
) -> Result<Response> {
    if !current_user.is_admin {
        return Err(Error::Unauthorized("Admin access required".to_string()));
    }

    if !validate_csrf(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
    }

    let user = UserModel::find_by_username(&ctx.db, &username)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    UserModel::set_banned(&ctx.db, user.id, false)
        .await
        .map_err(|_| Error::InternalServerError)?;

    Ok(format::redirect("/admin").into_response())
}

/// Admin eligibility settings form.
#[derive(Debug, Deserialize)]
pub struct AdminSettingsForm {
    pub community_creation_open: Option<String>,
    pub require_min_account_age: Option<String>,
    pub min_account_age_days: Option<i32>,
    pub require_email_verified: Option<String>,
    pub require_min_standing: Option<String>,
    pub min_standing_score: Option<i32>,
    pub disqualify_if_content_removed: Option<String>,
    pub disqualify_if_suspended: Option<String>,
    pub instance_name: Option<String>,
    pub open_registrations: Option<String>,
    #[serde(default)]
    pub csrf_token: Option<String>,
}

/// Admin settings page — view and update eligibility settings.
#[instrument(skip(v, ctx, theme, current_user, jar))]
pub async fn settings(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(theme): Extension<CurrentTheme>,
) -> Result<Response> {
    if !current_user.is_admin {
        return Err(Error::Unauthorized("Admin access required".to_string()));
    }

    let settings = crate::models::_entities::instance_settings::Entity::find()
        .one(&ctx.db)
        .await
        .map_err(|_| Error::InternalServerError)?
        .unwrap_or_else(|| crate::models::_entities::instance_settings::Model {
            id: true,
            community_creation_open: true,
            require_min_account_age: false,
            min_account_age_days: 30,
            require_email_verified: false,
            require_min_standing: false,
            min_standing_score: 0,
            disqualify_if_content_removed: false,
            disqualify_if_suspended: false,
            instance_name: "Taverna".to_string(),
            open_registrations: true,
            updated_at: chrono::DateTime::default(),
        });

    let csrf_token = uuid::Uuid::new_v4().to_string();
    let csrf_cookie = Cookie::build(("csrf_token", csrf_token.clone()))
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(cfg!(not(debug_assertions)))
        .path("/")
        .build();

    let response = helpers::render_with_theme(
        &v,
        "admin/settings.html",
        json!({
            "current_user": current_user,
            "settings": settings,
            "csrf_token": csrf_token,
        }),
        &theme,
    )?;
    Ok((jar.add(csrf_cookie), response).into_response())
}

/// Updates instance settings (admin only).
#[instrument(skip(ctx, jar, headers, taverna))]
pub async fn update_settings(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Form(form): Form<AdminSettingsForm>,
) -> Result<Response> {
    if !current_user.is_admin {
        return Err(Error::Unauthorized("Admin access required".to_string()));
    }

    if !validate_csrf(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
    }

    let mut settings: crate::models::_entities::instance_settings::ActiveModel =
        crate::models::_entities::instance_settings::Entity::find()
            .one(&ctx.db)
            .await
            .map_err(|_| Error::InternalServerError)?
            .ok_or_else(|| Error::NotFound)?
            .into();

    settings.community_creation_open = sea_orm::Set(form.community_creation_open.is_some());
    settings.require_min_account_age = sea_orm::Set(form.require_min_account_age.is_some());
    settings.min_account_age_days = sea_orm::Set(form.min_account_age_days.unwrap_or(30));
    settings.require_email_verified = sea_orm::Set(form.require_email_verified.is_some());
    settings.require_min_standing = sea_orm::Set(form.require_min_standing.is_some());
    settings.min_standing_score = sea_orm::Set(form.min_standing_score.unwrap_or(0));
    settings.disqualify_if_content_removed =
        sea_orm::Set(form.disqualify_if_content_removed.is_some());
    settings.disqualify_if_suspended = sea_orm::Set(form.disqualify_if_suspended.is_some());
    if let Some(ref name) = form.instance_name {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            settings.instance_name = sea_orm::Set(trimmed.to_string());
        }
    }
    settings.open_registrations = sea_orm::Set(form.open_registrations.is_some());
    settings.updated_at = sea_orm::Set(chrono::Utc::now().into());

    settings
        .update(&ctx.db)
        .await
        .map_err(|_| Error::InternalServerError)?;

    Ok(format::redirect("/admin/settings").into_response())
}

/// Suspends a user account (admin only).
#[instrument(skip(ctx, jar, headers, taverna))]
pub async fn suspend_user(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path(username): Path<String>,
    Form(form): Form<AdminForm>,
) -> Result<Response> {
    if !current_user.is_admin {
        return Err(Error::Unauthorized("Admin access required".to_string()));
    }

    if !validate_csrf(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
    }

    let user = UserModel::find_by_username(&ctx.db, &username)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    if user.id == current_user.id {
        return Err(Error::BadRequest("Cannot suspend yourself".to_string()));
    }

    if user.is_admin {
        return Err(Error::BadRequest(
            "Cannot suspend another admin".to_string(),
        ));
    }

    UserModel::set_suspended(&ctx.db, user.id, true)
        .await
        .map_err(|_| Error::InternalServerError)?;

    Ok(format::redirect("/admin").into_response())
}

/// Unsuspends a user account (admin only).
#[instrument(skip(ctx, jar, headers, taverna))]
pub async fn unsuspend_user(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path(username): Path<String>,
    Form(form): Form<AdminForm>,
) -> Result<Response> {
    if !current_user.is_admin {
        return Err(Error::Unauthorized("Admin access required".to_string()));
    }

    if !validate_csrf(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
    }

    let user = UserModel::find_by_username(&ctx.db, &username)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    UserModel::set_suspended(&ctx.db, user.id, false)
        .await
        .map_err(|_| Error::InternalServerError)?;

    Ok(format::redirect("/admin").into_response())
}

/// Forces email verification for a user (admin only, no token required).
#[instrument(skip(ctx, jar, headers, taverna))]
pub async fn force_verify_user(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path(username): Path<String>,
    Form(form): Form<AdminForm>,
) -> Result<Response> {
    if !current_user.is_admin {
        return Err(Error::Unauthorized("Admin access required".to_string()));
    }

    if !validate_csrf(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
    }

    let user = UserModel::find_by_username(&ctx.db, &username)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    UserModel::force_verify_email(&ctx.db, user.id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    Ok(format::redirect("/admin").into_response())
}

/// Soft-deletes a community (admin only).
#[instrument(skip(ctx, jar, headers, taverna))]
pub async fn delete_community(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Form(form): Form<AdminForm>,
) -> Result<Response> {
    if !current_user.is_admin {
        return Err(Error::Unauthorized("Admin access required".to_string()));
    }

    if !validate_csrf(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
    }

    let community = CommunityModel::find_by_slug(&ctx.db, &slug)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::NotFound)?;

    CommunityModel::soft_delete(&ctx.db, community.id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    Ok(format::redirect("/admin").into_response())
}
