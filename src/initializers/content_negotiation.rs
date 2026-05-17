use activitypub_federation::FEDERATION_CONTENT_TYPE;
use axum::{extract::Request, http::header::ACCEPT, middleware::Next, response::Response};

#[derive(Clone, Debug)]
pub enum RenderMode {
    Html,
    ActivityPub,
}

pub async fn content_negotiation_middleware(mut req: Request, next: Next) -> Response {
    let accept = req
        .headers()
        .get(ACCEPT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let mode = if accept.contains(FEDERATION_CONTENT_TYPE) || accept.contains("application/ld+json")
    {
        RenderMode::ActivityPub
    } else {
        RenderMode::Html
    };

    req.extensions_mut().insert(mode);
    next.run(req).await
}
