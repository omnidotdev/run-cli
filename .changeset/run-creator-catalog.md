---
"@omnidotdev/run-cli": minor
---

Add Creator to the catalog (`run up creator`). Self-hosts Creator in single-owner mode (`SELF_HOST_MODE=true`, anonymous API via `PROTECT_ROUTES=false`, no external Gatekeeper) with Postgres, the API, and the app behind a Caddy proxy. Optional local InvokeAI (`INVOKEAI_BASE_URL`, unset by default) enables offline GPU image generation.
