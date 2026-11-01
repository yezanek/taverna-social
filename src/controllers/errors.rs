//! Error helpers shared by the HTML controllers.
//!
//! `Error::Unauthorized` (401) means "not signed in"; a signed-in user who
//! may not do something, or a request whose CSRF token does not check out,
//! gets a 403 instead.
use axum::http::StatusCode;
use loco_rs::{Error, controller::ErrorDetail};

/// A 403 with `reason` as the error detail.
#[must_use]
pub fn forbidden(reason: &str) -> Error {
    Error::CustomError(StatusCode::FORBIDDEN, ErrorDetail::with_reason(reason))
}

/// The 403 for a missing or mismatched CSRF token.
#[must_use]
pub fn invalid_csrf() -> Error {
    forbidden("Invalid CSRF token")
}
