//! Double-submit CSRF protection shared by every form handler.
//!
//! A page that renders a form calls [`issue`], puts the token in a hidden
//! field and sets the returned cookie. The POST handler calls [`validate`],
//! which checks the Origin (or Referer) against `base_url` and compares the
//! form token with the cookie.
use axum::http::HeaderMap;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::Deserialize;

use crate::common::config::TavernaConfig;

/// Name of the cookie holding the CSRF token.
pub const CSRF_COOKIE: &str = "csrf_token";

/// Body of a POST form whose only field is the CSRF token
/// (join, leave, delete, pin, lock, remove, ban, ...).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CsrfForm {
    #[serde(default)]
    pub csrf_token: Option<String>,
}

/// Creates a fresh CSRF token and the cookie that carries it.
#[must_use]
pub fn issue() -> (String, Cookie<'static>) {
    let token = uuid::Uuid::new_v4().to_string();
    let cookie = Cookie::build((CSRF_COOKIE, token.clone()))
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(cfg!(not(debug_assertions)))
        .path("/")
        .build();
    (token, cookie)
}

/// Validates the CSRF token from the form against the cookie, and checks
/// that the request's Origin (or Referer) matches `base_url` when present.
#[must_use]
pub fn validate(
    jar: &CookieJar,
    form_token: Option<&str>,
    headers: &HeaderMap,
    taverna: &TavernaConfig,
) -> bool {
    if let Ok(base) = url::Url::parse(&taverna.base_url) {
        let origin = headers
            .get("origin")
            .or_else(|| headers.get("referer"))
            .and_then(|v| v.to_str().ok())
            .and_then(|v| url::Url::parse(v).ok());
        if let Some(origin_url) = origin
            && !same_origin(&origin_url, &base)
        {
            return false;
        }
    }
    match (jar.get(CSRF_COOKIE), form_token) {
        (Some(cookie), Some(form)) => !form.is_empty() && cookie.value() == form,
        _ => false,
    }
}

/// True when both URLs share scheme, host and port.
#[must_use]
pub fn same_origin(a: &url::Url, b: &url::Url) -> bool {
    a.scheme() == b.scheme()
        && a.host() == b.host()
        && a.port_or_known_default() == b.port_or_known_default()
}
