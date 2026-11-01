# Deployment Guide

**License:** AGPL-3.0-or-later — see LICENSE file.

## Requirements
- PostgreSQL 16 with extensions: uuid-ossp, ltree, pgmq
- Reverse proxy: Caddy or nginx (handles TLS)
- A domain name with TLS certificate

## PostgreSQL Setup
pgmq must be installed as a PostgreSQL extension:
  CREATE EXTENSION pgmq;  (requires superuser)

## Environment Variables
  DATABASE_URL=postgres://user:pass@localhost/taverna_production
  BASE_URL=https://your-domain.com
  TAVERNA_DOMAIN=your-domain.com
  JWT_SECRET=<openssl rand -hex 32>
  RUST_LOG=info

  Optional (defaults in config/production.example.yaml, `settings:` block):
  FEDERATION_ENABLED=true|false
  BLOCKED_DOMAINS=spam.example,bad.example
  HTTP_SIGNATURE_EXPIRY_SECONDS=30
  RATE_LIMITING_ENABLED=true|false
  TRUSTED_PROXIES=127.0.0.1,::1

## Configuration
  cp config/production.example.yaml config/production.yaml
  The template sets the connection pool for production load (10–100
  connections) and enables response compression. config/production.yaml is
  git-ignored.

## Build
  cargo build --release
  ./target/release/taverna-cli start

## Database Setup
  ./target/release/taverna-cli db migrate

## Reverse Proxy (Caddy example)
  Caddy terminates TLS and serves HTTP/2:
  your-domain.com {
    reverse_proxy localhost:3000 {
      header_up X-Forwarded-For {remote_host}
      header_up X-Real-IP {remote_host}
    }
    encode gzip zstd
  }

  Rate limits are kept per client address. Taverna only believes
  X-Forwarded-For / X-Real-IP from addresses listed in TRUSTED_PROXIES.
  With Caddy on the same host, set TRUSTED_PROXIES=127.0.0.1,::1;
  otherwise every visitor shares Caddy's address and one limit. Leave it
  empty if clients reach Taverna directly, so a forged header can't buy a
  fresh limit. Also bind the app to localhost (server.binding) when it
  sits behind a proxy.

## Background jobs
  The server process runs two background loops (not started in tests):
  - every 5 seconds, queued outbound ActivityPub deliveries in
    outbound_activities are handed to the delivery worker, with retries and
    backoff;
  - once a day, cached remote content (remote_actors, remote_posts,
    remote_comments) past its stale_after time is deleted. Local content is
    never touched by this job.

## First Run
  After deployment, create the first admin user via the registration page,
  then in psql:
    UPDATE users SET is_admin = true WHERE username = 'yourusername';
