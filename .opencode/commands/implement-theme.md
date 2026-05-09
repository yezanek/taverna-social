---
description: Implement the light/dark/sepia theme switcher (Phase 3, Tasks 3.5–3.9). Shows a plan first.
---

You are implementing the Taverna theme switcher.

Read SPEC.md Section 3.5 and PHASE_3.md Tasks 3.5–3.9 completely first.

Show me a plan covering these files before writing anything:
- src/initializers/theme_middleware.rs
- src/controllers/theme.rs
- assets/static/theme.css
- views/layouts/base.html (modifications)
- src/app.rs (route and middleware registration)
- tests/theme_test.rs

Wait for my approval, then implement in this order:

1. assets/static/theme.css
   - Three [data-theme] blocks with ALL custom properties from SPEC.md Section 3.5
   - Base styles using var(--...) properties
   - Theme switcher button styles

2. src/initializers/theme_middleware.rs
   - CurrentTheme(String) struct
   - Middleware reads taverna_theme cookie, validates, injects extension
   - Default to "light" if cookie is absent or invalid

3. src/controllers/theme.rs
   - POST /theme handler
   - Validates theme value (light/dark/sepia, default light)
   - Sets cookie: taverna_theme, Path=/, Max-Age=31536000, SameSite=Lax, NOT httpOnly
   - Redirects to Referer header or / if no Referer

4. views/layouts/base.html
   - <html data-theme="{{ theme }}">
   - <link rel="stylesheet" href="/static/theme.css">
   - Theme switcher form in nav (three submit buttons, POST /theme, no JS)

5. Register in app.rs
   - POST /theme route
   - Theme middleware in middleware stack (before all handlers)

6. tests/theme_test.rs
   - Test each valid theme sets correct cookie
   - Test invalid theme defaults to light
   - Test cookie Max-Age is 31536000
   - Test middleware injects correct CurrentTheme

After each file, run cargo check.
After all files, run cargo test.

Verify: no hardcoded hex colours in any .html files — only var(--...) properties.
