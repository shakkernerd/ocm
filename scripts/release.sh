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
Prepare or resume a signed ocm release from the current main branch.

Usage:
  scripts/release.sh <version> [--remote <name>] [--skip-checks]

Examples:
  scripts/release.sh 0.2.4
  scripts/release.sh 1.0.0-beta.1 --remote upstream
EOF
}

package_version() {
  perl -ne 'print "$1\n" if /^version = "([^"]+)"$/' Cargo.toml | head -n1
}

lockfile_version() {
  perl -0ne 'print "$1\n" if /\[\[package\]\]\nname = "ocm"\nversion = "([^"]+)"/s' Cargo.lock | head -n1
}

ref_commit() {
  git rev-list -n1 "$1" 2>/dev/null || true
}

remote_ref_commit() {
  git ls-remote "$remote" "$1" | awk 'NR==1 { print $1 }'
}

remote_tag_commit() {
  git ls-remote "$remote" "refs/tags/${tag}^{}" "refs/tags/${tag}" | awk '
    $2 ~ /\^\{\}$/ { print $1; found=1; exit }
    NR == 1 { first=$1 }
    END { if (!found && first != "") print first }
  '
}

refresh_dirty_files() {
  dirty_files=()
  while IFS= read -r file; do
    [[ -n "$file" ]] || continue
    dirty_files+=("$file")
  done < <(
    {
      git diff --name-only --ignore-submodules --
      git diff --cached --name-only --ignore-submodules --
    } | sort -u
  )
}

only_version_files_dirty() {
  local file
  [[ "${#dirty_files[@]}" -gt 0 ]] || return 1
  for file in "${dirty_files[@]}"; do
    case "$file" in
      Cargo.toml|Cargo.lock)
        ;;
      *)
        return 1
        ;;
    esac
  done
  return 0
}

current_head_subject() {
  git log -1 --pretty=%s 2>/dev/null || true
}

log_resume_state() {
  log_step "resume state: $*"
}

log_skip() {
  log_step "skip: $*"
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

if ! git remote get-url "$remote" >/dev/null 2>&1; then
  echo "error: git remote not found: $remote" >&2
  exit 1
fi

tag="v${version}"
release_commit_message="chore: bump version to ${version}"
current_version="$(package_version)"
current_lock_version="$(lockfile_version)"

if [[ -z "$current_version" || -z "$current_lock_version" ]]; then
  echo "error: could not read the ocm version from Cargo.toml and Cargo.lock" >&2
  exit 1
fi

if [[ "$current_version" != "$current_lock_version" ]]; then
  echo "error: Cargo.toml and Cargo.lock are out of sync; fix the version files before running scripts/release.sh" >&2
  exit 1
fi

refresh_dirty_files

head_sha="$(git rev-parse HEAD)"
head_subject="$(current_head_subject)"
head_is_release_commit=0
if [[ "$head_subject" == "$release_commit_message" ]]; then
  head_is_release_commit=1
fi

local_tag_commit_sha="$(ref_commit "$tag")"
remote_tag_commit_sha="$(remote_tag_commit)"
remote_main_sha="$(remote_ref_commit "refs/heads/main")"

if [[ -n "$local_tag_commit_sha" && "$local_tag_commit_sha" != "$head_sha" ]]; then
  echo "error: local tag ${tag} already exists and does not point at HEAD" >&2
  exit 1
fi

if [[ -n "$remote_tag_commit_sha" && "$remote_tag_commit_sha" != "$head_sha" ]]; then
  echo "error: remote tag ${tag} already exists on ${remote} and does not point at HEAD" >&2
  exit 1
fi

release_state="fresh"
need_update_version=0
need_checks=0
need_commit=0

log_step "Preparing release ${tag} from branch ${branch} using remote ${remote}"

if [[ "$current_version" == "$version" ]]; then
  if [[ "${#dirty_files[@]}" -gt 0 ]]; then
    if ! only_version_files_dirty; then
      echo "error: tracked changes are present; commit or stash them before running scripts/release.sh" >&2
      exit 1
    fi
    if [[ "$head_is_release_commit" -eq 1 || -n "$local_tag_commit_sha" || -n "$remote_tag_commit_sha" ]]; then
      echo "error: version files are dirty for ${version}, but a release commit or tag already exists; clean up the release state before retrying" >&2
      exit 1
    fi
    release_state="resume-version-files"
    need_checks=1
    need_commit=1
    log_resume_state "version files already updated to ${version}"
    log_resume_state "continuing from local checks"
  else
    if [[ "$head_is_release_commit" -ne 1 ]]; then
      echo "error: ocm is already on ${version}, but HEAD is not the expected release commit; clean up or finish that release state manually" >&2
      exit 1
    fi

    release_state="resume-release-commit"
    log_resume_state "release commit already exists at ${head_sha:0:7}"
    if [[ -n "$local_tag_commit_sha" ]]; then
      log_resume_state "local tag ${tag} already exists"
    fi
    if [[ -n "$remote_tag_commit_sha" ]]; then
      log_resume_state "remote tag ${tag} already exists on ${remote}"
    fi
    if [[ "$remote_main_sha" == "$head_sha" ]]; then
      log_resume_state "main is already pushed to ${remote}"
    fi
  fi
else
  if [[ "${#dirty_files[@]}" -gt 0 ]]; then
    echo "error: tracked changes are present; commit or stash them before running scripts/release.sh" >&2
    exit 1
  fi
  if [[ -n "$local_tag_commit_sha" || -n "$remote_tag_commit_sha" ]]; then
    echo "error: release tag ${tag} already exists, but version files are still on ${current_version}" >&2
    exit 1
  fi
  release_state="fresh"
  need_update_version=1
  need_checks=1
  need_commit=1
fi

if [[ "$skip_checks" -eq 0 ]]; then
  if [[ "$need_checks" -eq 1 ]]; then
    log_step "Local checks are enabled"
  else
    log_skip "local checks already passed before this resume point"
  fi
else
  log_step "Local checks are skipped"
  need_checks=0
fi

if [[ "$need_update_version" -eq 1 ]]; then
  run_step "Updating version files to ${version}" "${script_dir}/update-version.sh" "$version"
else
  log_skip "version files are already set to ${version}"
fi

if [[ "$need_checks" -eq 1 ]]; then
  run_step "Running cargo fmt --check" cargo fmt --check
  run_step "Building test binaries" cargo test --no-run
  run_step "Running test suite" cargo test
  run_step "Building release binary" cargo build --release
fi

if [[ "$need_commit" -eq 1 ]]; then
  run_step "Staging version files" git add Cargo.toml Cargo.lock
  if git diff --cached --quiet --ignore-submodules -- Cargo.toml Cargo.lock; then
    echo "error: no staged version changes remain for ${version}; cannot create the release commit" >&2
    exit 1
  fi
  run_step "Creating release commit" git commit -m "$release_commit_message"
  head_sha="$(git rev-parse HEAD)"
else
  log_skip "release commit already exists"
fi

if [[ -z "$local_tag_commit_sha" && -n "$remote_tag_commit_sha" ]]; then
  run_step "Fetching existing tag ${tag} from ${remote}" git fetch "$remote" "refs/tags/${tag}:refs/tags/${tag}"
  local_tag_commit_sha="$(ref_commit "$tag")"
fi

if [[ -z "$local_tag_commit_sha" ]]; then
  log_step "Creating signed tag ${tag}; git or GPG may prompt here"
  if ! git tag -s "$tag" -m "$tag"; then
    echo "error: failed to create signed tag ${tag}; make sure git signing is configured" >&2
    exit 1
  fi
  log_step "done: Creating signed tag ${tag}"
  local_tag_commit_sha="$(ref_commit "$tag")"
else
  log_skip "local tag ${tag} already exists"
fi

remote_main_sha="$(remote_ref_commit "refs/heads/main")"
remote_tag_commit_sha="$(remote_tag_commit)"

push_targets=()
if [[ "$remote_main_sha" != "$head_sha" ]]; then
  push_targets+=("main")
fi
if [[ "$remote_tag_commit_sha" != "$head_sha" ]]; then
  push_targets+=("$tag")
fi

if [[ "${#push_targets[@]}" -gt 0 ]]; then
  run_step "Pushing ${push_targets[*]} to ${remote}" git push "$remote" "${push_targets[@]}"
else
  log_skip "main and ${tag} are already pushed to ${remote}"
fi

cat <<EOF
Release prep complete for ${tag}.

Next:
  1. Open GitHub Releases
  2. Create or publish the release ${tag} from the existing tag
  3. The release workflow will build and upload the tarballs

Optional GitHub CLI:
  gh release create ${tag} --title ${tag} --generate-notes
EOF
