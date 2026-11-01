# Deployment Guide

**License:** AGPL-3.0-or-later (see [LICENSE](LICENSE)). If you run a modified
Taverna for others, you must offer them its source.

This guide runs Taverna as one process behind Caddy on the same host. Every
setting mentioned here is described in
[docs/CONFIGURATION.md](docs/CONFIGURATION.md).

## Requirements

- PostgreSQL 16 with the `uuid-ossp` and `ltree` extensions available (the
  `postgresql-contrib` package). `pgmq` is not used.
- A domain name and a reverse proxy that terminates TLS (Caddy or nginx).
  Federation needs HTTPS: release builds refuse plain-HTTP ActivityPub URLs.
- Rust stable (1.85 or newer) to build.

## 1. Database

```sh
sudo -u postgres createuser --pwprompt taverna
sudo -u postgres createdb --owner taverna taverna_production
```

The first migration runs `CREATE EXTENSION IF NOT EXISTS` for `uuid-ossp`
and `ltree`. If the `taverna` role may not create extensions, create them
once as a superuser:

```sh
sudo -u postgres psql -d taverna_production \
  -c 'CREATE EXTENSION IF NOT EXISTS "uuid-ossp"; CREATE EXTENSION IF NOT EXISTS ltree;'
```

## 2. Build

```sh
cargo build --release --locked
```

The binary is `target/release/taverna-cli`. It reads `config/` and
`assets/` (templates and static files) from its working directory, so run it
from the repository root or deploy those two directories next to it.

## 3. Configure

```sh
cp config/production.example.yaml config/production.yaml   # git-ignored
```

The template sizes the connection pool for production (10 to 100
connections), turns on response compression, enables SMTP on port 587 and
leaves auto-migration off. Review `server.binding`: the template binds
`0.0.0.0`; behind a proxy on the same host, set it to `127.0.0.1` so the app
is only reachable through the proxy.

Set the environment (for example in a systemd `EnvironmentFile`, or a `.env`
file in the working directory):

```sh
LOCO_ENV=production
DATABASE_URL=postgres://taverna:<password>@localhost:5432/taverna_production
JWT_SECRET=<output of: openssl rand -hex 32>
BASE_URL=https://your-domain.com
TAVERNA_DOMAIN=your-domain.com
TRUSTED_PROXIES=127.0.0.1,::1
SMTP_HOST=smtp.your-provider.com

# Optional, defaults from the template's settings: block
FEDERATION_ENABLED=true
RATE_LIMITING_ENABLED=true
RUST_LOG=info
```

`BASE_URL` and `TAVERNA_DOMAIN` must both be set and agree; the template's
`taverna.example.com` fallbacks are placeholders. `DOMAIN` is no longer
read. If your SMTP server needs a login, see the mailer section of the
configuration guide.

## 4. Migrate

Production does not auto-migrate. Run this after every upgrade, before
starting the new version:

```sh
LOCO_ENV=production ./target/release/taverna-cli db migrate
```

## 5. Start

```sh
LOCO_ENV=production ./target/release/taverna-cli start
```

The server listens on port 3000. `LOCO_ENV=production ./target/release/taverna-cli doctor`
checks the configuration and database connection.

A minimal systemd unit:

```ini
[Unit]
Description=Taverna
After=network.target postgresql.service

[Service]
User=taverna
WorkingDirectory=/opt/taverna
EnvironmentFile=/opt/taverna/taverna.env
ExecStart=/opt/taverna/target/release/taverna-cli start
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

## 6. Reverse proxy (Caddy)

Caddy gets the certificate, terminates TLS and serves HTTP/2:

```
your-domain.com {
    reverse_proxy localhost:3000 {
        header_up X-Forwarded-For {remote_host}
        header_up X-Real-IP {remote_host}
    }
    encode gzip zstd
}
```

With `encode` in Caddy you can turn off `server.middlewares.compression` in
`config/production.yaml`.

Rate limits are kept per client address. Taverna only believes
`X-Forwarded-For` / `X-Real-IP` from addresses listed in `TRUSTED_PROXIES`.
With Caddy on the same host, set `TRUSTED_PROXIES=127.0.0.1,::1`; otherwise
every visitor shares Caddy's address and one limit. Leave it empty if
clients reach Taverna directly, so a forged header can't buy a fresh limit.

## 7. First run

Open `https://your-domain.com`. While the instance has no local users,
every page redirects to `/setup`. The wizard asks for the instance name,
whether registrations are open, and the first account, which becomes the
admin. After that, `/setup` answers 403. The admin manages users,
communities and instance settings at `/admin`.

There is no UI to promote more admins yet. To make another local user an
admin:

```sh
psql "$DATABASE_URL" -c "UPDATE users SET is_admin = true WHERE username = 'theirname' AND is_local;"
```

## Background jobs

The server process runs two background loops (not started in tests):

- every 5 seconds, queued outbound ActivityPub deliveries in
  `outbound_activities` are handed to the delivery worker, with retries and
  backoff;
- once a day, cached remote content (`remote_actors`, `remote_posts`,
  `remote_comments`) past its `stale_after` time is deleted. Local content
  is never touched by this job.

No separate worker process or scheduler is needed.

## Upgrading

```sh
git pull
cargo build --release --locked
LOCO_ENV=production ./target/release/taverna-cli db migrate
sudo systemctl restart taverna
```

Compare `config/production.yaml` with `config/production.example.yaml` after
each upgrade; new settings are added to the template first.
