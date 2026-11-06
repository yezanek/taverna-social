use axum::{extract::Request, http::HeaderMap, middleware::Next, response::Response};
use axum_extra::extract::cookie::CookieJar;

use crate::common::accent::{ACCENT_COOKIE, Rgb, accent_css};

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

/// The request's appearance: the theme templates put in `data-theme` and
/// the user's custom accent colour, if any.
#[derive(Debug, Clone, Copy, Default)]
pub struct CurrentTheme {
    pub theme: Theme,
    pub accent: Option<Rgb>,
}

impl CurrentTheme {
    /// Reads both preferences from the request's cookies.
    #[must_use]
    pub fn from_headers(headers: &HeaderMap) -> Self {
        Self {
            theme: extract_theme(headers).unwrap_or_default(),
            accent: extract_accent(headers),
        }
    }

    /// Template context: `theme`, `accent` (hex or null) and `accent_css`,
    /// the stylesheet overriding the theme accent (null for the default).
    #[must_use]
    pub fn context(&self) -> serde_json::Value {
        serde_json::json!({
            "theme": self.theme.as_str(),
            "accent": self.accent.map(Rgb::to_hex),
            "accent_css": self.accent.map(accent_css),
        })
    }
}

pub async fn theme_middleware(mut req: Request, next: Next) -> Response {
    let current = CurrentTheme::from_headers(req.headers());
    req.extensions_mut().insert(current);
    next.run(req).await
}

/// Reads a valid theme from the `taverna_theme` cookie.
#[must_use]
pub fn extract_theme(headers: &HeaderMap) -> Option<Theme> {
    CookieJar::from_headers(headers)
        .get(THEME_COOKIE)
        .and_then(|c| Theme::parse(c.value()))
}

/// Reads a valid accent colour from the `taverna_accent` cookie, which holds
/// the six hex digits without the `#`.
#[must_use]
pub fn extract_accent(headers: &HeaderMap) -> Option<Rgb> {
    CookieJar::from_headers(headers)
        .get(ACCENT_COOKIE)
        .and_then(|c| Rgb::parse(&format!("#{}", c.value())))
}
