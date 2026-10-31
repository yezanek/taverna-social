//! Session handling: the [`AuthUser`] / [`OptionalAuthUser`] extractors and
//! the [`refresh_session`] middleware that renews an expired access token
//! from the 7-day refresh token (SPEC §3.4).
use axum::{
    extract::{FromRef, FromRequestParts, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use loco_rs::controller::ErrorDetail;
use loco_rs::{Error, app::AppContext};
use tracing::instrument;

use crate::common::config::TavernaConfig;
use crate::common::jwt::{
    ACCESS_TOKEN_COOKIE, REFRESH_TOKEN_COOKIE, TokenKind, create_access_token, verify_token,
};
use crate::models::_entities::users;
use crate::models::users::UserModel;

#[derive(Debug, Clone)]
pub struct AuthUser(pub users::Model);

#[derive(Debug, Clone)]
pub struct OptionalAuthUser(pub Option<users::Model>);

/// Why a request could not be tied to a usable account.
#[derive(Debug)]
enum AuthFailure {
    Unauthenticated(&'static str),
    Banned,
}

impl IntoResponse for AuthFailure {
    fn into_response(self) -> Response {
        match self {
            Self::Unauthenticated(reason) => {
                Error::Unauthorized(reason.to_string()).into_response()
            }
            Self::Banned => {
                let mut response = Error::CustomError(
                    StatusCode::FORBIDDEN,
                    ErrorDetail::with_reason("Account is banned"),
                )
                .into_response();
                // A banned account loses its session.
                for name in [ACCESS_TOKEN_COOKIE, REFRESH_TOKEN_COOKIE] {
                    if let Ok(value) =
                        HeaderValue::from_str(&expired_cookie(name).encoded().to_string())
                    {
                        response.headers_mut().append(header::SET_COOKIE, value);
                    }
                }
                response
            }
        }
    }
}

/// Resolves the access-token cookie to a non-banned local user.
async fn authenticate(parts: &Parts, ctx: &AppContext) -> Result<users::Model, AuthFailure> {
    let jwt = ctx
        .config
        .get_jwt_config()
        .map_err(|_| AuthFailure::Unauthenticated("JWT not configured"))?;
    let jar = CookieJar::from_headers(&parts.headers);
    let token = jar
        .get(ACCESS_TOKEN_COOKIE)
        .ok_or(AuthFailure::Unauthenticated("Token not found in cookie"))?;
    let claims = verify_token(token.value(), &jwt.secret, TokenKind::Access)
        .map_err(|_| AuthFailure::Unauthenticated("Invalid token"))?;
    let user_id: uuid::Uuid = claims
        .sub
        .parse()
        .map_err(|_| AuthFailure::Unauthenticated("Invalid user ID in token"))?;
    let user = UserModel::find_by_id(&ctx.db, user_id)
        .await
        .map_err(|_| AuthFailure::Unauthenticated("User not found"))?
        .ok_or(AuthFailure::Unauthenticated("User not found"))?;
    if user.is_banned {
        return Err(AuthFailure::Banned);
    }
    Ok(user)
}

impl<S> FromRequestParts<S> for AuthUser
where
    AppContext: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = Response;

    #[instrument(skip(parts, state))]
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let ctx = AppContext::from_ref(state);
        authenticate(parts, &ctx)
            .await
            .map(Self)
            .map_err(IntoResponse::into_response)
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
        let ctx = AppContext::from_ref(state);
        Ok(Self(authenticate(parts, &ctx).await.ok()))
    }
}

/// Builds a session cookie (httpOnly, SameSite=Lax, path `/`).
#[must_use]
pub fn session_cookie(
    name: &'static str,
    value: String,
    max_age: time::Duration,
    secure: bool,
) -> Cookie<'static> {
    Cookie::build((name, value))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path("/")
        .secure(secure)
        .max_age(max_age)
        .build()
}

/// A cookie that tells the browser to drop `name`.
fn expired_cookie(name: &'static str) -> Cookie<'static> {
    Cookie::build((name, ""))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(time::Duration::ZERO)
        .build()
}

/// The access-token cookie for `token`.
#[must_use]
pub fn access_cookie(token: String, secure: bool) -> Cookie<'static> {
    session_cookie(
        ACCESS_TOKEN_COOKIE,
        token,
        time::Duration::seconds(TokenKind::Access.lifetime().num_seconds()),
        secure,
    )
}

/// Renews the access token from the refresh token.
///
/// When the access cookie is missing or expired but a valid refresh token is
/// present, a new access token is minted, swapped into this request's
/// `Cookie` header (so the extractors see it) and set on the response.
pub async fn refresh_session(
    State(ctx): State<AppContext>,
    mut req: Request,
    next: Next,
) -> Response {
    // Own the headers so the future does not borrow the (non-Sync) request.
    let headers = req.headers().clone();
    let path = req.uri().path().to_owned();
    let Some(token) = renewed_access_token(&ctx, &headers, &path).await else {
        return next.run(req).await;
    };

    let jar =
        CookieJar::from_headers(req.headers()).add(Cookie::new(ACCESS_TOKEN_COOKIE, token.clone()));
    let cookie_header = jar
        .iter()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect::<Vec<_>>()
        .join("; ");
    req.headers_mut().remove(header::COOKIE);
    if let Ok(value) = HeaderValue::from_str(&cookie_header) {
        req.headers_mut().insert(header::COOKIE, value);
    }

    let secure = req
        .extensions()
        .get::<TavernaConfig>()
        .is_some_and(|c| c.base_url.starts_with("https"));
    let mut response = next.run(req).await;
    if let Ok(value) = HeaderValue::from_str(&access_cookie(token, secure).encoded().to_string()) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    response
}

/// A fresh access token, when the request needs (and may have) one.
async fn renewed_access_token(ctx: &AppContext, headers: &HeaderMap, path: &str) -> Option<String> {
    // Logging out must not hand out a fresh session; static files need none.
    if path == "/logout" || path.starts_with("/static/") {
        return None;
    }
    let jwt = ctx.config.get_jwt_config().ok()?;
    let jar = CookieJar::from_headers(headers);
    if jar
        .get(ACCESS_TOKEN_COOKIE)
        .is_some_and(|c| verify_token(c.value(), &jwt.secret, TokenKind::Access).is_ok())
    {
        return None;
    }
    let refresh = jar.get(REFRESH_TOKEN_COOKIE)?;
    let claims = verify_token(refresh.value(), &jwt.secret, TokenKind::Refresh).ok()?;
    let user_id: uuid::Uuid = claims.sub.parse().ok()?;
    let user = UserModel::find_by_id(&ctx.db, user_id).await.ok()??;
    if user.is_banned {
        return None;
    }
    create_access_token(&user, &jwt.secret).ok()
}
