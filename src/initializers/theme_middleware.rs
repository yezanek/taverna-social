use axum::{extract::Request, http::HeaderMap, middleware::Next, response::Response};
use axum_extra::extract::cookie::CookieJar;

/// Name of the theme cookie (SPEC §3.5).
pub const THEME_COOKIE: &str = "taverna_theme";

/// One of the three supported themes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Theme {
    #[default]
    Light,
    Dark,
    Sepia,
}

impl Theme {
    /// Parses a cookie/form value; `None` for anything unknown.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            "sepia" => Some(Self::Sepia),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
            Self::Sepia => "sepia",
        }
    }
}

/// The request's theme, as the string templates put in `data-theme`.
#[derive(Debug, Clone)]
pub struct CurrentTheme(pub String);

pub async fn theme_middleware(mut req: Request, next: Next) -> Response {
    let theme = extract_theme(req.headers()).unwrap_or_default();
    req.extensions_mut()
        .insert(CurrentTheme(theme.as_str().to_string()));
    next.run(req).await
}

/// Reads a valid theme from the `taverna_theme` cookie.
#[must_use]
pub fn extract_theme(headers: &HeaderMap) -> Option<Theme> {
    CookieJar::from_headers(headers)
        .get(THEME_COOKIE)
        .and_then(|c| Theme::parse(c.value()))
}
