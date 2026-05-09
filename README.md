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
taverna/
├── README.md                   ← you are here
├── AGENTS.md                   ← coding rules; read automatically by AI tools
├── SPEC.md                     ← full technical specification (authoritative)
├── DECISIONS.md                ← architecture decision log
├── PHASE_1.md                  ← scaffold & database
├── PHASE_2.md                  ← authentication
├── PHASE_3.md                  ← communities, posts, theme switcher
├── PHASE_4.md                  ← ActivityPub federation
├── PHASE_5.md                  ← profiles, moderation, production
├── OPENCODE_PROMPTS.md         ← prompts to paste into opencode (implementor)
├── GEMINI_PROMPTS.md           ← prompts to paste into gemini-cli (QA)
├── taverna_design.docx         ← full architecture design document
└── .opencode/
    └── commands/               ← opencode slash commands (auto-loaded by opencode)
        ├── implement-phase.md
        ├── implement-theme.md
        ├── implement-federation.md
        ├── fix-issue.md
        └── fix-clippy.md
```

Once the project is scaffolded by the AI tools, the Rust project structure appears alongside these files:
```
├── Cargo.toml
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
| opencode | Implements code | `AGENTS.md`, `.opencode/commands/*.md` |
| gemini-cli | QA and review | `AGENTS.md` |
| Both | Reference when told | `SPEC.md`, `PHASE_*.md`, `DECISIONS.md` |

`OPENCODE_PROMPTS.md` and `GEMINI_PROMPTS.md` are **your** cheat sheets — prompts you copy and paste into each tool. The tools do not read these files on their own.

### Workflow per phase

```
1. Open OPENCODE_PROMPTS.md → copy the phase prompt → paste into opencode
2. opencode shows a plan → you approve → it implements
3. Run the gate: cargo check && cargo test && cargo clippy -- -D warnings
4. Open GEMINI_PROMPTS.md → copy the phase QA prompt → paste into gemini-cli
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

## Prerequisites (NixOS)

Everything is available through Nix. No curl installers, no Homebrew, no npm globals.

### Nix shell (quick start)

Create a `shell.nix` in the project root:

```nix
{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  buildInputs = with pkgs; [
    # Rust
    rustup

    # Loco and SeaORM CLIs (installed via cargo below)
    cargo

    # PostgreSQL 16
    postgresql_16

    # pgmq (built from source via cargo-pgmq or installed as a PG extension)
    # See: https://github.com/tembo-io/pgmq — install the extension into your PG data dir

    # AI tools
    opencode        # opencode.ai — if not yet in nixpkgs, use the flake below
    gemini-cli      # google gemini CLI

    # Node (only needed for gemini-cli if not yet packaged natively)
    nodejs_22
  ];

  shellHook = ''
    export DATABASE_URL="postgres://localhost/taverna_development"
    export BASE_URL="http://localhost:3000"
    export TAVERNA_DOMAIN="localhost"
    # Generate once and add to your .env:
    # export JWT_SECRET=$(openssl rand -hex 32)
  '';
}
```

Then:
```bash
nix-shell
cargo install loco-cli sea-orm-cli   # first time only
```

### Nix flake (recommended)

Create `flake.nix` in the project root:

```nix
{
  description = "Taverna — federated community platform";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        rust = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };
      in {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            rust
            postgresql_16
            openssl
            pkg-config
            nodejs_22      # for gemini-cli until native package is available
          ];

          shellHook = ''
            export DATABASE_URL="postgres://localhost/taverna_development"
            export BASE_URL="http://localhost:3000"
            export TAVERNA_DOMAIN="localhost"
            export RUST_LOG="info"
            export PKG_CONFIG_PATH="${pkgs.openssl.dev}/lib/pkgconfig"

            echo "Taverna dev shell ready."
            echo "Run: cargo install loco-cli sea-orm-cli  (first time only)"
          '';
        };
      }
    );
}
```

```bash
nix develop          # enter the dev shell
cargo install loco-cli sea-orm-cli   # first time only
```

### Installing opencode and gemini-cli via Nix

```bash
# opencode — check nixpkgs first
nix-env -iA nixpkgs.opencode

# If not yet in nixpkgs, install via the official installer inside a nix-shell
# that has curl available, or use the flake from opencode's repo directly

# gemini-cli — via Node in the flake shell above
npm install -g @google/gemini-cli
gemini auth
```

### PostgreSQL on NixOS

Add to your `configuration.nix`:

```nix
services.postgresql = {
  enable = true;
  package = pkgs.postgresql_16;
  ensureDatabases = [ "taverna_development" "taverna_test" ];
  ensureUsers = [{
    name = "taverna";
    ensureDBOwnership = true;
  }];
};
```

For pgmq, install the extension after PostgreSQL is running:
```bash
# As the postgres superuser:
psql -U postgres -c "CREATE EXTENSION IF NOT EXISTS pgmq;"
# See https://github.com/tembo-io/pgmq for NixOS-specific packaging
```

---

## Running Locally (after implementation)

```bash
nix develop   # enter dev shell
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
