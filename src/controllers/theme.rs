use axum::http::HeaderMap;
use axum::{Extension, Form};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use loco_rs::prelude::*;
use serde::Deserialize;
use tracing::instrument;

use crate::common::config::TavernaConfig;

#[derive(Debug, Deserialize)]
pub struct ThemeForm {
    pub theme: String,
}

#[instrument(skip(jar, _ctx))]
pub async fn set_theme(
    jar: CookieJar,
    State(_ctx): State<AppContext>,
    Extension(_taverna): Extension<TavernaConfig>,
    headers: HeaderMap,
    Form(form): Form<ThemeForm>,
) -> Result<Response> {
    let theme_value = match form.theme.as_str() {
        "dark" | "sepia" => form.theme.clone(),
        _ => "light".to_string(),
    };

    let redirect_to = headers
        .get("referer")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("/")
        .to_string();

    let cookie = Cookie::build(("taverna_theme", theme_value))
        .http_only(false)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(time::Duration::seconds(31536000))
        .build();

    Ok((jar.add(cookie), format::redirect(&redirect_to)).into_response())
}
