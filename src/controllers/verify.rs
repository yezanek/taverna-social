use axum::extract::Query;
use axum::http::HeaderMap;
use axum::{Extension, Form};
use axum_extra::extract::cookie::CookieJar;
use loco_rs::prelude::*;
use serde::Deserialize;
use serde_json::json;
use tracing::instrument;

use crate::common::config::TavernaConfig;
use crate::controllers::csrf::{self, CsrfForm};
use crate::initializers::auth_middleware::AuthUser;
use crate::initializers::theme_middleware::CurrentTheme;
use crate::mailers::verification;
use crate::models::email_verifications::{self, VerifyOutcome};
use crate::views::helpers;

#[derive(Debug, Deserialize)]
pub struct VerifyQuery {
    pub token: String,
}

/// `GET /verify-email?token=...` (SPEC §3.6).
#[instrument(skip(v, ctx, theme, query))]
pub async fn verify_email(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    Extension(theme): Extension<CurrentTheme>,
    Query(query): Query<VerifyQuery>,
) -> Result<Response> {
    let outcome = email_verifications::verify(&ctx.db, &query.token)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "email verification failed");
            Error::InternalServerError
        })?;

    let error = match outcome {
        VerifyOutcome::Verified | VerifyOutcome::AlreadyVerified => {
            return Ok(format::redirect("/").into_response());
        }
        VerifyOutcome::Expired => "Verification token has expired. Request a new one.",
        VerifyOutcome::Unknown => "Invalid verification token.",
    };
    helpers::render_with_theme(&v, "auth/register.html", json!({ "error": error }), &theme)
}

/// `POST /resend-verification`: issues a new token and mails it.
#[instrument(skip(ctx, jar, headers, taverna, form))]
pub async fn resend_verification(
    State(ctx): State<AppContext>,
    AuthUser(current_user): AuthUser,
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Form(form): Form<CsrfForm>,
) -> Result<Response> {
    if !csrf::validate(&jar, form.csrf_token.as_deref(), &headers, &taverna) {
        return Err(Error::Unauthorized("Invalid CSRF token".to_string()));
    }
    if current_user.email_verified {
        return Err(Error::BadRequest("Email already verified".to_string()));
    }
    let email = current_user
        .email
        .as_deref()
        .ok_or_else(|| Error::BadRequest("No email on file".to_string()))?;

    let token = email_verifications::issue(&ctx.db, current_user.id)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "failed to store verification token");
            Error::InternalServerError
        })?;
    verification::send_verification_email(
        &ctx,
        email,
        &current_user.username,
        &token,
        &taverna.base_url,
    )
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "failed to send verification email");
        Error::InternalServerError
    })?;

    Ok(format::redirect("/settings").into_response())
}
