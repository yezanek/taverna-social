# Working on Taverna with AI coding agents

Taverna was planned and built with AI coding agents, and is still developed
that way. This file explains how: which files an agent reads, the workflow
for a change, the checks every change must pass, and prompts you can reuse.

Everything here works with any agent that can read the repository and run
shell commands (Claude Code, opencode, gemini-cli, Codex, Cursor, Aider, …).
Nothing depends on one tool's slash commands or config folders.

---

## The files

| File | What it is | Who reads it |
|---|---|---|
| [`AGENTS.md`](../../AGENTS.md) | Coding rules, project layout, git conventions | Every agent, first. Many tools load it automatically; for one that looks for another file name (such as `CLAUDE.md` or `GEMINI.md`), point that file at `AGENTS.md` instead of copying it. |
| [`SPEC.md`](SPEC.md) | The functional and technical specification | Every agent, before changing behaviour |
| [`DECISIONS.md`](DECISIONS.md) | Architecture decision records (ADR-001 …) | Before changing anything an ADR covers |
| [`../DESIGN.md`](../DESIGN.md) | How the system is built today | For orientation |
| [`PHASE_1.md` … `PHASE_8.md`](.) | The plans the project was built from, each with its implementation and review prompts | When redoing or extending that area, or as a template for a new phase |
| [`REVIEW.md`](REVIEW.md) | Whole-codebase review prompts: targeted checks, final acceptance test, audit | The reviewing agent |
| [`../CONFIGURATION.md`](../CONFIGURATION.md), [`../../DEPLOY.md`](../../DEPLOY.md) | Every setting; production setup | When touching config or deployment |

All eight phases are complete. The phase files are kept as a record and as
templates; where one disagrees with the code, the code, SPEC.md, DECISIONS.md
and DESIGN.md win. Each phase file starts with a note on what has changed
since it was written.

---

## Roles

Work is split between two roles. They can be two different tools, or the same
tool in two separate sessions; what matters is that the reviewer did not write
the code it reviews.

| Role | Does | Reads |
|---|---|---|
| **Implementer** | Plans and writes the change, runs the gate | AGENTS.md, SPEC.md, the relevant phase file or issue |
| **Reviewer** | Checks the change against the spec and the rules, reports findings | AGENTS.md, SPEC.md, the phase file's review prompt or REVIEW.md |

You (the human) approve the plan, decide on findings, and merge.

---

## Workflow for a change

1. **Branch.** Create a branch with a semantic name (see
   [Branches and pull requests](#branches-and-pull-requests)) from `master`.
2. **Plan.** Ask the implementer for a plan before any code: every file it
   will create or change and why, and any commands it will run. Approve or
   correct it.
3. **Implement.** The implementer works task by task and runs `cargo check`
   after each one.
4. **Gate.** Run the checks below. All must pass.
5. **Review.** Start the reviewer in a fresh session with the phase's review
   prompt, or a prompt from REVIEW.md.
6. **Fix.** Give the findings to the implementer with the "Fix a reported
   issue" prompt below. Re-run the gate.
7. **Pull request.** Push and open a PR with a semantic title, filling in
   `.github/pull_request_template.md`. CI and CodeRabbit run on it. Merge when
   CI is green and no CRITICAL findings remain.
8. **Record.** If the change makes a design decision, add an ADR to
   DECISIONS.md. If it changes behaviour or settings, update SPEC.md,
   DESIGN.md, README, CONFIGURATION.md or DEPLOY.md as needed.

### The gate

These are the same checks CI runs:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

Tests need PostgreSQL 16 and a test database (see "Development setup").
Never delete or skip a test to make the gate pass; fix the code.

---

## Branches and pull requests

Always use semantic names. Never use generic names such as `patch-1`,
`update`, `fix`, `wip` or a tool's default like `claude/project-thread-1234`
or `opencode/session-5`.

**Branches:** `<type>/<short-kebab-case-summary>`, for example
`feat/community-bans`, `fix/safari-export-button`,
`chore/remove-ai-tool-leftovers`, `docs/design-document`.

**Pull request titles:** a conventional commit, `<type>: <summary>` in the
imperative, for example `fix: key rate limits on the socket peer`. CI's
PR Title check rejects other titles.

**Commit messages:** the same `<type>: <summary>` form.

`<type>` is one of `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `perf`,
matching the PR template. Pick the type of the main change; a branch holds
one change.

---

## Development setup

See the [README](../../README.md) for the full guide. In short:

1. **Rust** stable, 1.85 or newer (edition 2024):
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```
2. **PostgreSQL 16** with the contrib extensions (`uuid-ossp`, `ltree`). On
   Debian or Ubuntu releases that ship an older version, add the PGDG apt
   repository from postgresql.org. Then:
   ```bash
   sudo -u postgres createuser --superuser "$USER"
   createdb taverna_development
   createdb taverna_test
   ```
   pgmq is **not** needed (ADR-021).
3. **No extra CLI.** `cargo loco` is an alias for `cargo run --` (see
   `.cargo/config.toml`). Install `loco-cli` or `sea-orm-cli` only if you want
   Loco's generators or `sea-orm-cli generate entity`.
4. **Your agent.** Install it as its documentation says, and start it in the
   repository root so it sees `AGENTS.md`.

To keep `target/` small, which matters on small disks and CI runners:

```bash
export CARGO_PROFILE_DEV_DEBUG=line-tables-only CARGO_INCREMENTAL=0
```

---

## Reusable prompts

Paste these into the implementer. Replace the parts in `[brackets]`.

### Implement a phase or feature

```
You are implementing [Phase N / the feature described in issue #N] of Taverna.

Step 1: read these files completely before writing any code:
1. AGENTS.md: coding rules, project layout, git conventions
2. docs/ai/SPEC.md: the specification
3. [docs/ai/PHASE_N.md / the issue]: the tasks and deliverables
4. docs/ai/DECISIONS.md entries that cover the area you will touch

Step 2: show me a complete plan:
- every file you will CREATE, with one line on what goes in it
- every file you will MODIFY, with one line on what changes
- any commands you will run (migrations, code generation)
- the branch name you will use (<type>/<short-kebab-summary>)
Do not write code or run commands yet. Wait for my approval.

Step 3: after I approve, implement each task in order. Run cargo check after
each task and fix errors before moving on.

Step 4: when all tasks are done, run the gate:
  cargo fmt --all
  cargo clippy --all-targets --all-features -- -D warnings
  cargo test --all-features
Fix failures in the code; never delete or skip tests.

Step 5: confirm every item in the deliverables checklist and report anything
missing.
```

### Fix a reported issue

```
A review reported the following issue:

[paste the finding]

Before changing anything:
1. Read AGENTS.md and the relevant part of docs/ai/SPEC.md to work out the
   correct behaviour.
2. Find the code involved.
3. Show me a plan: exactly what you will change and why. If you think the
   finding is wrong, say so and show the evidence instead.

Wait for my approval, then fix it, add or update a test that would have caught
it, run the gate, and report what you changed.
```

### Clean up lint warnings

```
Run cargo clippy --all-targets --all-features -- -D warnings and capture the
full output. Group the warnings into auto-fixable and manual. Show me a plan
with, for each warning, the file, line, issue and proposed fix. Wait for my
approval.

Then run cargo clippy --fix --allow-dirty, fix the rest by hand, and run the
gate again. Report the result.
```

### Areas that need extra care

When a change touches one of these, add the matching lines to the prompt.

**Themes and templates**
```
Read SPEC.md Section 3.5 first. Colours come only from the CSS custom
properties in assets/static/theme.css (var(--...)); no hard-coded colours in
templates. The theme comes from the taverna_theme cookie and is injected by
the Views extractor (ADR-027); <html> carries data-theme="{{ theme }}". Every
form must work without JavaScript. HTMX endpoints return fragments only.
```

**Federation**
```
Read SPEC.md Section 6 and the ADRs it cites first. The activitypub_federation
crate does HTTP signatures, actor fetching, WebFinger helpers and SSRF
protection: do not reimplement any of them or call reqwest directly. Build
outbound activities in federation::outbound and only enqueue them from
handlers (ADR-019, ADR-023). Every ActivityHandler::receive must be idempotent,
every Object::verify must call verify_domains_match, remote content goes only
into the remote_* tables (ADR-017), and a community that receives a Create
must Announce it to its followers.
```

**Database**
```
Add a new migration; never edit one that has shipped. Implement both up and
down, register it in migration/src/lib.rs, use UUID keys and TIMESTAMPTZ,
add the indexes the queries need, and regenerate the entities in
src/models/_entities/. Use keyset pagination, never OFFSET (ADR-018).
```

---

## Recovery prompts

Use these when an agent goes off track.

**It starts writing files before showing a plan:**
```
Stop. Before making any more changes, show me a complete plan: every file you
will create, and every file you will modify and what will change. Wait for my
approval before continuing.
```

**It hand-rolls HTTP signatures or uses rsa/sha2/base64 in federation code:**
```
Do not implement HTTP signatures or key handling yourself. The
activitypub_federation crate does this: outbound requests are signed when the
delivery worker sends them, inbound ones are verified by receive_activity(),
and keys come from generate_actor_keypair(). Remove the manual code and use
the crate's API.
```

**The Announce pattern is missing:**
```
When a community inbox receives Create for a post, the community actor must
also send Announce{Create} to all its followers. Without it, remote Lemmy
followers never see the post. Add it, queued through federation::outbound.
```

**The theme does not apply:**
```
Check that (1) base.html has data-theme="{{ theme }}" on <html>, (2) the theme
middleware is in the stack in src/app.rs, (3) the handler renders through the
Views extractor so the theme is in the context, and (4) theme.css is at
assets/static/theme.css and linked from base.html.
```

**It hard-codes colours:**
```
Do not hard-code colours in templates. Use the CSS custom properties defined in
assets/static/theme.css, such as var(--bg-primary), var(--text-primary) and
var(--accent). Replace the hex and rgb values.
```

**It leaves placeholders:**
```
[file] contains placeholder code (todo!, unimplemented!, or a stub that returns
Ok(()) without doing the work). All delivered code must be a real
implementation. Complete it now.
```

**It uses a generic branch or PR name:**
```
Rename the branch to <type>/<short-kebab-summary> describing this change, and
title the PR "<type>: <summary>", as AGENTS.md requires.
```
