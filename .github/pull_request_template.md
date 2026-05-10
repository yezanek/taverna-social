## Summary

<!-- What does this PR do? One or two sentences. -->

## Type of change

- [ ] `feat` — new feature
- [ ] `fix` — bug fix
- [ ] `refactor` — code change that neither fixes a bug nor adds a feature
- [ ] `docs` — documentation only
- [ ] `test` — adding or updating tests
- [ ] `chore` — build, deps, CI changes
- [ ] `perf` — performance improvement

## Checklist

- [ ] `cargo fmt` applied
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo test` passes
- [ ] No `unwrap()` in production code (use `?` or `expect("reason")`)
- [ ] No `todo!()` or `unimplemented!()` left in production code
- [ ] No hardcoded hex colours in templates (use `var(--...)`)
- [ ] Federation changes use the `activitypub_federation` crate (no raw `reqwest`, no manual signatures)
- [ ] New ActivityPub inbox handlers are idempotent (check `ap_id` before inserting)
- [ ] Database migrations have both `up` and `down` implemented
- [ ] New migrations added to `migration/src/lib.rs`

## Federation impact

<!-- If this PR touches federation code, describe the ActivityPub behaviour change. -->
<!-- If not, write "N/A". -->

## Testing

<!-- How was this tested? Local dev server? Unit tests? Against a real Lemmy/Mastodon instance? -->
