#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: scripts/validate-version.sh <version>" >&2
}

if [[ $# -ne 1 ]]; then
  usage
  exit 1
fi

version="$1"
if [[ ! "$version" =~ ^[0-9A-Za-z.+-]+$ ]]; then
  echo "error: invalid semantic version: $version" >&2
  exit 1
fi

tmp_dir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

mkdir -p "${tmp_dir}/src"
cat >"${tmp_dir}/Cargo.toml" <<EOF
[package]
name = "ocm-version-check"
version = "${version}"
edition = "2024"
EOF
printf 'fn main() {}\n' >"${tmp_dir}/src/main.rs"

if ! cargo metadata \
  --format-version 1 \
  --no-deps \
  --manifest-path "${tmp_dir}/Cargo.toml" \
  >/dev/null 2>&1; then
  echo "error: invalid semantic version: $version" >&2
  exit 1
fi
