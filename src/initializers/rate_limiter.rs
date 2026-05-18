use async_trait::async_trait;
use axum::Router as AxumRouter;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use governor::clock;
use governor::state::InMemoryState;
use governor::state::direct::NotKeyed;
use governor::{Quota, RateLimiter};
use loco_rs::Result;
use loco_rs::app::{AppContext, Initializer};
use std::num::NonZeroU32;
use std::sync::LazyLock;

static INBOX_LIMITER: LazyLock<RateLimiter<NotKeyed, InMemoryState, clock::DefaultClock>> =
    LazyLock::new(|| {
        RateLimiter::direct(
            Quota::per_second(NonZeroU32::new(5).unwrap())
                .allow_burst(NonZeroU32::new(20).unwrap()),
        )
    });

static AUTH_LIMITER: LazyLock<RateLimiter<NotKeyed, InMemoryState, clock::DefaultClock>> =
    LazyLock::new(|| {
        RateLimiter::direct(
            Quota::per_second(NonZeroU32::new(10).unwrap())
                .allow_burst(NonZeroU32::new(30).unwrap()),
        )
    });

static DEFAULT_LIMITER: LazyLock<RateLimiter<NotKeyed, InMemoryState, clock::DefaultClock>> =
    LazyLock::new(|| {
        RateLimiter::direct(
            Quota::per_second(NonZeroU32::new(60).unwrap())
                .allow_burst(NonZeroU32::new(600).unwrap()),
        )
    });

async fn rate_limit_middleware(req: Request, next: Next) -> Response {
    let path = req.uri().path();

    let limiter = if path.ends_with("/inbox") {
        &*INBOX_LIMITER
    } else if path == "/login" || path == "/register" {
        &*AUTH_LIMITER
    } else {
        &*DEFAULT_LIMITER
    };

    if limiter.check().is_ok() {
        next.run(req).await
    } else {
        StatusCode::TOO_MANY_REQUESTS.into_response()
    }
}

pub struct RateLimiterInitializer;

#[async_trait]
impl Initializer for RateLimiterInitializer {
    fn name(&self) -> String {
        "rate-limiter".to_string()
    }

    async fn after_routes(&self, router: AxumRouter, _ctx: &AppContext) -> Result<AxumRouter> {
        Ok(router.layer(middleware::from_fn(rate_limit_middleware)))
    }
}
