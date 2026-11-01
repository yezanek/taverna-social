use crate::controllers::errors::{forbidden, invalid_csrf};
use axum::extract::{Path, Query};
use axum::http::HeaderMap;
use axum::{Extension, Form};
use axum_extra::extract::cookie::CookieJar;
use loco_rs::prelude::*;
use sea_orm::PaginatorTrait;
use serde::Deserialize;
use serde_json::json;
use tracing::instrument;

use crate::common::config::TavernaConfig;
use crate::controllers::csrf;
use crate::initializers::auth_middleware::AuthUser;
use crate::models::_entities::{community_members, posts};
use crate::models::communities::CommunityModel;
use crate::models::feed::Cursor;
use crate::models::users::UserModel;
use crate::views::Views;

/// Generic admin action form (used for CSRF-protected POST actions).
#[derive(Debug, Deserialize)]
pub struct AdminForm {
    #[serde(default)]
    pub csrf_token: Option<String>,
}

const ADMIN_PER_PAGE: u64 = 50;

/// Independent cursors for the two lists on the admin dashboard.
#[derive(Debug, Default, Deserialize)]
pub struct AdminListQuery {
    pub communities_cursor: Option<String>,
    pub users_cursor: Option<String>,
}

/// Admin dashboard showing communities and users for moderation.
#[instrument(skip(views, ctx, current_user, jar))]
pub async fn dashboard(
    views: Views,
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Query(page): Query<AdminListQuery>,
) -> Result<Response> {
    if !current_user.is_admin {
        return Err(forbidden("Admin access required"));
    }

    let communities_cursor = Cursor::from_param(page.communities_cursor.as_deref());
    let users_cursor = Cursor::from_param(page.users_cursor.as_deref());

    let community_page =
        CommunityModel::list_all(&ctx.db, communities_cursor.as_ref(), ADMIN_PER_PAGE).await?;

    let mut communities: Vec<serde_json::Value> = Vec::new();
    for c in community_page.items {
        let member_count = community_members::Entity::find()
            .filter(community_members::Column::CommunityId.eq(c.id))
            .count(&ctx.db)
            .await?;
        let post_count = posts::Entity::find()
            .filter(posts::Column::CommunityId.eq(c.id))
            .filter(posts::Column::IsDeleted.eq(false))
            .filter(posts::Column::IsRemoved.eq(false))
            .count(&ctx.db)
            .await?;
        communities.push(json!({
            "slug": c.slug,
            "name": c.name,
            "is_local": c.is_local,
            "member_count": member_count,
            "post_count": post_count,
        }));
    }

    let user_page = UserModel::list_all(&ctx.db, users_cursor.as_ref(), ADMIN_PER_PAGE).await?;

    let users: Vec<serde_json::Value> = user_page
        .items
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

    let (csrf_token, csrf_cookie) = csrf::issue();

    let response = views.render(
        "admin/dashboard.html",
        json!({
            "current_user": current_user,
            "communities": communities,
            "communities_next_cursor": community_page.next_cursor,
            "users": users,
            "users_next_cursor": user_page.next_cursor,
            "csrf_token": csrf_token,
        }),
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
        return Err(forbidden("Admin access required"));
    }

    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }

    let user = UserModel::find_by_username(&ctx.db, &username)
        .await?
        .ok_or_else(|| Error::NotFound)?;

    if user.id == current_user.id {
        return Err(Error::BadRequest("Cannot ban yourself".to_string()));
    }

    if user.is_admin {
        return Err(Error::BadRequest("Cannot ban another admin".to_string()));
    }

    UserModel::set_banned(&ctx.db, user.id, true)
        .await
        .map_err(Error::wrap)?;

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
        return Err(forbidden("Admin access required"));
    }

    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }

    let user = UserModel::find_by_username(&ctx.db, &username)
        .await?
        .ok_or_else(|| Error::NotFound)?;

    UserModel::set_banned(&ctx.db, user.id, false)
        .await
        .map_err(Error::wrap)?;

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
#[instrument(skip(views, ctx, current_user, jar))]
pub async fn settings(
    views: Views,
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
) -> Result<Response> {
    if !current_user.is_admin {
        return Err(forbidden("Admin access required"));
    }

    let settings = crate::models::instance_settings::load(&ctx.db).await?;

    let (csrf_token, csrf_cookie) = csrf::issue();

    let response = views.render(
        "admin/settings.html",
        json!({
            "current_user": current_user,
            "settings": settings,
            "csrf_token": csrf_token,
        }),
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
        return Err(forbidden("Admin access required"));
    }

    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }

    let mut settings: crate::models::_entities::instance_settings::ActiveModel =
        crate::models::_entities::instance_settings::Entity::find()
            .one(&ctx.db)
            .await?
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

    settings.update(&ctx.db).await?;

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
        return Err(forbidden("Admin access required"));
    }

    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }

    let user = UserModel::find_by_username(&ctx.db, &username)
        .await?
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
        .map_err(Error::wrap)?;

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
        return Err(forbidden("Admin access required"));
    }

    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }

    let user = UserModel::find_by_username(&ctx.db, &username)
        .await?
        .ok_or_else(|| Error::NotFound)?;

    UserModel::set_suspended(&ctx.db, user.id, false)
        .await
        .map_err(Error::wrap)?;

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
        return Err(forbidden("Admin access required"));
    }

    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }

    let user = UserModel::find_by_username(&ctx.db, &username)
        .await?
        .ok_or_else(|| Error::NotFound)?;

    UserModel::force_verify_email(&ctx.db, user.id)
        .await
        .map_err(Error::wrap)?;

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
        return Err(forbidden("Admin access required"));
    }

    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }

    let community = CommunityModel::find_by_slug(&ctx.db, &slug)
        .await?
        .ok_or_else(|| Error::NotFound)?;

    CommunityModel::soft_delete(&ctx.db, community.id).await?;

    Ok(format::redirect("/admin").into_response())
}
