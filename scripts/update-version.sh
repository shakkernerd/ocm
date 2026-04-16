#!/usr/bin/env bash
set -euo pipefail

timestamp() {
  date '+%H:%M:%S'
}

log_step() {
  printf '[%s] %s\n' "$(timestamp)" "$*" >&2
}

run_step() {
  local description="$1"
  shift
  local started_at="$SECONDS"
  log_step "$description"
  "$@"
  log_step "done: ${description} ($((SECONDS - started_at))s)"
}

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

log_step "Updating Cargo.toml and Cargo.lock from ${current_version} to ${new_version}"

perl -0pi -e 's/^(version = ")[^"]+(")/$1.$ENV{OCM_NEW_VERSION}.$2/me' Cargo.toml

perl -0pi -e 's/(\[\[package\]\]\nname = "ocm"\nversion = ")[^"]+(")/$1.$ENV{OCM_NEW_VERSION}.$2/se' Cargo.lock

updated_toml_version="$(perl -ne 'print "$1\n" if /^version = "([^"]+)"$/' Cargo.toml | head -n1)"
updated_lock_version="$(perl -0ne 'print "$1\n" if /\[\[package\]\]\nname = "ocm"\nversion = "([^"]+)"/s' Cargo.lock | head -n1)"

if [[ "$updated_toml_version" != "$new_version" || "$updated_lock_version" != "$new_version" ]]; then
  echo "error: version files did not update cleanly" >&2
  exit 1
fi

run_step "Verifying the version bump with cargo check --locked" cargo check --locked --quiet

echo "Updated ocm version: ${current_version} -> ${new_version}"
