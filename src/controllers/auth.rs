use axum::Form;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use loco_rs::prelude::*;
use serde::Deserialize;
use serde_json::json;
use tracing::instrument;

use crate::common::jwt::{create_access_token, create_refresh_token};
use crate::models::users::{UserError, UserModel};
use crate::views::helpers;

#[derive(Debug, Deserialize)]
pub struct RegisterForm {
    pub username: String,
    pub email: String,
    pub password: String,
    pub password_confirm: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginForm {
    pub email: String,
    pub password: String,
}

#[instrument(skip(v, jar))]
pub async fn register_form(
    ViewEngine(v): ViewEngine<TeraView>,
    jar: CookieJar,
) -> Result<Response> {
    let theme = helpers::get_theme(&jar);
    helpers::render(&v, "auth/register.html", json!({"theme": theme}))
}

#[instrument(skip(v, jar, ctx, form))]
pub async fn register(
    ViewEngine(v): ViewEngine<TeraView>,
    jar: CookieJar,
    State(ctx): State<AppContext>,
    Form(form): Form<RegisterForm>,
) -> Result<Response> {
    let theme = helpers::get_theme(&jar);

    if form.password != form.password_confirm {
        return helpers::render(
            &v,
            "auth/register.html",
            json!({
                "theme": theme,
                "error": "Passwords do not match",
                "username": form.username,
                "email": form.email,
            }),
        );
    }

    if form.password.len() < 8 {
        return helpers::render(
            &v,
            "auth/register.html",
            json!({
                "theme": theme,
                "error": "Password must be at least 8 characters",
                "username": form.username,
                "email": form.email,
            }),
        );
    }

    let base_url = "http://localhost:3000".to_string();

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
            helpers::render(
                &v,
                "auth/register.html",
                json!({
                    "theme": theme,
                    "error": error_msg,
                    "username": form.username,
                    "email": form.email,
                }),
            )
        }
    }
}

#[instrument(skip(v, jar))]
pub async fn login_form(ViewEngine(v): ViewEngine<TeraView>, jar: CookieJar) -> Result<Response> {
    let theme = helpers::get_theme(&jar);
    helpers::render(&v, "auth/login.html", json!({"theme": theme}))
}

#[instrument(skip(v, jar, ctx, form))]
pub async fn login(
    ViewEngine(v): ViewEngine<TeraView>,
    jar: CookieJar,
    State(ctx): State<AppContext>,
    Form(form): Form<LoginForm>,
) -> Result<Response> {
    let theme = helpers::get_theme(&jar);

    let user = UserModel::find_by_email(&ctx.db, &form.email)
        .await
        .map_err(|_| Error::InternalServerError)?;

    let user = match user {
        Some(u) if UserModel::verify_password(&u, &form.password) => u,
        _ => {
            return helpers::render(
                &v,
                "auth/login.html",
                json!({
                    "theme": theme,
                    "error": "Invalid credentials",
                }),
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

    let access_cookie = Cookie::build(("access_token", access_token))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(time::Duration::hours(1))
        .build();

    let refresh_cookie = Cookie::build(("refresh_token", refresh_token))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(time::Duration::days(7))
        .build();

    let response = format::redirect("/").into_response();
    Ok((jar.add(access_cookie).add(refresh_cookie), response).into_response())
}

#[instrument(skip(jar))]
pub async fn logout(jar: CookieJar) -> Result<Response> {
    let access_cookie = Cookie::build(("access_token", ""))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(time::Duration::new(0, 0))
        .build();

    let refresh_cookie = Cookie::build(("refresh_token", ""))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(time::Duration::new(0, 0))
        .build();

    let response = format::redirect("/login").into_response();
    Ok((jar.add(access_cookie).add(refresh_cookie), response).into_response())
}
