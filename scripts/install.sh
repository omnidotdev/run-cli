#!/bin/sh
# Install script for the Run CLI
# Usage: curl -fsSL https://run.omni.dev/install.sh | sh

set -eu

REPO="omnidotdev/run-cli"
BINARY="run"

main() {
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Linux)  target_os="unknown-linux-gnu" ;;
    Darwin) target_os="apple-darwin" ;;
    *)
      echo "Error: unsupported OS: $os" >&2
      exit 1
      ;;
  esac

  case "$arch" in
    x86_64|amd64)  target_arch="x86_64" ;;
    aarch64|arm64) target_arch="aarch64" ;;
    *)
      echo "Error: unsupported architecture: $arch" >&2
      exit 1
      ;;
  esac

  target="${target_arch}-${target_os}"

  # Determine install directory
  if [ "$(id -u)" = "0" ]; then
    install_dir="/usr/local/bin"
  else
    install_dir="${HOME}/.local/bin"
  fi

  # Fetch latest version tag
  latest_url="https://github.com/${REPO}/releases/latest"
  version="$(curl -fsSL -o /dev/null -w '%{url_effective}' "$latest_url" | rev | cut -d'/' -f1 | rev)"

  if [ -z "$version" ]; then
    echo "Error: could not determine latest version" >&2
    exit 1
  fi

  download_url="https://github.com/${REPO}/releases/download/${version}/${BINARY}-${target}.tar.gz"

  echo "Installing ${BINARY} ${version} for ${target}..."

  # Create temp directory for download
  tmp_dir="$(mktemp -d)"
  trap 'rm -rf "$tmp_dir"' EXIT

  # Download and extract
  curl -fsSL "$download_url" -o "${tmp_dir}/${BINARY}.tar.gz"
  tar -xzf "${tmp_dir}/${BINARY}.tar.gz" -C "$tmp_dir"

  # Install
  mkdir -p "$install_dir"
  mv "${tmp_dir}/${BINARY}" "${install_dir}/${BINARY}"
  chmod +x "${install_dir}/${BINARY}"

  echo ""
  echo "Installed ${BINARY} to ${install_dir}/${BINARY}"

  # Check if install dir is in PATH
  case ":${PATH}:" in
    *":${install_dir}:"*) ;;
    *)
      echo ""
      echo "Warning: ${install_dir} is not in your PATH"
      echo "Add it with: export PATH=\"${install_dir}:\$PATH\""
      ;;
  esac

  echo ""
  echo "Get started:"
  echo "  ${BINARY} --help         Show available commands"
  echo "  ${BINARY} ls             List available products"
  echo "  ${BINARY} up <product>   Start a product stack"
}

main
