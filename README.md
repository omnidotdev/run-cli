# Run

Self-hosting made simple.

A CLI for deploying Omni products with one command — automatic SSL, reverse proxy, secrets, and health checks included.

## Installation

### Shell (macOS/Linux)

```sh
curl -fsSL https://run.omni.dev/install.sh | sh
```

### Homebrew

```sh
brew install omnidotdev/tap/run-cli
```

### AUR (Arch Linux)

```sh
paru -S omnidotdev-run-cli-bin
```

### From source

```sh
cargo install --git https://github.com/omnidotdev/run-cli
```

## Quick Start

```sh
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

Apache 2.0 — see [LICENSE.md](LICENSE.md)
