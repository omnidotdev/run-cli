---
"@omnidotdev/run-cli": minor
---

Add `run deploy <target> --format json`. Alongside the default `yaml` (the Fractal manifest), the `json` format emits the manifest plus reduced recipe metadata (display name, description, services, typed inputs, and whether the stack generates secrets or runs a reverse proxy) so programmatic consumers, such as the deploy experience on run.omni.dev, can render a review without re-parsing the recipe.
