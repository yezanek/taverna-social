---
description: Fix a specific issue reported by gemini-cli QA. Shows a plan before making changes.
arguments:
  - name: issue
    description: The issue description from gemini-cli (paste the relevant section)
    required: true
---

gemini-cli QA reported the following issue:

$issue

Before making any changes:

1. Read AGENTS.md and SPEC.md to understand what the correct behaviour should be.
2. Locate the relevant file(s).
3. Show me a plan: exactly what you will change and why.

Wait for my approval, then fix it.

After fixing:
- Run cargo check — must pass with zero errors
- Run cargo test — must pass (do not delete any tests)
- Confirm the specific issue is resolved

Report what you changed.
