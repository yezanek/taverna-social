---
description: Run cargo clippy, show a plan for all fixes, wait for approval, then clean up.
---

Run cargo clippy -- -D warnings and capture the full output.

Group the warnings:
- Auto-fixable (cargo clippy --fix can handle)
- Requires manual fix

Show me a plan: for each warning, the file, line, issue, and proposed fix.
Wait for my approval.

After approval:
1. Run: cargo clippy --fix --allow-dirty -- -D warnings  (auto-fixes)
2. Fix the manual ones
3. Run: cargo clippy -- -D warnings  (confirm zero warnings)
4. Run: cargo test  (confirm nothing broken)
5. Run: cargo fmt

Report the final result.
