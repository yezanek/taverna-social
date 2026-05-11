use axum::response::Response;
use axum_extra::extract::cookie::CookieJar;
use loco_rs::prelude::*;
use serde_json::{json, Value};

pub fn get_theme(jar: &CookieJar) -> String {
    jar.get("taverna_theme")
        .map(|c| c.value().to_string())
        .filter(|v| v == "light" || v == "dark" || v == "sepia")
        .unwrap_or_else(|| "light".to_string())
}

#[allow(clippy::result_large_err)]
pub fn render(v: &TeraView, template: &str, data: Value) -> Result<Response> {
    let mut context = data;
    if let Some(obj) = context.as_object_mut() {
        if !obj.contains_key("theme") {
            obj.insert("theme".to_string(), json!("light"));
        }
    }

    let html = v.render(template, context)?;
    format::html(&html)
}
