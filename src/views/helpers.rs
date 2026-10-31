use axum::response::Response;
use loco_rs::prelude::*;
use serde_json::{Value, json};

use crate::initializers::theme_middleware::CurrentTheme;

pub fn render(v: &TeraView, template: &str, data: Value) -> Result<Response> {
    let html = v.render(template, data)?;
    format::html(&html)
}

pub fn render_with_theme(
    v: &TeraView,
    template: &str,
    mut ctx: Value,
    theme: &CurrentTheme,
) -> Result<Response> {
    if let Some(obj) = ctx.as_object_mut() {
        obj.insert("theme".to_string(), json!(theme.0));
    }
    render(v, template, ctx)
}
