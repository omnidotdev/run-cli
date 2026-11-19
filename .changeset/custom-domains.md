---
"@omnidotdev/run-cli": minor
---

Serve a stack at your own domain with `run up <target> --domain admin.example.com` (or `RUN_DOMAIN`). The stack gets publicly trusted HTTPS with certificates obtained and renewed automatically. The app is served at `https://<domain>` and each other route at its own subdomain (e.g. `https://api.<domain>`), all on port 443, so it works from networks that only allow standard HTTPS. `run up` prints the DNS records to create: one per hostname, or the domain plus a `*.<domain>` wildcard. Every app URL (`BASE_URL`, `API_BASE_URL`, and the rest, plus `${app.fqdn}` in recipes) points at the domain. The domain is remembered per stack, so `run update` keeps it; pass `--domain localhost` to switch back. Local stacks behave exactly as before.
