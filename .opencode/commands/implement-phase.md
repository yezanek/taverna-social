---
description: Implement a Taverna phase from scratch. Shows a plan first, waits for approval, then builds.
arguments:
  - name: phase
    description: Phase number to implement (1, 2, 3, 4, or 5)
    required: true
---

You are implementing Phase $phase of the Taverna project.

Step 1 — Read these files completely before writing any code:
1. AGENTS.md — coding rules, project layout, dependency list
2. SPEC.md — the authoritative specification
3. PHASE_$phase.md — the tasks and deliverables for this phase

Step 2 — Show me a complete implementation plan:
- Every file you will CREATE, with a one-line description of what goes in it
- Every file you will MODIFY, with a one-line description of what changes
- Any shell commands you will run (migrations, code generation)

Do not write any code or run any commands yet. Wait for my approval.

Step 3 — After I approve the plan, implement each task in order.
After each task, run: cargo check
If cargo check fails, fix the errors before moving to the next task.

Step 4 — When all tasks are complete:
Run: cargo test
Fix any failing tests (do not delete tests — fix the code).
Run: cargo clippy -- -D warnings
Fix any warnings.
Run: cargo fmt

Step 5 — Confirm every item in the PHASE_$phase.md deliverables checklist.
Report any missing items.
