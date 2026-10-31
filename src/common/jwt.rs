//! Session tokens (SPEC §3.4 / §7): a 1h access token and a 7d refresh
//! token, both HS256 JWTs carried in httpOnly cookies.
//!
//! The `kind` claim keeps the two apart, so a refresh token can never be
//! presented as an access token (and vice versa).
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};

use crate::models::_entities::users;

/// Cookie holding the access token.
pub const ACCESS_TOKEN_COOKIE: &str = "access_token";
/// Cookie holding the refresh token.
pub const REFRESH_TOKEN_COOKIE: &str = "refresh_token";

/// Which of the two session tokens a JWT is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TokenKind {
    Access,
    Refresh,
}

impl TokenKind {
    /// How long a token of this kind stays valid.
    #[must_use]
    pub fn lifetime(self) -> chrono::Duration {
        match self {
            Self::Access => chrono::Duration::hours(1),
            Self::Refresh => chrono::Duration::days(7),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: usize,
    pub is_admin: bool,
    pub kind: TokenKind,
}

fn create_token(
    user: &users::Model,
    secret: &str,
    kind: TokenKind,
) -> Result<String, jsonwebtoken::errors::Error> {
    let expiration = chrono::Utc::now()
        .checked_add_signed(kind.lifetime())
        .ok_or(jsonwebtoken::errors::ErrorKind::ExpiredSignature)?
        .timestamp();
    let claims = Claims {
        sub: user.id.to_string(),
        exp: usize::try_from(expiration)
            .map_err(|_| jsonwebtoken::errors::ErrorKind::ExpiredSignature)?,
        is_admin: user.is_admin,
        kind,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
}

/// Issues a 1-hour access token.
pub fn create_access_token(
    user: &users::Model,
    secret: &str,
) -> Result<String, jsonwebtoken::errors::Error> {
    create_token(user, secret, TokenKind::Access)
}

/// Issues a 7-day refresh token.
pub fn create_refresh_token(
    user: &users::Model,
    secret: &str,
) -> Result<String, jsonwebtoken::errors::Error> {
    create_token(user, secret, TokenKind::Refresh)
}

/// Verifies signature and expiry, and that the token is of the `expected` kind.
pub fn verify_token(
    token: &str,
    secret: &str,
    expected: TokenKind,
) -> Result<Claims, jsonwebtoken::errors::Error> {
    let claims = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )?
    .claims;
    if claims.kind != expected {
        return Err(jsonwebtoken::errors::ErrorKind::InvalidToken.into());
    }
    Ok(claims)
}
