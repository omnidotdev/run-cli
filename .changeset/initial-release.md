---
"@omnidotdev/run-cli": minor
---

Initial release of the Run CLI: self-hosting made simple

- `run up <product>` deploys a full stack with one command (secrets, TLS, proxy, health checks, OAuth provisioning)
- `run down <product>` stops a stack, `--fresh` wipes all data
- `run plan <product>` previews deployment without making changes
- `run logs <product>` tails container logs
- `run ls` lists available products
- Ships with Runa as the first product in the catalog
- Automatic mkcert TLS with HTTP fallback
- Caddy reverse proxy with per-service routing
- curl-pipe installer: `curl -fsSL https://run.omni.dev/install.sh | sh`
