use axum::{Extension, Form};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use loco_rs::prelude::*;
use serde::Deserialize;
use serde_json::json;
use tracing::instrument;

use crate::initializers::first_run;
use crate::initializers::theme_middleware::CurrentTheme;
use crate::models::_entities::{instance_settings, users};
use crate::models::users::{UserError, UserModel};
use crate::views::helpers;

#[derive(Debug, Deserialize)]
pub struct SetupForm {
    pub instance_name: String,
    pub username: String,
    pub email: String,
    pub password: String,
    pub password_confirm: String,
    #[serde(default)]
    pub open_registrations: Option<String>,
}

#[instrument(skip(v, ctx, theme, jar))]
pub async fn wizard_form(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    jar: CookieJar,
    Extension(theme): Extension<CurrentTheme>,
) -> Result<Response> {
    let is_first = first_run::is_first_run(&ctx.db)
        .await
        .map_err(|_| Error::InternalServerError)?;
    if !is_first {
        return Err(Error::CustomError(
            axum::http::StatusCode::FORBIDDEN,
            loco_rs::controller::ErrorDetail::with_reason("Setup already completed"),
        ));
    }
    let csrf_token = uuid::Uuid::new_v4().to_string();
    let csrf_cookie = Cookie::build(("csrf_token", csrf_token.clone()))
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(cfg!(not(debug_assertions)))
        .path("/")
        .build();

    let response = helpers::render(
        &v,
        "setup/wizard.html",
        json!({
            "csrf_token": csrf_token,
            "theme": theme.0,
        }),
    )?;
    Ok((jar.add(csrf_cookie), response).into_response())
}

#[instrument(skip(ctx, _jar, form))]
pub async fn wizard_submit(
    State(ctx): State<AppContext>,
    _jar: CookieJar,
    Form(form): Form<SetupForm>,
) -> Result<Response> {
    let is_first = first_run::is_first_run(&ctx.db)
        .await
        .map_err(|_| Error::InternalServerError)?;
    if !is_first {
        return Err(Error::CustomError(
            axum::http::StatusCode::FORBIDDEN,
            loco_rs::controller::ErrorDetail::with_reason("Setup already completed"),
        ));
    }

    let instance_name = form.instance_name.trim().to_string();
    if instance_name.is_empty() {
        return Err(Error::BadRequest("Instance name is required".to_string()));
    }

    if form.password != form.password_confirm {
        return Err(Error::BadRequest("Passwords do not match".to_string()));
    }

    if form.password.len() < 8 {
        return Err(Error::BadRequest(
            "Password must be at least 8 characters".to_string(),
        ));
    }

    let base_url =
        std::env::var("BASE_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());

    let user = UserModel::create_local(
        &ctx.db,
        &form.username,
        &form.email,
        &form.password,
        &base_url,
    )
    .await
    .map_err(|e| match e {
        UserError::InvalidUsername(msg) => Error::BadRequest(msg),
        UserError::UsernameTaken => Error::BadRequest("Username already taken".to_string()),
        UserError::EmailTaken => Error::BadRequest("Email already taken".to_string()),
        UserError::Validation(msg) => Error::BadRequest(msg),
        _ => Error::InternalServerError,
    })?;

    let user_id = user.id;
    let mut user_active: users::ActiveModel = users::Entity::find_by_id(user_id)
        .one(&ctx.db)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::InternalServerError)?
        .into();
    user_active.is_admin = sea_orm::Set(true);
    user_active
        .update(&ctx.db)
        .await
        .map_err(|_| Error::InternalServerError)?;

    let settings = instance_settings::Entity::find()
        .one(&ctx.db)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or_else(|| Error::InternalServerError)?;

    let mut settings_active: instance_settings::ActiveModel = settings.into();
    settings_active.instance_name = sea_orm::Set(instance_name);
    settings_active.open_registrations = sea_orm::Set(form.open_registrations.is_some());
    settings_active.updated_at = sea_orm::Set(chrono::Utc::now().into());
    settings_active
        .update(&ctx.db)
        .await
        .map_err(|_| Error::InternalServerError)?;

    first_run::bust_cache();

    Ok(format::redirect("/").into_response())
}
