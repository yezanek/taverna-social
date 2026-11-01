use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::response::Response;
use loco_rs::controller::views::{ViewEngine, ViewRenderer, engines::TeraView};
use loco_rs::prelude::*;
use serde_json::{Value, json};

use crate::initializers::theme_middleware::{CurrentTheme, Theme};

/// The view engine bundled with the request's theme (SPEC §3.5).
///
/// Handlers take `views: Views` and call `views.render(template, data)`; the
/// theme is added to every template's context here, so no handler passes it.
#[derive(Clone)]
pub struct Views {
    engine: TeraView,
    theme: String,
}

impl Views {
    /// Renders `template` with `data` plus `theme` into an HTML response.
    pub fn render(&self, template: &str, data: Value) -> Result<Response> {
        format::html(&self.render_string(template, data)?)
    }

    /// Renders `template` with `data` plus `theme` into a string.
    pub fn render_string(&self, template: &str, mut data: Value) -> Result<String> {
        if let Some(obj) = data.as_object_mut() {
            obj.entry("theme").or_insert_with(|| json!(self.theme));
        }
        self.engine.render(template, data)
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Views {
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let ViewEngine(engine) = parts
            .extensions
            .get::<ViewEngine<TeraView>>()
            .cloned()
            .ok_or_else(|| Error::string("view engine missing from request extensions"))?;
        // The theme middleware always sets it; fall back to the default theme.
        let theme = parts
            .extensions
            .get::<CurrentTheme>()
            .map_or_else(|| Theme::default().as_str().to_string(), |t| t.0.clone());
        Ok(Self { engine, theme })
    }
}
