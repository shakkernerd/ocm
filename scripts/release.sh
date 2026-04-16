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
Prepare a signed ocm release from the current main branch.

Usage:
  scripts/release.sh <version> [--remote <name>] [--skip-checks]

Examples:
  scripts/release.sh 0.2.4
  scripts/release.sh 1.0.0-beta.1 --remote upstream
EOF
}

version=""
remote="origin"
skip_checks=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --remote)
      shift
      [[ $# -gt 0 ]] || { echo "error: --remote requires a value" >&2; exit 1; }
      remote="$1"
      ;;
    --skip-checks)
      skip_checks=1
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    -*)
      echo "error: unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
    *)
      if [[ -n "$version" ]]; then
        echo "error: version was already provided: $version" >&2
        usage >&2
        exit 1
      fi
      version="$1"
      ;;
  esac
  shift
done

if [[ -z "$version" ]]; then
  usage >&2
  exit 1
fi

if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$ ]]; then
  echo "error: version must look like 1.2.3 or 1.2.3-beta.1" >&2
  exit 1
fi

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "${script_dir}/.." && pwd)"
cd "$repo_root"

branch="$(git symbolic-ref --quiet --short HEAD || true)"
if [[ "$branch" != "main" ]]; then
  echo "error: releases must be prepared from the main branch (current: ${branch:-detached})" >&2
  exit 1
fi

if ! git diff --quiet --ignore-submodules -- || ! git diff --cached --quiet --ignore-submodules --; then
  echo "error: tracked changes are present; commit or stash them before running scripts/release.sh" >&2
  exit 1
fi

if ! git remote get-url "$remote" >/dev/null 2>&1; then
  echo "error: git remote not found: $remote" >&2
  exit 1
fi

tag="v${version}"

if git rev-parse --quiet --verify "refs/tags/${tag}" >/dev/null; then
  echo "error: local tag already exists: ${tag}" >&2
  exit 1
fi

if git ls-remote --exit-code --tags "$remote" "refs/tags/${tag}" >/dev/null 2>&1; then
  echo "error: remote tag already exists on ${remote}: ${tag}" >&2
  exit 1
fi

log_step "Preparing release ${tag} from branch ${branch} using remote ${remote}"
if [[ "$skip_checks" -eq 0 ]]; then
  log_step "Local checks are enabled"
else
  log_step "Local checks are skipped"
fi

run_step "Updating version files to ${version}" "${script_dir}/update-version.sh" "$version"

if [[ "$skip_checks" -eq 0 ]]; then
  run_step "Running cargo fmt --check" cargo fmt --check
  run_step "Running cargo test" cargo test
  run_step "Building release binary" cargo build --release
fi

run_step "Staging version files" git add Cargo.toml Cargo.lock
run_step "Creating release commit" git commit -m "chore: bump version to ${version}"

log_step "Creating signed tag ${tag}; git or GPG may prompt here"
if ! git tag -s "$tag" -m "$tag"; then
  echo "error: failed to create signed tag ${tag}; make sure git signing is configured" >&2
  exit 1
fi
log_step "done: Creating signed tag ${tag}"

run_step "Pushing main and ${tag} to ${remote}" git push "$remote" main "$tag"

cat <<EOF
Release prep complete for ${tag}.

Next:
  1. Open GitHub Releases
  2. Create or publish the release ${tag} from the existing tag
  3. The release workflow will build and upload the tarballs

Optional GitHub CLI:
  gh release create ${tag} --title ${tag} --generate-notes
EOF
