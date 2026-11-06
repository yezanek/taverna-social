use axum::http::HeaderMap;
use axum::{Extension, Form};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use loco_rs::prelude::*;
use serde::Deserialize;
use tracing::instrument;

use crate::common::accent::{ACCENT_COOKIE, Rgb};
use crate::common::config::TavernaConfig;
use crate::controllers::csrf::same_origin;
use crate::initializers::theme_middleware::{THEME_COOKIE, Theme};
use crate::models::_entities::users;

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
    let redirect_to = local_referer(&headers, &taverna).unwrap_or_else(|| "/".to_string());
    Ok((jar.add(theme_cookie(theme)), format::redirect(&redirect_to)).into_response())
}

/// A one-year preference cookie readable by the whole site.
fn preference_cookie(
    name: &'static str,
    value: String,
    max_age: time::Duration,
) -> Cookie<'static> {
    Cookie::build((name, value))
        .http_only(false)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(max_age)
        .build()
}

/// The `taverna_theme` cookie for `theme`.
#[must_use]
pub fn theme_cookie(theme: Theme) -> Cookie<'static> {
    preference_cookie(
        THEME_COOKIE,
        theme.as_str().to_string(),
        time::Duration::days(365),
    )
}

/// The `taverna_accent` cookie for `accent` (hex digits without `#`, which
/// cookie encoding would escape); `None` expires the cookie so the theme's
/// own accent is used.
#[must_use]
pub fn accent_cookie(accent: Option<Rgb>) -> Cookie<'static> {
    match accent {
        Some(rgb) => preference_cookie(
            ACCENT_COOKIE,
            rgb.to_hex()[1..].to_string(),
            time::Duration::days(365),
        ),
        None => preference_cookie(ACCENT_COOKIE, String::new(), time::Duration::ZERO),
    }
}

/// Copies a user's saved theme and accent into `jar` (used on login), so
/// the account's appearance follows it to a new browser. A user with no
/// saved theme keeps whatever this browser already has.
pub fn apply_saved_appearance(jar: CookieJar, user: &users::Model) -> CookieJar {
    let jar = match user.theme.as_deref().and_then(Theme::parse) {
        Some(theme) => jar.add(theme_cookie(theme)),
        None => jar,
    };
    jar.add(accent_cookie(
        user.accent_color.as_deref().and_then(Rgb::parse),
    ))
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
