#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Update the ocm package version safely.

Usage:
  scripts/update-version.sh <version>

Examples:
  scripts/update-version.sh 1.2.3
  scripts/update-version.sh 1.0.0-beta.1
EOF
}

if [[ $# -ne 1 ]]; then
  usage >&2
  exit 1
fi

new_version="$1"
if [[ ! "$new_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$ ]]; then
  echo "error: version must look like 1.2.3 or 1.2.3-beta.1" >&2
  exit 1
fi

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "${script_dir}/.." && pwd)"
cd "$repo_root"

current_version="$(perl -ne 'print "$1\n" if /^version = "([^"]+)"$/' Cargo.toml | head -n1)"
if [[ -z "$current_version" ]]; then
  echo "error: could not read the package version from Cargo.toml" >&2
  exit 1
fi

if [[ "$current_version" == "$new_version" ]]; then
  echo "ocm is already on ${new_version}"
  exit 0
fi

export OCM_NEW_VERSION="$new_version"

perl -0pi -e '
  s{
    (\[package\]\n(?:(?!^\[).*\n)*?version = ")
    [^"]+
    (")
  }{$1.$ENV{OCM_NEW_VERSION}.$2}msex
' Cargo.toml

perl -0pi -e '
  s{
    (\[\[package\]\]\nname = "ocm"\nversion = ")
    [^"]+
    (")
  }{$1.$ENV{OCM_NEW_VERSION}.$2}msex
' Cargo.lock

cargo check --locked --quiet

echo "Updated ocm version: ${current_version} -> ${new_version}"
