---
"@omnidotdev/run-cli": minor
---

Runa and Vortex now keep TLS certificate checks on whenever they can: their services only skip verification for local stacks using mkcert certificates, never at a custom domain or on a cloud deploy. Recipes can do the same with the new `${tls.reject_unauthorized}` token, which is `0` under local mkcert certificates and `1` everywhere else.
