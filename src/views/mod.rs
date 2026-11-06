use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::response::Response;
use loco_rs::controller::views::{ViewEngine, ViewRenderer, engines::TeraView};
use loco_rs::prelude::*;
use serde_json::Value;

use crate::initializers::theme_middleware::CurrentTheme;

/// The view engine bundled with the request's theme and accent (SPEC §3.5).
///
/// Handlers take `views: Views` and call `views.render(template, data)`; the
/// theme and accent are added to every template's context here, so no
/// handler passes them.
#[derive(Clone)]
pub struct Views {
    engine: TeraView,
    appearance: CurrentTheme,
}

impl Views {
    /// Renders `template` with `data` plus the appearance into an HTML response.
    pub fn render(&self, template: &str, data: Value) -> Result<Response> {
        format::html(&self.render_string(template, data)?)
    }

    /// Renders `template` with `data` plus the appearance into a string.
    /// Values the handler already set win.
    pub fn render_string(&self, template: &str, mut data: Value) -> Result<String> {
        if let (Some(obj), Value::Object(appearance)) =
            (data.as_object_mut(), self.appearance.context())
        {
            for (key, value) in appearance {
                obj.entry(key).or_insert(value);
            }
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
        // The theme middleware always sets it; fall back to the defaults.
        let appearance = parts
            .extensions
            .get::<CurrentTheme>()
            .copied()
            .unwrap_or_default();
        Ok(Self { engine, appearance })
    }
}
