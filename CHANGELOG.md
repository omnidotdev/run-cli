# @omnidotdev/run-cli

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
