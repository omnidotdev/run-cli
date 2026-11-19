---
"@omnidotdev/run-cli": minor
---

Recipes can show a message once, right after a stack's first successful `run up`, with a top-level `notes` field. Notes can reference the stack's generated secrets and URLs (for example `${BASE_URL}` or a one-time setup token) and use the same `${app.*}`, `${input.*}`, and `${secret.*}` tokens as services, so a recipe can tell the admin exactly what to do on first sign-in.
