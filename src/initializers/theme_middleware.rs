use axum::{extract::Request, http::HeaderMap, middleware::Next, response::Response};

#[derive(Debug, Clone)]
pub struct CurrentTheme(pub String);

pub async fn theme_middleware(mut req: Request, next: Next) -> Response {
    let theme = extract_theme(req.headers()).unwrap_or_else(|| "light".to_string());
    req.extensions_mut().insert(CurrentTheme(theme));
    next.run(req).await
}

fn extract_theme(headers: &HeaderMap) -> Option<String> {
    let cookie_header = headers.get("cookie")?.to_str().ok()?;
    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if let Some(value) = cookie.strip_prefix("taverna_theme=") {
            let v = value.to_string();
            if v == "light" || v == "dark" || v == "sepia" {
                return Some(v);
            }
        }
    }
    None
}
