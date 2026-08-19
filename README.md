<div align="center">

  <h1 align="center">Run</h1>

[Website](https://run.omni.dev) | [Docs](https://docs.omni.dev/products/run) | [Discord](https://discord.gg/omnidotdev) | [X](https://x.com/omnidotdev)

</div>

**Run** is a CLI for self-hosting Omni products. One command deploys a full stack with automatic SSL, reverse proxy, secrets, and health checks.

## Installation

| Platform | Channel | Command / Link |
| --- | --- | --- |
| All | [GitHub Releases](https://github.com/omnidotdev/run-cli/releases) | Download from releases page |
| macOS / Linux | Shell | `curl -fsSL https://run.omni.dev/install.sh \| sh` |
| macOS / Linux | [Homebrew](https://github.com/omnidotdev/homebrew-tap/blob/master/Formula/run-cli.rb) | `brew install omnidotdev/tap/run-cli` |
| Arch Linux | [AUR](https://aur.archlinux.org/packages/omnidotdev-run-cli) / [AUR (bin)](https://aur.archlinux.org/packages/omnidotdev-run-cli-bin) | `paru -S omnidotdev-run-cli` or `paru -S omnidotdev-run-cli-bin` |

### Build from source

```bash
git clone https://github.com/omnidotdev/run-cli
cd run-cli
cargo build --release
# Binary will be at target/release/run
```

## Quick Start

```bash
# Deploy Runa
run up runa

# Preview what will be deployed
run plan runa

# Tail container logs
run logs runa

# Stop, preserve data
run down runa

# Stop and wipe all data
run down runa --fresh
```

## Commands

| Command | Description |
|---------|-------------|
| `run up <product>` | Start a product stack |
| `run down <product>` | Stop a product stack |
| `run ls` | List available products |
| `run logs <product>` | Tail container logs |
| `run plan <product>` | Preview deployment plan |

## Prerequisites

- [Docker](https://docs.docker.com/get-docker/) with the Compose plugin
- [mkcert](https://github.com/FiloSottile/mkcert) (optional, for trusted HTTPS)

## License

The code in this repository is licensed under Apache 2.0, &copy; [Omni LLC](https://omni.dev). See [LICENSE.md](LICENSE.md) for more information.
