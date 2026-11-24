# @omnidotdev/run-cli

## 0.2.0

### Minor Changes

- [#40](https://github.com/omnidotdev/run-cli/pull/40) [`53c1cf6`](https://github.com/omnidotdev/run-cli/commit/53c1cf6eb724a870e26a54cb338eafbe0bec4046) Thanks [@coopbri](https://github.com/coopbri)! - Recipes can generate base64 secrets with `method = "base64"` in `[secrets]`, producing `length` random bytes as standard base64. Use it for apps that expect a base64-encoded key of an exact size, such as a 32-byte encryption key. `run deploy --format json` reports these secrets with the method `base64`.

- [#40](https://github.com/omnidotdev/run-cli/pull/40) [`53c1cf6`](https://github.com/omnidotdev/run-cli/commit/53c1cf6eb724a870e26a54cb338eafbe0bec4046) Thanks [@coopbri](https://github.com/coopbri)! - Serve a stack at your own domain with `run up <target> --domain admin.example.com` (or `RUN_DOMAIN`). The stack gets publicly trusted HTTPS with certificates obtained and renewed automatically. The app is served at `https://<domain>` and each other route at its own subdomain (e.g. `https://api.<domain>`), all on port 443, so it works from networks that only allow standard HTTPS. `run up` prints the DNS records to create: one per hostname, or the domain plus a `*.<domain>` wildcard. Every app URL (`BASE_URL`, `API_BASE_URL`, and the rest, plus `${app.fqdn}` in recipes) points at the domain. The domain is remembered per stack, so `run update` keeps it; pass `--domain localhost` to switch back. Local stacks behave exactly as before.

- [#41](https://github.com/omnidotdev/run-cli/pull/41) [`3f27017`](https://github.com/omnidotdev/run-cli/commit/3f27017b6e8efc9715e55f86f6d0fe5db033770e) Thanks [@coopbri](https://github.com/coopbri)! - Add Mantle to the catalog. `run up mantle` (or `omni run up mantle`) starts Postgres, the Mantle API, and the Mantle app behind the local proxy, and prompts for the OpenID Connect provider to sign in with (issuer, JWKS URL, client ID, client secret). Each user whose provider sends no Omni workspace claims gets a personal workspace.

- [#40](https://github.com/omnidotdev/run-cli/pull/40) [`53c1cf6`](https://github.com/omnidotdev/run-cli/commit/53c1cf6eb724a870e26a54cb338eafbe0bec4046) Thanks [@coopbri](https://github.com/coopbri)! - Typed inputs are now remembered per stack. A value you set through `RUN_INPUT_<KEY>` or answer at the prompt is saved with the stack, so `run update`, a later `run up`, and the dashboard reuse it instead of prompting again or falling back to the default. Set `RUN_INPUT_<KEY>` again to change it. The stack's generated settings file, which holds these values next to its secrets, is now readable by its owner only.

- [#40](https://github.com/omnidotdev/run-cli/pull/40) [`53c1cf6`](https://github.com/omnidotdev/run-cli/commit/53c1cf6eb724a870e26a54cb338eafbe0bec4046) Thanks [@coopbri](https://github.com/coopbri)! - Recipes can show a message once, right after a stack's first successful `run up`, with a top-level `notes` field. Notes can reference the stack's generated secrets and URLs (for example `${BASE_URL}` or a one-time setup token) and use the same `${app.*}`, `${input.*}`, and `${secret.*}` tokens as services, so a recipe can tell the admin exactly what to do on first sign-in.

- [#31](https://github.com/omnidotdev/run-cli/pull/31) [`badf120`](https://github.com/omnidotdev/run-cli/commit/badf12045942009cd0e1f8fede81db035e046873) Thanks [@coopbri](https://github.com/coopbri)! - Add `run deploy <target> --format json`. Alongside the default `yaml` (the Fractal manifest), the `json` format emits the manifest plus reduced recipe metadata (display name, description, services, typed inputs, and whether the stack generates secrets or runs a reverse proxy) so programmatic consumers, such as the deploy experience on run.omni.dev, can render a review without re-parsing the recipe.

- [#32](https://github.com/omnidotdev/run-cli/pull/32) [`2dfe301`](https://github.com/omnidotdev/run-cli/commit/2dfe301d61a496e72ae7749009ccdb15283ffd9b) Thanks [@coopbri](https://github.com/coopbri)! - Extend `run deploy <target> --format json` with a `deploySpec`: the resolution a
  cloud target must reproduce that the `fractal.yaml` drops as local-only. It lists
  the auto-secrets to generate (name/method/length), the URL bindings (which
  service owns a public URL and which env vars, e.g. `BASE_URL`/`AUTH_BASE_URL`,
  bind to it, derived from the recipe's proxy routes), the post-deploy hooks, and
  the local proxy service to skip. This lets run.omni.dev / Fractal deploy a
  multi-service recipe the way `run up` resolves it, instead of forking that logic.

- [#23](https://github.com/omnidotdev/run-cli/pull/23) [`95bf22e`](https://github.com/omnidotdev/run-cli/commit/95bf22e94cb500b6f5dfc4c8b7cb12d119329528) Thanks [@coopbri](https://github.com/coopbri)! - Make `run up vortex` work out of the box. The Vortex catalog entry now enables Vortex's single-owner self-host mode (`SELF_HOST_MODE`) instead of pointing at an external Gatekeeper and Warden, so a self-hoster gets a working instance from one command with no external identity or authorization services to wire up. Built-in event schemas and workflows seed under the owner's organization.

- [#40](https://github.com/omnidotdev/run-cli/pull/40) [`53c1cf6`](https://github.com/omnidotdev/run-cli/commit/53c1cf6eb724a870e26a54cb338eafbe0bec4046) Thanks [@coopbri](https://github.com/coopbri)! - Runa and Vortex now keep TLS certificate checks on whenever they can: their services only skip verification for local stacks using mkcert certificates, never at a custom domain or on a cloud deploy. Recipes can do the same with the new `${tls.reject_unauthorized}` token, which is `0` under local mkcert certificates and `1` everywhere else.

## 0.1.1

### Patch Changes

- [`caa3e85`](https://github.com/omnidotdev/run-cli/commit/caa3e851a93b6701119dee1107395567eff15043) Thanks [@coopbri](https://github.com/coopbri)! - Generate secrets from the operating system's random source (`getrandom`) instead of the `rand` crate, removing a low-severity `rand` soundness advisory. Secret values are unchanged.

## 0.1.0

### Minor Changes

- [`315d63a`](https://github.com/omnidotdev/run-cli/commit/315d63a962ec6c92b242aa385388f6f595ee12bb) Thanks [@coopbri](https://github.com/coopbri)! - Initial release of the Run CLI: self-host anything with one command.

  - Universal target resolver: `run up <target>` resolves a catalog name, a Run recipe (`run.toml`/`run.yaml`), a Docker Compose file or directory, or a bare image into one working stack
  - Automatic local TLS via mkcert (with HTTP fallback), a Caddy reverse proxy, and generated secrets
  - Magic tokens (`${secret.*}`, `${input.*}`, `${app.*}`) and typed `[[inputs]]` prompts
  - Backups and restore: `run backup` / `run restore` snapshot and restore data volumes; `run down --fresh` auto-snapshots before wiping
  - Safe updates: `run update` pulls the latest images, recreates, health-gates, and automatically rolls back (restoring data) if the new release is unhealthy
  - `run status` shows a stack's services or lists every known stack; `run plan` previews and `run logs` tails
  - `run deploy` emits a `fractal.yaml` to deploy the same stack to Fractal cloud, plus a one-click "Deploy to Fractal" button for any repository
  - Starter catalog: Runa, Vortex, Uptime Kuma, Pi-hole, and Backfeed
  - Install with `curl -fsSL https://run.omni.dev/install.sh | sh`, Homebrew, or the AUR
