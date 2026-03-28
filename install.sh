#!/usr/bin/env bash
set -euo pipefail

REPO="shakkernerd/ocm"
DEFAULT_PREFIX="${HOME}/.local"

usage() {
  cat <<'EOF'
Install ocm from GitHub release binaries.

Usage:
  install.sh [--version <tag>] [--prefix <dir>] [--bin-dir <dir>] [--force]

Options:
  --version <tag>  Release tag to install. Defaults to the latest release.
  --prefix <dir>   Base install prefix. Defaults to ~/.local.
  --bin-dir <dir>  Install directory for the ocm binary. Defaults to <prefix>/bin.
  --force          Overwrite an existing ocm binary.
  --help           Show this help.
EOF
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: required command not found: $1" >&2
    exit 1
  fi
}

detect_target() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Darwin) os="apple-darwin" ;;
    Linux) os="unknown-linux-gnu" ;;
    *)
      echo "error: unsupported operating system: $os" >&2
      exit 1
      ;;
  esac

  case "$arch" in
    x86_64|amd64) arch="x86_64" ;;
    arm64|aarch64) arch="aarch64" ;;
    *)
      echo "error: unsupported architecture: $arch" >&2
      exit 1
      ;;
  esac

  printf '%s-%s\n' "$arch" "$os"
}

download_url_for() {
  local version="$1"
  local asset="$2"
  if [[ "$version" == "latest" ]]; then
    printf 'https://github.com/%s/releases/latest/download/%s\n' "$REPO" "$asset"
  else
    printf 'https://github.com/%s/releases/download/%s/%s\n' "$REPO" "$version" "$asset"
  fi
}

version="latest"
prefix="$DEFAULT_PREFIX"
bin_dir=""
force="false"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      shift
      [[ $# -gt 0 ]] || { echo "error: --version requires a value" >&2; exit 1; }
      version="$1"
      ;;
    --prefix)
      shift
      [[ $# -gt 0 ]] || { echo "error: --prefix requires a value" >&2; exit 1; }
      prefix="$1"
      ;;
    --bin-dir)
      shift
      [[ $# -gt 0 ]] || { echo "error: --bin-dir requires a value" >&2; exit 1; }
      bin_dir="$1"
      ;;
    --force)
      force="true"
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
  shift
done

require_command curl
require_command tar
require_command mktemp
require_command uname

if [[ -z "$bin_dir" ]]; then
  bin_dir="${prefix}/bin"
fi

target="$(detect_target)"
asset="ocm-${target}.tar.gz"
url="$(download_url_for "$version" "$asset")"

tmp_dir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

archive_path="${tmp_dir}/${asset}"

echo "Downloading ${url}"
curl -fsSL "$url" -o "$archive_path"
tar -xzf "$archive_path" -C "$tmp_dir"

binary_path="${tmp_dir}/ocm"
if [[ ! -x "$binary_path" ]]; then
  echo "error: release archive did not contain an executable ocm binary" >&2
  exit 1
fi

mkdir -p "$bin_dir"
destination="${bin_dir}/ocm"
if [[ -e "$destination" && "$force" != "true" ]]; then
  echo "error: ${destination} already exists; rerun with --force to replace it" >&2
  exit 1
fi

install -m 0755 "$binary_path" "$destination"

echo "Installed ocm to ${destination}"
case ":${PATH}:" in
  *":${bin_dir}:"*) ;;
  *)
    echo "Add ${bin_dir} to your PATH to run ocm from any shell."
    ;;
esac
