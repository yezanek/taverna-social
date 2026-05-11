use axum::Form;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use loco_rs::prelude::*;
use serde::Deserialize;
use tracing::instrument;

#[derive(Debug, Deserialize)]
pub struct ThemeForm {
    pub theme: String,
}

#[instrument(skip(jar, _ctx))]
pub async fn set_theme(
    jar: CookieJar,
    State(_ctx): State<AppContext>,
    Form(form): Form<ThemeForm>,
) -> Result<Response> {
    let theme_value = match form.theme.as_str() {
        "dark" | "sepia" => form.theme.clone(),
        _ => "light".to_string(),
    };

    let cookie = Cookie::build(("taverna_theme", theme_value))
        .http_only(false)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(time::Duration::seconds(31536000))
        .build();

    Ok((jar.add(cookie), format::redirect("/")).into_response())
}
