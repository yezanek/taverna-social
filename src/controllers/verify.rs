use axum::Extension;
use axum::http::HeaderMap;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use loco_rs::prelude::*;
use serde::Deserialize;
use serde_json::json;
use tracing::instrument;

use crate::common::config::TavernaConfig;
use crate::initializers::auth_middleware::AuthUser;
use crate::initializers::theme_middleware::CurrentTheme;
use crate::mailers::verification;
use crate::models::_entities::{email_verifications, users};
use crate::views::helpers;

#[derive(Debug, Deserialize)]
pub struct VerifyQuery {
    pub token: String,
}

#[derive(Debug, Deserialize)]
pub struct ResendForm {
    #[serde(default)]
    pub csrf_token: Option<String>,
}

#[instrument(skip(v, ctx, theme, jar))]
pub async fn verify_email(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    jar: CookieJar,
    Extension(theme): Extension<CurrentTheme>,
    axum::extract::Query(query): axum::extract::Query<VerifyQuery>,
) -> Result<Response> {
    let verification = email_verifications::Entity::find()
        .filter(email_verifications::Column::Token.eq(&query.token))
        .one(&ctx.db)
        .await
        .map_err(|_| Error::InternalServerError)?;

    match verification {
        Some(ver) => {
            if ver.verified_at.is_some() {
                return Ok(format::redirect("/").into_response());
            }

            if ver.expires_at < chrono::Utc::now() {
                let response = helpers::render_with_theme(
                    &v,
                    "auth/register.html",
                    json!({"error": "Verification token has expired. Request a new one."}),
                    &theme,
                )?;
                return Ok((jar, response).into_response());
            }

            let uid = ver.user_id;
            let mut active: email_verifications::ActiveModel = ver.into();
            active.verified_at = Set(Some(chrono::Utc::now().into()));
            active
                .update(&ctx.db)
                .await
                .map_err(|_| Error::InternalServerError)?;

            let mut user_active: users::ActiveModel = users::Entity::find_by_id(uid)
                .one(&ctx.db)
                .await
                .map_err(|_| Error::InternalServerError)?
                .ok_or_else(|| Error::NotFound)?
                .into();
            user_active.email_verified = Set(true);
            user_active
                .update(&ctx.db)
                .await
                .map_err(|_| Error::InternalServerError)?;

            Ok(format::redirect("/").into_response())
        }
        None => {
            let response = helpers::render_with_theme(
                &v,
                "auth/register.html",
                json!({"error": "Invalid verification token."}),
                &theme,
            )?;
            Ok((jar, response).into_response())
        }
    }
}

#[instrument(skip(ctx, jar, _headers, taverna))]
pub async fn resend_verification(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    _headers: HeaderMap,
) -> Result<Response> {
    if current_user.email_verified {
        return Err(Error::BadRequest("Email already verified".to_string()));
    }

    let token = uuid::Uuid::new_v4().to_string();

    let email = current_user
        .email
        .as_deref()
        .ok_or_else(|| Error::BadRequest("No email on file".to_string()))?;

    email_verifications::ActiveModel {
        id: Set(uuid::Uuid::new_v4()),
        user_id: Set(current_user.id),
        token: Set(token.clone()),
        expires_at: Set((chrono::Utc::now() + chrono::Duration::hours(24)).into()),
        verified_at: Set(None),
        created_at: Set(chrono::Utc::now().into()),
    }
    .insert(&ctx.db)
    .await
    .map_err(|_| Error::InternalServerError)?;

    verification::send_verification_email(
        &ctx,
        email,
        &current_user.username,
        &token,
        &taverna.base_url,
    )
    .await
    .map_err(|_| Error::InternalServerError)?;

    let csrf_token = uuid::Uuid::new_v4().to_string();
    let csrf_cookie = Cookie::build(("csrf_token", csrf_token.clone()))
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(cfg!(not(debug_assertions)))
        .path("/")
        .build();

    let response = format::redirect("/settings").into_response();
    Ok((jar.add(csrf_cookie), response).into_response())
}
