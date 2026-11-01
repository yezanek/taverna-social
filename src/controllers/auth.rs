use axum::http::StatusCode;
use axum::{Extension, Form};
use axum_extra::extract::cookie::CookieJar;
use loco_rs::prelude::*;
use serde::Deserialize;
use serde_json::json;
use tracing::instrument;

use crate::common::config::TavernaConfig;
use crate::common::jwt::{
    ACCESS_TOKEN_COOKIE, REFRESH_TOKEN_COOKIE, TokenKind, create_access_token, create_refresh_token,
};
use crate::controllers::csrf;
use crate::initializers::auth_middleware::{access_cookie, session_cookie};
use crate::initializers::theme_middleware::CurrentTheme;
use crate::models::email_verifications;
use crate::models::users::{UserError, UserModel};
use crate::views::helpers;

/// Form data for user registration.
#[derive(Debug, Deserialize)]
pub struct RegisterForm {
    pub username: String,
    pub email: String,
    pub password: String,
    pub password_confirm: String,
    #[serde(default)]
    pub csrf_token: Option<String>,
}

/// Form data for user login.
#[derive(Debug, Deserialize)]
pub struct LoginForm {
    pub identifier: String,
    pub password: String,
    #[serde(default)]
    pub csrf_token: Option<String>,
}

/// Renders the registration page with a CSRF token.
#[instrument(skip(v, jar))]
pub async fn register_form(
    ViewEngine(v): ViewEngine<TeraView>,
    jar: CookieJar,
    Extension(theme): Extension<CurrentTheme>,
) -> Result<Response> {
    let (csrf_token, csrf_cookie) = csrf::issue();
    let response = helpers::render_with_theme(
        &v,
        "auth/register.html",
        json!({"csrf_token": csrf_token}),
        &theme,
    )?;
    Ok((jar.add(csrf_cookie), response).into_response())
}

/// Handles user registration form submission.
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
    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Ok(StatusCode::FORBIDDEN.into_response());
    }

    let settings = crate::models::instance_settings::load(&ctx.db).await?;
    if !settings.open_registrations {
        return render_with_csrf(
            &v,
            jar,
            "auth/register.html",
            json!({ "error": "Registrations are closed on this instance" }),
            &theme,
        );
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

    if form.password.len() < crate::models::users::MIN_PASSWORD_LEN {
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
        Ok(user) => {
            // Registration succeeds even when the mail cannot go out: the
            // user can resend from their settings page.
            match email_verifications::issue(&ctx.db, user.id).await {
                Ok(token) => {
                    if let Err(e) = crate::mailers::verification::send_verification_email(
                        &ctx,
                        &form.email,
                        &form.username,
                        &token,
                        &base_url,
                    )
                    .await
                    {
                        tracing::warn!(user_id = %user.id, error = %e, "failed to send verification email");
                    }
                }
                Err(e) => {
                    tracing::warn!(user_id = %user.id, error = %e, "failed to store verification token");
                }
            }

            Ok(format::redirect("/login").into_response())
        }
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

/// Renders the login page with a CSRF token.
#[instrument(skip(v, jar))]
pub async fn login_form(
    ViewEngine(v): ViewEngine<TeraView>,
    jar: CookieJar,
    Extension(theme): Extension<CurrentTheme>,
) -> Result<Response> {
    let (csrf_token, csrf_cookie) = csrf::issue();
    let response = helpers::render_with_theme(
        &v,
        "auth/login.html",
        json!({"csrf_token": csrf_token}),
        &theme,
    )?;
    Ok((jar.add(csrf_cookie), response).into_response())
}

/// Handles login form submission and issues JWT cookies on success.
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
    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Ok(StatusCode::FORBIDDEN.into_response());
    }

    let user = if form.identifier.contains('@') {
        UserModel::find_by_email(&ctx.db, &form.identifier).await?
    } else {
        UserModel::find_by_username(&ctx.db, &form.identifier).await?
    };

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

    let jwt_secret = ctx.config.get_jwt_config()?;

    let access_token = create_access_token(&user, &jwt_secret.secret).map_err(Error::wrap)?;
    let refresh_token = create_refresh_token(&user, &jwt_secret.secret).map_err(Error::wrap)?;

    let secure_cookies = taverna.base_url.starts_with("https");
    let access_cookie = access_cookie(access_token, secure_cookies);
    let refresh_cookie = session_cookie(
        REFRESH_TOKEN_COOKIE,
        refresh_token,
        time::Duration::seconds(TokenKind::Refresh.lifetime().num_seconds()),
        secure_cookies,
    );

    let response = format::redirect("/").into_response();
    Ok((jar.add(access_cookie).add(refresh_cookie), response).into_response())
}

/// Clears the auth cookies and redirects to login.
#[instrument(skip(jar))]
pub async fn logout(
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
) -> Result<Response> {
    let secure_cookies = taverna.base_url.starts_with("https");
    let [access_cookie, refresh_cookie] = [ACCESS_TOKEN_COOKIE, REFRESH_TOKEN_COOKIE]
        .map(|name| session_cookie(name, String::new(), time::Duration::ZERO, secure_cookies));

    let response = format::redirect("/login").into_response();
    Ok((jar.add(access_cookie).add(refresh_cookie), response).into_response())
}

/// Renders a template with a fresh CSRF cookie, carrying over existing cookie jar.
fn render_with_csrf(
    v: &TeraView,
    jar: CookieJar,
    template: &str,
    mut ctx: serde_json::Value,
    theme: &CurrentTheme,
) -> Result<Response> {
    let (csrf_token, csrf_cookie) = csrf::issue();
    if let Some(obj) = ctx.as_object_mut() {
        obj.insert("csrf_token".to_string(), serde_json::json!(csrf_token));
    }
    let response = helpers::render_with_theme(v, template, ctx, theme)?;
    Ok((jar.add(csrf_cookie), response).into_response())
}
