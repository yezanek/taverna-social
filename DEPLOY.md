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

## Build
  cargo build --release
  ./target/release/taverna-cli start

## Database Setup
  ./target/release/taverna-cli db migrate

## Reverse Proxy (Caddy example)
  your-domain.com {
    reverse_proxy localhost:3000
  }

## First Run
  After deployment, create the first admin user via the registration page,
  then in psql:
    UPDATE users SET is_admin = true WHERE username = 'yourusername';
