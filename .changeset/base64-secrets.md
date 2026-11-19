---
"@omnidotdev/run-cli": minor
---

Recipes can generate base64 secrets with `method = "base64"` in `[secrets]`, producing `length` random bytes as standard base64. Use it for apps that expect a base64-encoded key of an exact size, such as a 32-byte encryption key. `run deploy --format json` reports these secrets with the method `base64`.
