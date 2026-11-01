use async_trait::async_trait;
use axum::Router as AxumRouter;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use governor::{DefaultKeyedRateLimiter, Quota};
use loco_rs::Result;
use loco_rs::app::{AppContext, Initializer};
use std::num::NonZeroU32;

use crate::common::config::TavernaConfig;
use std::sync::LazyLock;
use std::time::Duration;

/// How often idle client entries are dropped from the limiters.
const PRUNE_INTERVAL: Duration = Duration::from_secs(60);

fn client_key(req: &Request) -> String {
    req.headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|s| s.trim().to_string())
        .or_else(|| {
            req.headers()
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "127.0.0.1".to_string())
}

macro_rules! nzu32 {
    ($n:literal) => {
        match NonZeroU32::new($n) {
            Some(n) => n,
            None => unreachable!("non-zero literal {}", $n),
        }
    };
}

static INBOX_LIMITER: LazyLock<DefaultKeyedRateLimiter<String>> = LazyLock::new(|| {
    DefaultKeyedRateLimiter::keyed(Quota::per_second(nzu32!(5)).allow_burst(nzu32!(20)))
});

static AUTH_LIMITER: LazyLock<DefaultKeyedRateLimiter<String>> = LazyLock::new(|| {
    DefaultKeyedRateLimiter::keyed(Quota::per_second(nzu32!(10)).allow_burst(nzu32!(30)))
});

static DEFAULT_LIMITER: LazyLock<DefaultKeyedRateLimiter<String>> = LazyLock::new(|| {
    DefaultKeyedRateLimiter::keyed(Quota::per_second(nzu32!(60)).allow_burst(nzu32!(600)))
});

async fn rate_limit_middleware(req: Request, next: Next) -> Response {
    let path = req.uri().path();
    let key = client_key(&req);

    let limiter = if path.ends_with("/inbox") {
        &*INBOX_LIMITER
    } else if path == "/login" || path == "/register" {
        &*AUTH_LIMITER
    } else {
        &*DEFAULT_LIMITER
    };

    if limiter.check_key(&key).is_ok() {
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

    async fn after_routes(&self, router: AxumRouter, ctx: &AppContext) -> Result<AxumRouter> {
        let settings = ctx
            .shared_store
            .get::<TavernaConfig>()
            .ok_or_else(|| loco_rs::Error::string("TavernaConfig missing from shared store"))?
            .rate_limiting;
        if !settings.enabled {
            return Ok(router);
        }
        // Keyed limiters keep one entry per client forever unless pruned;
        // drop the ones whose quota has fully replenished.
        tokio::spawn(async {
            let mut interval = tokio::time::interval(PRUNE_INTERVAL);
            loop {
                interval.tick().await;
                for limiter in [&*INBOX_LIMITER, &*AUTH_LIMITER, &*DEFAULT_LIMITER] {
                    limiter.retain_recent();
                    limiter.shrink_to_fit();
                }
            }
        });
        Ok(router.layer(middleware::from_fn(rate_limit_middleware)))
    }
}
