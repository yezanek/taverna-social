use crate::controllers::errors::invalid_csrf;
use axum::http::HeaderMap;
use axum::{Extension, Form};
use axum_extra::extract::cookie::CookieJar;
use loco_rs::prelude::*;
use sea_orm::sea_query::Expr;
use serde::Deserialize;
use serde_json::json;
use tracing::instrument;

use crate::common::config::TavernaConfig;
use crate::controllers::csrf;
use crate::initializers::first_run::{self, FirstRunState};
use crate::models::_entities::{instance_settings, users};
use crate::models::users::{UserError, UserModel};
use crate::views::Views;

#[derive(Debug, Deserialize)]
pub struct SetupForm {
    pub instance_name: String,
    pub username: String,
    pub email: String,
    pub password: String,
    pub password_confirm: String,
    #[serde(default)]
    pub open_registrations: Option<String>,
    #[serde(default)]
    pub csrf_token: Option<String>,
}

#[instrument(skip(views, ctx, jar))]
pub async fn wizard_form(
    views: Views,
    State(ctx): State<AppContext>,
    jar: CookieJar,
) -> Result<Response> {
    let is_first = first_run::is_first_run(&ctx.db).await?;
    if !is_first {
        return Err(Error::CustomError(
            axum::http::StatusCode::FORBIDDEN,
            loco_rs::controller::ErrorDetail::with_reason("Setup already completed"),
        ));
    }
    let (csrf_token, csrf_cookie) = csrf::issue();

    let response = views.render(
        "setup/wizard.html",
        json!({
            "csrf_token": csrf_token,
            "hide_nav": true,
        }),
    )?;
    Ok((jar.add(csrf_cookie), response).into_response())
}

#[instrument(skip(ctx, jar, headers, taverna, first_run_state, form))]
pub async fn wizard_submit(
    State(ctx): State<AppContext>,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    Extension(first_run_state): Extension<FirstRunState>,
    headers: HeaderMap,
    Form(form): Form<SetupForm>,
) -> Result<Response> {
    let is_first = first_run::is_first_run(&ctx.db).await?;
    if !is_first {
        return Err(Error::CustomError(
            axum::http::StatusCode::FORBIDDEN,
            loco_rs::controller::ErrorDetail::with_reason("Setup already completed"),
        ));
    }

    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(invalid_csrf());
    }

    let instance_name = form.instance_name.trim().to_string();
    if instance_name.is_empty() {
        return Err(Error::BadRequest("Instance name is required".to_string()));
    }

    if form.password != form.password_confirm {
        return Err(Error::BadRequest("Passwords do not match".to_string()));
    }

    if form.password.len() < crate::models::users::MIN_PASSWORD_LEN {
        return Err(Error::BadRequest(
            "Password must be at least 8 characters".to_string(),
        ));
    }

    let user = UserModel::create_local(
        &ctx.db,
        &form.username,
        &form.email,
        &form.password,
        &taverna.base_url,
    )
    .await
    .map_err(|e| match e {
        UserError::InvalidUsername(msg) => Error::BadRequest(msg),
        UserError::UsernameTaken => Error::BadRequest("Username already taken".to_string()),
        UserError::EmailTaken => Error::BadRequest("Email already taken".to_string()),
        UserError::Validation(msg) => Error::BadRequest(msg),
        e => Error::wrap(e),
    })?;

    users::Entity::update_many()
        .col_expr(users::Column::IsAdmin, Expr::value(true))
        .filter(users::Column::Id.eq(user.id))
        .exec(&ctx.db)
        .await?;

    let settings = instance_settings::Entity::find()
        .one(&ctx.db)
        .await?
        .ok_or_else(|| Error::InternalServerError)?;

    let mut settings_active: instance_settings::ActiveModel = settings.into();
    settings_active.instance_name = sea_orm::Set(instance_name);
    settings_active.open_registrations = sea_orm::Set(form.open_registrations.is_some());
    settings_active.updated_at = sea_orm::Set(chrono::Utc::now().into());
    settings_active.update(&ctx.db).await?;

    first_run_state.mark_complete();

    Ok(format::redirect("/").into_response())
}
