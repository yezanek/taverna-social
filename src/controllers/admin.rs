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

#[derive(Debug, Deserialize)]
pub struct AdminForm {
    #[serde(default)]
    pub csrf_token: Option<String>,
}

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

    let (users, _) = UserModel::list_all(&ctx.db, 0, 50)
        .await
        .map_err(|_| Error::InternalServerError)?;

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

    UserModel::set_banned(&ctx.db, user.id, true)
        .await
        .map_err(|_| Error::InternalServerError)?;

    Ok(format::redirect("/admin").into_response())
}

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

    crate::models::communities::CommunityModel::soft_delete(&ctx.db, community.id)
        .await
        .map_err(|_| Error::InternalServerError)?;

    Ok(format::redirect("/admin").into_response())
}
