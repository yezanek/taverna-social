# Configuration

Taverna is configured in three places:

1. **`config/<environment>.yaml`**, Loco's config file for the running
   environment. It holds the server, database, mailer and JWT settings, and
   Taverna's own settings in the `settings:` block.
2. **Environment variables**, which those YAML files read with
   `<%= get_env(name="...", default="...") %>`. A `.env` file in the working
   directory is loaded at startup (see [`.env.example`](../.env.example));
   variables already set in the environment take precedence.
3. **Instance settings** stored in the database and edited by an admin at
   `/admin/settings`.

## Choosing the environment

Loco picks the config file from the environment name: `--environment <name>`
(`-e`) on the command line, otherwise `LOCO_ENV`, otherwise `development`.

| File | Used for | Committed |
|---|---|---|
| `config/development.yaml` | `cargo loco start` locally | yes |
| `config/test.yaml` | `cargo test` | yes |
| `config/production.example.yaml` | template for `config/production.yaml` | yes |
| `config/production.yaml` | production | no (git-ignored) |

`config/local.yaml` and `config/*.local.yaml` are git-ignored too.

The binary reads `config/` and the templates in `assets/` relative to its
working directory, so run it from the repository root (or a copy of those
two directories).

## Environment variables

| Variable | Read by | Default (development) | Notes |
|---|---|---|---|
| `DATABASE_URL` | `database.uri` | `postgres://localhost:5432/taverna_development` | Required in production. |
| `TEST_DATABASE_URL` | `database.uri` in `test.yaml` | `postgres://localhost:5432/taverna_test` | The test suite truncates and recreates this database. |
| `JWT_SECRET` | `auth.jwt.secret` | `dev-secret-change-in-production` | Required in production. Generate with `openssl rand -hex 32`. The test config uses a fixed secret. |
| `BASE_URL` | `server.host`, `settings.base_url` | `http://localhost:3000` | Public URL, no trailing slash. All ActivityPub IDs are built from it. |
| `TAVERNA_DOMAIN` | `settings.domain` | `localhost:3000` | Public host name used for WebFinger handles and as the federation domain. Must match the host in `BASE_URL`. The old `DOMAIN` variable is no longer read. |
| `FEDERATION_ENABLED` | `settings.federation.enabled` | `false` (production template: `true`) | See [Federation](#federation). |
| `BLOCKED_DOMAINS` | `settings.federation.blocked_domains` | empty | Comma-separated. Parsed, but not yet enforced by the inbox. |
| `HTTP_SIGNATURE_EXPIRY_SECONDS` | `settings.federation.http_signature_expiry_seconds` | `30` | Parsed, but not yet used: signature checks use the `activitypub_federation` defaults. |
| `RATE_LIMITING_ENABLED` | `settings.rate_limiting.enabled` | `true` | |
| `TRUSTED_PROXIES` | `settings.rate_limiting.trusted_proxies` | empty | Comma-separated IPs or CIDRs. See [Rate limiting](#rate-limiting). |
| `SMTP_HOST` | `mailer.smtp.host` (production template only) | `localhost` | |
| `LOCO_ENV` | Loco | `development` | Selects the config file. |
| `RUST_LOG` | logger | unset | Overrides `logger.level` when set. |

## The `settings:` block

Taverna's own settings are deserialised from Loco's `settings:` block into
`TavernaConfig` (`src/common/config.rs`) once at boot and kept in the app's
shared store. The app refuses to start if the block is missing or invalid,
for example when a `trusted_proxies` entry is not an IP or CIDR.

```yaml
settings:
  base_url: <%= get_env(name="BASE_URL", default="http://localhost:3000") %>
  domain: <%= get_env(name="TAVERNA_DOMAIN", default="localhost:3000") %>
  federation:
    enabled: <%= get_env(name="FEDERATION_ENABLED", default="false") %>
    blocked_domains: "<%= get_env(name="BLOCKED_DOMAINS", default="") %>"
    http_signature_expiry_seconds: <%= get_env(name="HTTP_SIGNATURE_EXPIRY_SECONDS", default="30") %>
  rate_limiting:
    enabled: <%= get_env(name="RATE_LIMITING_ENABLED", default="true") %>
    trusted_proxies: "<%= get_env(name="TRUSTED_PROXIES", default="") %>"
```

| Key | Type | Required | Default if omitted |
|---|---|---|---|
| `base_url` | string | yes | |
| `domain` | string | yes | |
| `federation.enabled` | bool | yes | |
| `federation.blocked_domains` | list or comma-separated string | no | empty |
| `federation.http_signature_expiry_seconds` | integer | yes | |
| `rate_limiting` | map | no | enabled, no trusted proxies |
| `rate_limiting.enabled` | bool | no | `true` |
| `rate_limiting.trusted_proxies` | list or comma-separated string | no | empty |

Values can be hardcoded in the YAML instead of read from the environment;
list values can be written as YAML lists.

### Federation

`federation.enabled` controls whether local actions are sent to other
servers. When it is `false`, creating, editing, deleting and voting on posts
and comments, and joining or leaving remote communities, do not queue
outbound activities. The ActivityPub endpoints (WebFinger, NodeInfo, actors,
inboxes, outboxes, `/posts/{id}`, `/comments/{id}`) still respond.

Debug builds put `activitypub_federation` in debug mode, which allows plain
HTTP and local addresses. Release builds require HTTPS for federation, so a
production instance needs TLS in front of it (see [DEPLOY.md](../DEPLOY.md)).

Queued deliveries live in the `outbound_activities` table. A loop in the
server process picks up due rows every 5 seconds and delivers them with
retries and backoff; another loop deletes stale cached remote content once a
day. Neither loop runs in the test environment.

### Rate limiting

Requests are limited per client IP, in memory:

| Paths | Sustained | Burst |
|---|---|---|
| any path ending in `/inbox` | 5 per second | 20 |
| `/login`, `/register` | 10 per second | 30 |
| everything else | 60 per second | 600 |

Over the limit the server answers `429 Too Many Requests`.

The client is the TCP peer address. Only when the peer is listed in
`trusted_proxies` does Taverna read `X-Forwarded-For` (walking it from the
right past every trusted hop) or, without it, `X-Real-IP`. Behind a reverse
proxy on the same host, set `TRUSTED_PROXIES=127.0.0.1,::1`; otherwise
every visitor shares the proxy's address and one limit. When clients connect
directly, leave it empty so a forged header cannot buy a fresh limit.

## Loco settings worth knowing

| Setting | development | test | production template |
|---|---|---|---|
| `server.port` | 3000 | 5150 | 3000 |
| `server.binding` | `localhost` | default | `0.0.0.0` (use `127.0.0.1` behind a proxy) |
| `server.middlewares.compression` | off | off | on |
| `database.auto_migrate` | `true` | `true` | `false`: run `db migrate` yourself |
| `database.min/max_connections` | 2 / 20 | 0 / 10 | 10 / 100 |
| `mailer.smtp.enable` | `false` | stub mailer | `true`, port 587, TLS |
| `workers.mode` | `BackgroundAsync` | `ForegroundBlocking` | `BackgroundAsync` |
| `cache.kind` | `InMem` | `InMem` | `InMem` |

- **Static files** are served from `assets/static` at `/static`.
- **JWT sessions** are stored in the `access_token` cookie and expire after
  `auth.jwt.expiration` seconds (3600); the session is refreshed while the
  user is active.
- **Cache:** NodeInfo counts are cached in Loco's `ctx.cache`. Keep
  `cache.kind: InMem`; Loco's default null cache turns caching off.
- **Mail:** when no mailer is enabled, verification links are logged instead
  of sent. The production template sets only the SMTP host; if your server
  needs credentials, add Loco's `auth` block under `mailer.smtp`:

  ```yaml
  mailer:
    smtp:
      enable: true
      host: <%= get_env(name="SMTP_HOST") %>
      port: 587
      secure: true
      auth:
        user: <%= get_env(name="SMTP_USER") %>
        password: <%= get_env(name="SMTP_PASSWORD") %>
  ```

## Instance settings (admin UI)

These live in the single-row `instance_settings` table and are edited at
`/admin/settings`. The setup wizard sets the first two.

| Setting | Default | Effect |
|---|---|---|
| Instance name | `Taverna` | Name of the instance, set in the setup wizard. |
| Open registrations | on | When off, `/register` is closed. Also reported as `openRegistrations` in NodeInfo. |
| Community creation open | on | When on, every signed-in user may create communities and the rules below are ignored. When off, a user must pass every enabled rule below. |
| Require minimum account age | off, 30 days | Account must be at least this many days old. |
| Require verified email | off | Email address must be verified. |
| Require minimum standing | off, score 0 | Standing score must be at least this value. |
| Disqualify if content was removed | off | Users with moderator-removed posts or comments are not eligible. |
| Disqualify if currently suspended | off | Users with an active suspension are not eligible. |

The rules apply to admins too. Users can see their own eligibility on their
settings page.
