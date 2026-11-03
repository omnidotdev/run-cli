---
"@omnidotdev/run-cli": minor
---

Extend `run deploy <target> --format json` with a `deploySpec`: the resolution a
cloud target must reproduce that the `fractal.yaml` drops as local-only. It lists
the auto-secrets to generate (name/method/length), the URL bindings (which
service owns a public URL and which env vars, e.g. `BASE_URL`/`AUTH_BASE_URL`,
bind to it, derived from the recipe's proxy routes), the post-deploy hooks, and
the local proxy service to skip. This lets run.omni.dev / Fractal deploy a
multi-service recipe the way `run up` resolves it, instead of forking that logic.
