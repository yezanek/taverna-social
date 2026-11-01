use async_trait::async_trait;
use axum::Router as AxumRouter;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use governor::{DefaultKeyedRateLimiter, Quota};
use ipnet::IpNet;
use loco_rs::Result;
use loco_rs::app::{AppContext, Initializer};
use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroU32;
use std::sync::Arc;

use crate::common::config::TavernaConfig;
use std::sync::LazyLock;
use std::time::Duration;

/// How often idle client entries are dropped from the limiters.
const PRUNE_INTERVAL: Duration = Duration::from_secs(60);

/// The address a request is rate limited under.
///
/// The socket peer is the client, unless the peer is one of the trusted
/// proxies. Then `X-Forwarded-For` is walked from the right (the hop the
/// proxy itself appended) past every trusted hop, and the first untrusted
/// address is the client. A forged left part of the header is never reached
/// while an untrusted hop sits to its right. Without `X-Forwarded-For`, a
/// trusted proxy's `X-Real-IP` is used.
fn client_ip(peer: IpAddr, headers: &HeaderMap, trusted: &[IpNet]) -> IpAddr {
    let is_trusted = |ip: &IpAddr| trusted.iter().any(|net| net.contains(ip));
    if !is_trusted(&peer) {
        return peer;
    }

    let forwarded: Vec<&str> = headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    if forwarded.is_empty() {
        return headers
            .get("x-real-ip")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(peer);
    }

    let mut client = peer;
    for hop in forwarded.iter().rev() {
        if !is_trusted(&client) {
            break;
        }
        match hop.parse::<IpAddr>() {
            Ok(ip) => client = ip,
            // A malformed hop came from whoever `client` is; stop there.
            Err(_) => break,
        }
    }
    client
}

/// Parses `trusted_proxies` entries, each an IP address or a CIDR.
fn parse_trusted_proxies(entries: &[String]) -> Result<Vec<IpNet>> {
    entries
        .iter()
        .map(|entry| {
            entry
                .parse::<IpNet>()
                .or_else(|_| entry.parse::<IpAddr>().map(IpNet::from))
                .map_err(|_| {
                    loco_rs::Error::string(&format!(
                        "invalid trusted_proxies entry '{entry}': expected an IP address or CIDR"
                    ))
                })
        })
        .collect()
}

macro_rules! nzu32 {
    ($n:literal) => {
        match NonZeroU32::new($n) {
            Some(n) => n,
            None => unreachable!("non-zero literal {}", $n),
        }
    };
}

static INBOX_LIMITER: LazyLock<DefaultKeyedRateLimiter<IpAddr>> = LazyLock::new(|| {
    DefaultKeyedRateLimiter::keyed(Quota::per_second(nzu32!(5)).allow_burst(nzu32!(20)))
});

static AUTH_LIMITER: LazyLock<DefaultKeyedRateLimiter<IpAddr>> = LazyLock::new(|| {
    DefaultKeyedRateLimiter::keyed(Quota::per_second(nzu32!(10)).allow_burst(nzu32!(30)))
});

static DEFAULT_LIMITER: LazyLock<DefaultKeyedRateLimiter<IpAddr>> = LazyLock::new(|| {
    DefaultKeyedRateLimiter::keyed(Quota::per_second(nzu32!(60)).allow_burst(nzu32!(600)))
});

async fn rate_limit_middleware(
    State(trusted): State<Arc<Vec<IpNet>>>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path();
    // Loco serves with `into_make_service_with_connect_info`, so the peer is
    // always there; without it every request shares one bucket.
    let key = match req.extensions().get::<ConnectInfo<SocketAddr>>() {
        Some(ConnectInfo(addr)) => client_ip(addr.ip(), req.headers(), &trusted),
        None => IpAddr::from([0, 0, 0, 0]),
    };

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
        let trusted = Arc::new(parse_trusted_proxies(&settings.trusted_proxies)?);
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
        Ok(router.layer(middleware::from_fn_with_state(
            trusted,
            rate_limit_middleware,
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.append(*name, value.parse().unwrap());
        }
        map
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn trusted() -> Vec<IpNet> {
        parse_trusted_proxies(&["127.0.0.1".into(), "10.0.0.0/8".into()]).unwrap()
    }

    #[test]
    fn untrusted_peer_ignores_forwarded_headers() {
        let h = headers(&[("x-forwarded-for", "1.2.3.4"), ("x-real-ip", "5.6.7.8")]);
        assert_eq!(
            client_ip(ip("203.0.113.9"), &h, &trusted()),
            ip("203.0.113.9")
        );
        assert_eq!(client_ip(ip("127.0.0.1"), &h, &[]), ip("127.0.0.1"));
    }

    #[test]
    fn trusted_peer_uses_rightmost_untrusted_hop() {
        // The client forged the left entry; the proxy appended the real one.
        let h = headers(&[("x-forwarded-for", "6.6.6.6, 198.51.100.7, 10.1.2.3")]);
        assert_eq!(
            client_ip(ip("127.0.0.1"), &h, &trusted()),
            ip("198.51.100.7")
        );
    }

    #[test]
    fn multiple_forwarded_headers_are_read_in_order() {
        let h = headers(&[
            ("x-forwarded-for", "6.6.6.6"),
            ("x-forwarded-for", "198.51.100.7"),
        ]);
        assert_eq!(
            client_ip(ip("127.0.0.1"), &h, &trusted()),
            ip("198.51.100.7")
        );
    }

    #[test]
    fn malformed_hop_stops_at_the_last_trusted_address() {
        let h = headers(&[("x-forwarded-for", "198.51.100.7, garbage")]);
        assert_eq!(client_ip(ip("10.0.0.1"), &h, &trusted()), ip("10.0.0.1"));
    }

    #[test]
    fn all_trusted_hops_resolve_to_the_leftmost() {
        let h = headers(&[("x-forwarded-for", "10.0.0.5, 10.0.0.6")]);
        assert_eq!(client_ip(ip("127.0.0.1"), &h, &trusted()), ip("10.0.0.5"));
    }

    #[test]
    fn trusted_peer_falls_back_to_real_ip_then_peer() {
        let h = headers(&[("x-real-ip", "198.51.100.7")]);
        assert_eq!(
            client_ip(ip("127.0.0.1"), &h, &trusted()),
            ip("198.51.100.7")
        );
        assert_eq!(
            client_ip(ip("127.0.0.1"), &HeaderMap::new(), &trusted()),
            ip("127.0.0.1")
        );
    }

    #[test]
    fn trusted_proxies_must_be_ips_or_cidrs() {
        assert!(parse_trusted_proxies(&["::1".into(), "fd00::/8".into()]).is_ok());
        assert!(parse_trusted_proxies(&["proxy.local".into()]).is_err());
    }
}
