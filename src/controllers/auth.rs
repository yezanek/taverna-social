use axum::http::StatusCode;
use axum::{Extension, Form};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use loco_rs::prelude::*;
use serde::Deserialize;
use serde_json::json;
use tracing::instrument;

use crate::common::config::TavernaConfig;
use crate::common::jwt::{create_access_token, create_refresh_token};
use crate::initializers::theme_middleware::CurrentTheme;
use crate::models::users::{UserError, UserModel};
use crate::views::helpers;

#[derive(Debug, Deserialize)]
pub struct RegisterForm {
    pub username: String,
    pub email: String,
    pub password: String,
    pub password_confirm: String,
    #[serde(default)]
    pub csrf_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LoginForm {
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub csrf_token: Option<String>,
}

#[instrument(skip(v, jar))]
pub async fn register_form(
    ViewEngine(v): ViewEngine<TeraView>,
    jar: CookieJar,
    Extension(theme): Extension<CurrentTheme>,
) -> Result<Response> {
    let csrf_token = uuid::Uuid::new_v4().to_string();
    let csrf_cookie = Cookie::build(("csrf_token", csrf_token.clone()))
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(cfg!(not(debug_assertions)))
        .path("/")
        .build();
    let response = helpers::render_with_theme(
        &v,
        "auth/register.html",
        json!({"csrf_token": csrf_token}),
        &theme,
    )?;
    Ok((jar.add(csrf_cookie), response).into_response())
}

#[instrument(skip(v, jar, ctx, form))]
pub async fn register(
    ViewEngine(v): ViewEngine<TeraView>,
    jar: CookieJar,
    State(ctx): State<AppContext>,
    Extension(taverna): Extension<TavernaConfig>,
    Extension(theme): Extension<CurrentTheme>,
    headers: axum::http::HeaderMap,
    Form(form): Form<RegisterForm>,
) -> Result<Response> {
    if !validate_csrf(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Ok(StatusCode::FORBIDDEN.into_response());
    }

    if form.password != form.password_confirm {
        return render_with_csrf(
            &v,
            jar,
            "auth/register.html",
            json!({
                "error": "Passwords do not match",
                "username": form.username,
                "email": form.email,
            }),
            &theme,
        );
    }

    if form.password.len() < 8 {
        return render_with_csrf(
            &v,
            jar,
            "auth/register.html",
            json!({
                "error": "Password must be at least 8 characters",
                "username": form.username,
                "email": form.email,
            }),
            &theme,
        );
    }

    let base_url = taverna.base_url.clone();

    match UserModel::create_local(
        &ctx.db,
        &form.username,
        &form.email,
        &form.password,
        &base_url,
    )
    .await
    {
        Ok(_) => Ok(format::redirect("/login").into_response()),
        Err(e) => {
            let error_msg = match e {
                UserError::InvalidUsername(msg) => msg,
                UserError::UsernameTaken => "Username is already taken".to_string(),
                UserError::EmailTaken => "Email is already registered".to_string(),
                UserError::Validation(msg) => msg,
                _ => "Registration failed. Please try again.".to_string(),
            };
            render_with_csrf(
                &v,
                jar,
                "auth/register.html",
                json!({
                    "error": error_msg,
                    "username": form.username,
                    "email": form.email,
                }),
                &theme,
            )
        }
    }
}

#[instrument(skip(v, jar))]
pub async fn login_form(
    ViewEngine(v): ViewEngine<TeraView>,
    jar: CookieJar,
    Extension(theme): Extension<CurrentTheme>,
) -> Result<Response> {
    let csrf_token = uuid::Uuid::new_v4().to_string();
    let csrf_cookie = Cookie::build(("csrf_token", csrf_token.clone()))
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(cfg!(not(debug_assertions)))
        .path("/")
        .build();
    let response = helpers::render_with_theme(
        &v,
        "auth/login.html",
        json!({"csrf_token": csrf_token}),
        &theme,
    )?;
    Ok((jar.add(csrf_cookie), response).into_response())
}

#[instrument(skip(v, jar, ctx, form))]
pub async fn login(
    ViewEngine(v): ViewEngine<TeraView>,
    jar: CookieJar,
    State(ctx): State<AppContext>,
    Extension(taverna): Extension<TavernaConfig>,
    Extension(theme): Extension<CurrentTheme>,
    headers: axum::http::HeaderMap,
    Form(form): Form<LoginForm>,
) -> Result<Response> {
    if !validate_csrf(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Ok(StatusCode::FORBIDDEN.into_response());
    }

    let user = UserModel::find_by_email(&ctx.db, &form.email)
        .await
        .map_err(|_| Error::InternalServerError)?;

    let user = match user {
        Some(u) if UserModel::verify_password(&u, &form.password).await => u,
        _ => {
            return render_with_csrf(
                &v,
                jar,
                "auth/login.html",
                json!({
                    "error": "Invalid credentials",
                }),
                &theme,
            );
        }
    };

    let jwt_secret = ctx
        .config
        .get_jwt_config()
        .map_err(|_| Error::InternalServerError)?;

    let access_token =
        create_access_token(&user, &jwt_secret.secret).map_err(|_| Error::InternalServerError)?;
    let refresh_token =
        create_refresh_token(&user, &jwt_secret.secret).map_err(|_| Error::InternalServerError)?;

    let secure_cookies = taverna.base_url.starts_with("https");

    let access_cookie = Cookie::build(("access_token", access_token))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path("/")
        .secure(secure_cookies)
        .max_age(time::Duration::hours(1))
        .build();

    let refresh_cookie = Cookie::build(("refresh_token", refresh_token))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path("/")
        .secure(secure_cookies)
        .max_age(time::Duration::days(7))
        .build();

    let response = format::redirect("/").into_response();
    Ok((jar.add(access_cookie).add(refresh_cookie), response).into_response())
}

#[instrument(skip(jar))]
pub async fn logout(
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
) -> Result<Response> {
    let secure_cookies = taverna.base_url.starts_with("https");

    let access_cookie = Cookie::build(("access_token", ""))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path("/")
        .secure(secure_cookies)
        .max_age(time::Duration::new(0, 0))
        .build();

    let refresh_cookie = Cookie::build(("refresh_token", ""))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path("/")
        .secure(secure_cookies)
        .max_age(time::Duration::new(0, 0))
        .build();

    let response = format::redirect("/login").into_response();
    Ok((jar.add(access_cookie).add(refresh_cookie), response).into_response())
}

pub(crate) fn validate_csrf(
    jar: &CookieJar,
    form_token: Option<&str>,
    headers: &axum::http::HeaderMap,
    taverna: &TavernaConfig,
) -> bool {
    if let Ok(base) = url::Url::parse(&taverna.base_url) {
        let origin = headers
            .get("origin")
            .or_else(|| headers.get("referer"))
            .and_then(|v| v.to_str().ok())
            .and_then(|v| url::Url::parse(v).ok());
        if let Some(ref origin_url) = origin
            && (origin_url.scheme() != base.scheme()
                || origin_url.host() != base.host()
                || origin_url.port() != base.port())
        {
            return false;
        }
    }
    let cookie_token = jar.get("csrf_token").map(|c| c.value().to_string());
    match (cookie_token, form_token) {
        (Some(ref cookie), Some(form)) => cookie == form,
        _ => false,
    }
}

#[allow(clippy::result_large_err)]
fn render_with_csrf(
    v: &TeraView,
    jar: CookieJar,
    template: &str,
    mut ctx: serde_json::Value,
    theme: &CurrentTheme,
) -> Result<Response> {
    let csrf_token = uuid::Uuid::new_v4().to_string();
    let csrf_cookie = Cookie::build(("csrf_token", csrf_token.clone()))
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(cfg!(not(debug_assertions)))
        .path("/")
        .build();
    if let Some(obj) = ctx.as_object_mut() {
        obj.insert("csrf_token".to_string(), serde_json::json!(csrf_token));
    }
    let response = helpers::render_with_theme(v, template, ctx, theme)?;
    Ok((jar.add(csrf_cookie), response).into_response())
}
