use axum::{
    extract::{FromRef, FromRequestParts},
    http::request::Parts,
};
use loco_rs::controller::ErrorDetail;
use loco_rs::{Error, app::AppContext};
use tracing::instrument;

use crate::common::jwt::verify_token;
use crate::models::_entities::users;
use crate::models::users::UserModel;

const ACCESS_TOKEN_COOKIE: &str = "access_token";

#[derive(Debug, Clone)]
pub struct AuthUser(pub users::Model);

#[derive(Debug, Clone)]
pub struct OptionalAuthUser(pub Option<users::Model>);

impl<S> FromRequestParts<S> for AuthUser
where
    AppContext: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = Error;

    #[instrument(skip(parts, state))]
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let ctx: AppContext = AppContext::from_ref(state);

        let jwt_secret = ctx
            .config
            .get_jwt_config()
            .map_err(|_| Error::Unauthorized("JWT not configured".to_string()))?;

        let token = extract_token_from_cookie(parts, ACCESS_TOKEN_COOKIE)?;
        let claims = verify_token(&token, &jwt_secret.secret)
            .map_err(|_| Error::Unauthorized("Invalid token".to_string()))?;

        let user_id: uuid::Uuid = claims
            .sub
            .parse()
            .map_err(|_| Error::Unauthorized("Invalid user ID in token".to_string()))?;

        let user = UserModel::find_by_id(&ctx.db, user_id)
            .await
            .map_err(|_| Error::Unauthorized("User not found".to_string()))?
            .ok_or_else(|| Error::Unauthorized("User not found".to_string()))?;

        if user.is_banned {
            return Err(Error::CustomError(
                axum::http::StatusCode::FORBIDDEN,
                ErrorDetail::with_reason("Account is banned"),
            ));
        }

        Ok(Self(user))
    }
}

impl<S> FromRequestParts<S> for OptionalAuthUser
where
    AppContext: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    #[instrument(skip(parts, state))]
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let ctx: AppContext = AppContext::from_ref(state);

        let jwt_secret = match ctx.config.get_jwt_config() {
            Ok(c) => c,
            Err(_) => return Ok(Self(None)),
        };

        let token = match extract_token_from_cookie(parts, ACCESS_TOKEN_COOKIE) {
            Ok(t) => t,
            Err(_) => return Ok(Self(None)),
        };

        let claims = match verify_token(&token, &jwt_secret.secret) {
            Ok(c) => c,
            Err(_) => return Ok(Self(None)),
        };

        let user_id: uuid::Uuid = match claims.sub.parse() {
            Ok(id) => id,
            Err(_) => return Ok(Self(None)),
        };

        let user = match UserModel::find_by_id(&ctx.db, user_id).await {
            Ok(Some(u)) => u,
            _ => return Ok(Self(None)),
        };

        if user.is_banned {
            return Ok(Self(None));
        }

        Ok(Self(Some(user)))
    }
}

#[allow(clippy::result_large_err)]
fn extract_token_from_cookie(parts: &Parts, name: &str) -> Result<String, Error> {
    for header_value in parts.headers.get_all("cookie") {
        let Ok(cookies) = header_value.to_str() else {
            continue;
        };
        for cookie in cookies.split(';') {
            let cookie = cookie.trim();
            if let Some(value) = cookie.strip_prefix(&format!("{}=", name)) {
                return Ok(value.to_string());
            }
        }
    }

    Err(Error::Unauthorized("Token not found in cookie".to_string()))
}
