<div align="center">

  <h1 align="center">Run</h1>

[Website](https://run.omni.dev) | [Docs](https://docs.omni.dev/products/run) | [Discord](https://discord.gg/omnidotdev) | [X](https://x.com/omnidotdev) | [Threads](https://www.threads.com/@omnidotdev)

</div>

**Run** is a CLI for self-hosting anything on your own machine, Omni products and third-party apps alike. One command brings up a full stack with automatic SSL, reverse proxy, secrets, and health checks.

Full documentation lives at [docs.omni.dev/products/run](https://docs.omni.dev/products/run).

## Installation

| Platform | Channel | Command / Link |
| --- | --- | --- |
| All | [GitHub Releases](https://github.com/omnidotdev/run-cli/releases) | Download from the releases page |
| macOS / Linux | Shell | `curl -fsSL https://run.omni.dev/install.sh \| sh` |
| macOS / Linux | [Homebrew](https://github.com/omnidotdev/homebrew-tap/blob/master/Formula/run-cli.rb) | `brew install omnidotdev/tap/run-cli` |
| Arch Linux | [AUR](https://aur.archlinux.org/packages/omnidotdev-run-cli) / [AUR (bin)](https://aur.archlinux.org/packages/omnidotdev-run-cli-bin) | `paru -S omnidotdev-run-cli` or `paru -S omnidotdev-run-cli-bin` |
| Any | Source | `cargo build --release` (binary at `target/release/run`) |

## Prerequisites

- [Docker](https://docs.docker.com/get-docker/) with the Compose plugin
- [mkcert](https://github.com/FiloSottile/mkcert), optional, for trusted local HTTPS (without it, Run serves over HTTP)

## Quick start

```bash
# a catalog app (Omni or third-party)
run up runa
run up uptime-kuma

# or bring your own: a compose project, a recipe, or any image
run up .                     # a directory holding a compose file or run.toml
run up ./docker-compose.yml  # an explicit compose file
run up nginx                 # a bare image reference
```

Each command takes a **target**: a catalog name, a path to a recipe (`run.toml` / `run.yaml`) or a docker-compose file (or a directory holding one), or a bare image reference.

## Commands

| Command | Description |
| --- | --- |
| `run up <target>` | Start a stack (`run <target>` is shorthand) |
| `run down <target>` | Stop a stack; `--fresh` wipes data (auto-snapshots first unless `--no-backup`) |
| `run ls` | List catalog apps |
| `run plan <target>` | Preview what would start, without changes |
| `run logs <target> [service]` | Tail container logs |
| `run backup <target>` / `run backups <target>` | Snapshot volumes / list snapshots |
| `run restore <target> <snapshot>` | Restore volumes from a snapshot |
| `run status [target]` | Show a stack's services, or list every stack |
| `run update <target>` | Pull the latest images and recreate; auto-rollback on failure |
| `run deploy <target>` | Emit a `fractal.yaml` to deploy the stack to Fractal cloud |

Diagnose a stack with `run plan <target>` (a dry-run preview) and `run logs <target>`. Recipes, magic tokens, typed inputs, backups, and the built-in catalog are documented at [docs.omni.dev/products/run](https://docs.omni.dev/products/run).

## Development

```bash
cargo build                                # build
cargo test --workspace                     # run the tests
cargo clippy --all-targets -- -D warnings  # lint
cargo fmt --all                            # format
cargo run -p run-cli -- plan runa          # run the CLI locally
```

The crate version is driven by `package.json` (Changesets); `scripts/syncVersion.ts` propagates it to the Cargo manifests, so edit the version through a changeset rather than by hand. A pre-commit hook runs fmt, clippy, and the tests.

## License

The code in this repository is licensed under Apache 2.0, &copy; [Omni LLC](https://omni.dev). See [LICENSE.md](LICENSE.md) for more information.
