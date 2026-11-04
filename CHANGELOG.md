# @omnidotdev/run-cli

## 0.2.0

### Minor Changes

- [#31](https://github.com/omnidotdev/run-cli/pull/31) [`badf120`](https://github.com/omnidotdev/run-cli/commit/badf12045942009cd0e1f8fede81db035e046873) Thanks [@coopbri](https://github.com/coopbri)! - Add `run deploy <target> --format json`. Alongside the default `yaml` (the Fractal manifest), the `json` format emits the manifest plus reduced recipe metadata (display name, description, services, typed inputs, and whether the stack generates secrets or runs a reverse proxy) so programmatic consumers, such as the deploy experience on run.omni.dev, can render a review without re-parsing the recipe.

- [#32](https://github.com/omnidotdev/run-cli/pull/32) [`2dfe301`](https://github.com/omnidotdev/run-cli/commit/2dfe301d61a496e72ae7749009ccdb15283ffd9b) Thanks [@coopbri](https://github.com/coopbri)! - Extend `run deploy <target> --format json` with a `deploySpec`: the resolution a
  cloud target must reproduce that the `fractal.yaml` drops as local-only. It lists
  the auto-secrets to generate (name/method/length), the URL bindings (which
  service owns a public URL and which env vars, e.g. `BASE_URL`/`AUTH_BASE_URL`,
  bind to it, derived from the recipe's proxy routes), the post-deploy hooks, and
  the local proxy service to skip. This lets run.omni.dev / Fractal deploy a
  multi-service recipe the way `run up` resolves it, instead of forking that logic.

- [#23](https://github.com/omnidotdev/run-cli/pull/23) [`95bf22e`](https://github.com/omnidotdev/run-cli/commit/95bf22e94cb500b6f5dfc4c8b7cb12d119329528) Thanks [@coopbri](https://github.com/coopbri)! - Make `run up vortex` work out of the box. The Vortex catalog entry now enables Vortex's single-owner self-host mode (`SELF_HOST_MODE`) instead of pointing at an external Gatekeeper and Warden, so a self-hoster gets a working instance from one command with no external identity or authorization services to wire up. Built-in event schemas and workflows seed under the owner's organization.

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
