use axum::http::HeaderMap;
use axum::{Extension, Form};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use loco_rs::prelude::*;
use serde::Deserialize;
use tracing::instrument;

use crate::common::config::TavernaConfig;
use crate::controllers::csrf::same_origin;
use crate::initializers::theme_middleware::{THEME_COOKIE, Theme};

/// Form containing the selected theme value.
#[derive(Debug, Deserialize)]
pub struct ThemeForm {
    pub theme: String,
}

/// `POST /theme`: sets the `taverna_theme` cookie and redirects back
/// (SPEC §3.5). Unknown values fall back to light.
#[instrument(skip(jar, taverna, headers))]
pub async fn set_theme(
    jar: CookieJar,
    Extension(taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Form(form): Form<ThemeForm>,
) -> Result<Response> {
    let theme = Theme::parse(&form.theme).unwrap_or_default();
    let cookie = Cookie::build((THEME_COOKIE, theme.as_str()))
        .http_only(false)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(time::Duration::days(365))
        .build();

    let redirect_to = local_referer(&headers, &taverna).unwrap_or_else(|| "/".to_string());
    Ok((jar.add(cookie), format::redirect(&redirect_to)).into_response())
}

/// The Referer's path and query when it points at this instance. Anything
/// else (another site, a malformed value) is ignored so `/theme` cannot be
/// used as an open redirect.
fn local_referer(headers: &HeaderMap, taverna: &TavernaConfig) -> Option<String> {
    let referer = url::Url::parse(headers.get("referer")?.to_str().ok()?).ok()?;
    let base = url::Url::parse(&taverna.base_url).ok()?;
    if !same_origin(&referer, &base) {
        return None;
    }
    Some(match referer.query() {
        Some(q) => format!("{}?{}", referer.path(), q),
        None => referer.path().to_string(),
    })
}
