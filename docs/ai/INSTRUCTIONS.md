# Taverna

> Full architecture, specification, and AI build plan designed by [Claude](https://claude.ai) (Anthropic).

A self-hostable, federated community platform — Facebook Groups meets old Reddit, built in Rust and part of the Fediverse.

- **Multi-community** per instance (like Lemmy / Reddit)
- **Federated** with Lemmy instances and Mastodon users via ActivityPub
- **Text and links only** in v1 — no media uploads
- **Three themes** — light, dark, sepia — no JavaScript required
- **Single binary** — PostgreSQL is the only external dependency

**License:** AGPL-3.0 (required by the `activitypub_federation` dependency)

---

## Tech Stack

| Layer | Technology |
|---|---|
| Language | Rust |
| Web framework | Loco (Axum) |
| Database | PostgreSQL 16 (data + job queue via pgmq) |
| Templates | Tera (Jinja2 syntax) |
| Frontend | HTMX — no JS framework |
| ActivityPub | `activitypub_federation` 0.6 by LemmyNet |

---

## Repository Layout

```
taverna-social/                 ← git repo root
├── README.md                   ← your project README (not this file)
├── AGENTS.md                   ← coding rules; auto-loaded by AI tools at startup
├── docs/
│   └── ai/                     ← all AI planning and instruction files
│       ├── INSTRUCTIONS.md     ← you are here
│       ├── SPEC.md             ← full technical specification (authoritative)
│       ├── DECISIONS.md        ← architecture decision log
│       ├── PHASE_1.md          ← scaffold & database
│       ├── PHASE_2.md          ← authentication
│       ├── PHASE_3.md          ← communities, posts, theme switcher
│       ├── PHASE_4.md          ← ActivityPub federation
│       ├── PHASE_5.md          ← profiles, moderation, production
│       ├── PHASE_6.md          ← admin onboarding, email verification, eligibility
│       ├── PHASE_7.md          ← bug fixes & UX improvements
│       ├── PHASE_8.md          ← performance, scalability & federated content isolation
│       ├── OPENCODE_PROMPTS.md ← prompts to paste into opencode (implementor)
│       ├── GEMINI_PROMPTS.md   ← prompts to paste into gemini-cli (QA)
│       └── taverna_design.docx ← full architecture design document
├── .opencode/
│   └── commands/               ← opencode slash commands (auto-loaded by opencode)
│       ├── implement-phase.md
│       ├── implement-theme.md
│       ├── implement-federation.md
│       ├── fix-issue.md
│       └── fix-clippy.md
├── .github/
│   ├── pull_request_template.md
│   ├── deny.toml
│   └── workflows/
│       ├── ci.yml
│       ├── pr-checks.yml
│       ├── version-bump.yml
│       └── dependencies.yml
│
│   ← Rust project files, scaffolded by opencode in Phase 1:
├── Cargo.toml
├── Cargo.lock
├── config/
├── migration/
├── src/
└── assets/
```

---

## Building with AI Tools

This project is designed to be implemented by **opencode** (writer) and reviewed by **gemini-cli** (QA). The phase files break the work into five sequential stages, each with a deliverables checklist and a gate (`cargo check && cargo test`) before proceeding.

### Tools

| Tool | Role | Reads automatically |
|---|---|---|
| opencode | Implements code | `AGENTS.md` (root), `.opencode/commands/*.md` |
| gemini-cli | QA and review | `AGENTS.md` (root) |
| Both | Reference when told | `docs/ai/SPEC.md`, `docs/ai/PHASE_*.md`, `docs/ai/DECISIONS.md` |

`docs/ai/OPENCODE_PROMPTS.md` and `docs/ai/GEMINI_PROMPTS.md` are **your** cheat sheets — prompts you copy and paste into each tool. The tools do not read these files on their own.

### Workflow per phase

```
1. Open `docs/ai/OPENCODE_PROMPTS.md` → copy the phase prompt → paste into opencode
2. opencode shows a plan → you approve → it implements
3. Run the gate: cargo check && cargo test && cargo clippy -- -D warnings
4. Open `docs/ai/GEMINI_PROMPTS.md` → copy the phase QA prompt → paste into gemini-cli
5. gemini-cli reports issues → copy findings → paste into opencode with /project:fix-issue
6. Re-run the gate → proceed to next phase
```

### opencode slash commands

Inside opencode, these commands are available once you open the project directory:

```
/project:implement-phase 1      implement a full phase (shows plan first)
/project:implement-theme        implement the theme switcher specifically
/project:implement-federation   implement the AP federation layer specifically
/project:fix-issue "..."        fix a specific issue reported by gemini-cli
/project:fix-clippy             clean up all lint warnings (shows plan first)
```

---

## Prerequisites (LMDE / Debian)

### Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
```

### Loco and SeaORM CLIs

```bash
cargo install loco-cli sea-orm-cli
```

### PostgreSQL 16

LMDE ships an older PostgreSQL. Add the official PGDG apt repository to get 16:

```bash
sudo apt install -y curl ca-certificates
sudo install -d /usr/share/postgresql-common/pgdg
sudo curl -o /usr/share/postgresql-common/pgdg/apt.postgresql.org.asc \
  --fail https://www.postgresql.org/media/keys/ACCC4CF8.asc
sudo sh -c 'echo "deb [signed-by=/usr/share/postgresql-common/pgdg/apt.postgresql.org.asc] \
  https://apt.postgresql.org/pub/repos/apt $(lsb_release -cs)-pgdg main" \
  > /etc/apt/sources.list.d/pgdg.list'
sudo apt update && sudo apt install -y postgresql-16
sudo systemctl enable --now postgresql
```

Create the development database:

```bash
sudo -u postgres createuser --superuser $USER
createdb taverna_development
createdb taverna_test
```

### pgmq

pgmq is a PostgreSQL extension not yet in apt. Build it from source using `pgrx`:

```bash
# Install build dependencies
sudo apt install -y build-essential libclang-dev postgresql-server-dev-16

# Install pgrx and build pgmq
cargo install --locked cargo-pgrx
cargo pgrx init --pg16 $(which pg_config)
git clone https://github.com/tembo-io/pgmq.git && cd pgmq
cargo pgrx install --pg-config $(which pg_config)

# Enable the extension
psql -d taverna_development -c "CREATE EXTENSION IF NOT EXISTS pgmq;"
```

### opencode

```bash
curl -fsSL https://opencode.ai/install | bash
```

### gemini-cli

gemini-cli requires Node.js:

```bash
sudo apt install -y nodejs npm
npm install -g @google/gemini-cli
gemini auth
```

---

### Alternative: Homebrew on Linux

If you prefer Linuxbrew for managing dev tools independently of apt:

```bash
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
echo 'eval "$(/home/linuxbrew/.linuxbrew/bin/brew shellenv)"' >> ~/.bashrc
source ~/.bashrc

brew install node
npm install -g @google/gemini-cli
# opencode via brew if packaged, otherwise use the curl installer above
```

### Alternative: Nix with home-manager

If you want reproducible tool versions without touching system packages:

```bash
# Install Nix (standalone, does not require NixOS)
sh <(curl -L https://nixos.org/nix/install) --daemon

# Install home-manager
nix-channel --add https://github.com/nix-community/home-manager/archive/master.tar.gz home-manager
nix-channel --update
nix-shell '<home-manager>' -A install
```

Add to `~/.config/home-manager/home.nix`:

```nix
{ config, pkgs, ... }:
{
  home.packages = with pkgs; [
    rustup
    nodejs_22
    opencode      # if packaged; otherwise use the curl installer
  ];
}
```

```bash
home-manager switch
cargo install loco-cli sea-orm-cli   # still needed via cargo
npm install -g @google/gemini-cli
```

PostgreSQL itself is best left to the system (`apt`) rather than home-manager, since it needs to run as a daemon and manage data directories.

---

## Running Locally (after implementation)

```bash
cargo loco db migrate
cargo loco start
# → http://localhost:3000
```

## Running Tests

```bash
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

## Production Deployment

See `DEPLOY.md` (created during Phase 5) for full instructions including PostgreSQL extension setup, environment variables, and reverse proxy configuration.

---

## Federation

Taverna federates as an ActivityPub server compatible with:
- **Lemmy** — communities subscribe and exchange posts
- **Mastodon** — users follow profiles and receive posts in their home feed

WebFinger: `/.well-known/webfinger?resource=acct:username@yourdomain`
NodeInfo: `/nodeinfo/2.0`
