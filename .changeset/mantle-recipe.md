---
"@omnidotdev/run-cli": minor
---

Add Mantle to the catalog. `run up mantle` (or `omni run up mantle`) starts Postgres, the Mantle API, and the Mantle app behind the local proxy, and prompts for the OpenID Connect provider to sign in with (issuer, JWKS URL, client ID, client secret). Each user whose provider sends no Omni workspace claims gets a personal workspace.
