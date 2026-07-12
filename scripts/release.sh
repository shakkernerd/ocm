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

remote_tag_object() {
  git ls-remote "$remote" "refs/tags/${tag}" | awk 'NR == 1 { print $1 }'
}

tag_has_signature() {
  git cat-file -p "$1" | grep -Eq -- '-----BEGIN (PGP|SSH) SIGNATURE-----'
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

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "${script_dir}/.." && pwd)"
cd "$repo_root"

"${script_dir}/validate-version.sh" "$version"

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
starting_head="$(git rev-parse HEAD)"
starting_index_tree="$(git write-tree)"
current_version="$(package_version)"
current_lock_version="$(lockfile_version)"
created_release_commit=0
created_local_tag=0
transaction_complete=0
restore_version_files=0

rollback_release_prep() {
  local status=$?
  if [[ "$status" -eq 0 || "$transaction_complete" -eq 1 ]]; then
    return
  fi

  log_step "Rolling back local release preparation"
  if [[ "$created_local_tag" -eq 1 ]]; then
    git tag -d "$tag" >/dev/null 2>&1 || true
  fi
  if [[ "$created_release_commit" -eq 1 && "$(git rev-parse HEAD)" == "$head_sha" ]]; then
    git reset --mixed "$starting_head" >/dev/null
  fi
  if [[ "$restore_version_files" -eq 1 ]]; then
    git restore --source="$starting_head" --worktree -- Cargo.toml Cargo.lock
  fi
  git read-tree "$starting_index_tree"
}
trap rollback_release_prep EXIT

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
remote_tag_object_sha="$(remote_tag_object)"
remote_main_sha="$(remote_ref_commit "refs/heads/main")"

if [[ -n "$local_tag_commit_sha" && "$local_tag_commit_sha" != "$head_sha" ]]; then
  echo "error: local tag ${tag} already exists and does not point at HEAD" >&2
  exit 1
fi

if [[ -n "$remote_tag_commit_sha" && "$remote_tag_commit_sha" != "$head_sha" ]]; then
  echo "error: remote tag ${tag} already exists on ${remote} and does not point at HEAD" >&2
  exit 1
fi

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
    need_checks=1
    need_commit=1
    log_resume_state "version files already updated to ${version}"
    log_resume_state "continuing from local checks"
  else
    if [[ "$head_is_release_commit" -ne 1 ]]; then
      echo "error: ocm is already on ${version}, but HEAD is not the expected release commit; clean up or finish that release state manually" >&2
      exit 1
    fi

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
  restore_version_files=1
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
  created_release_commit=1
else
  log_skip "release commit already exists"
fi

if [[ -z "$local_tag_commit_sha" && -n "$remote_tag_commit_sha" ]]; then
  run_step "Fetching existing tag ${tag} from ${remote}" git fetch "$remote" "refs/tags/${tag}:refs/tags/${tag}"
  local_tag_commit_sha="$(ref_commit "$tag")"
fi

if [[ -z "$local_tag_commit_sha" ]]; then
  log_step "Creating signed tag ${tag}; git signing may prompt here"
  if ! git -c tag.gpgSign=true tag -a "$tag" -m "$tag"; then
    echo "error: failed to create signed tag ${tag}; make sure git tag signing is configured" >&2
    echo "hint: for SSH signing, set gpg.format=ssh and user.signingkey; for GPG signing, install and configure gpg" >&2
    exit 1
  fi
  if ! tag_has_signature "$tag"; then
    git tag -d "$tag" >/dev/null 2>&1 || true
    echo "error: created tag ${tag} was not signed; configure git tag signing before retrying" >&2
    exit 1
  fi
  log_step "done: Creating signed tag ${tag}"
  local_tag_commit_sha="$(ref_commit "$tag")"
  created_local_tag=1
else
  log_skip "local tag ${tag} already exists"
fi

if [[ "$(git cat-file -t "$tag" 2>/dev/null || true)" != "tag" ]]; then
  echo "error: release tag ${tag} must be an annotated tag" >&2
  exit 1
fi
if ! git verify-tag "$tag" >/dev/null 2>&1; then
  echo "error: release tag ${tag} does not have a valid configured signature" >&2
  exit 1
fi
local_tag_object_sha="$(git rev-parse "${tag}^{tag}")"

remote_main_sha="$(remote_ref_commit "refs/heads/main")"
remote_tag_commit_sha="$(remote_tag_commit)"
remote_tag_object_sha="$(remote_tag_object)"
if [[ -n "$remote_tag_object_sha" && "$remote_tag_object_sha" != "$local_tag_object_sha" ]]; then
  echo "error: remote tag ${tag} differs from the verified local signed tag; preserve both refs and resolve the release state manually" >&2
  exit 1
fi

push_targets=()
if [[ "$remote_main_sha" != "$head_sha" ]]; then
  push_targets+=("main")
fi
if [[ -z "$remote_tag_object_sha" ]]; then
  push_targets+=("$tag")
fi

if [[ "${#push_targets[@]}" -gt 0 ]]; then
  push_started_at="$SECONDS"
  log_step "Atomically pushing ${push_targets[*]} to ${remote}"
  if git push --atomic "$remote" "${push_targets[@]}"; then
    log_step "done: Atomically pushing ${push_targets[*]} to ${remote} ($((SECONDS - push_started_at))s)"
  else
    if ! pushed_main_sha="$(remote_ref_commit "refs/heads/main")" ||
      ! pushed_tag_sha="$(remote_tag_commit)" ||
      ! pushed_tag_object_sha="$(remote_tag_object)"; then
      transaction_complete=1
      trap - EXIT
      echo "error: push failed and remote state could not be determined; preserving the local release commit and tag for recovery" >&2
      exit 1
    fi
    if [[ "$pushed_main_sha" == "$head_sha" &&
      "$pushed_tag_sha" == "$head_sha" &&
      "$pushed_tag_object_sha" == "$local_tag_object_sha" ]]; then
      log_step "push reported failure, but ${remote} has both release refs at ${head_sha:0:7}"
    elif [[ "$pushed_main_sha" != "$remote_main_sha" ||
      "$pushed_tag_sha" != "$remote_tag_commit_sha" ||
      "$pushed_tag_object_sha" != "$remote_tag_object_sha" ]]; then
      transaction_complete=1
      trap - EXIT
      echo "error: push failed after remote release state changed; preserving the local release commit and tag for recovery" >&2
      exit 1
    else
      echo "error: atomic release push failed; remote refs were unchanged" >&2
      exit 1
    fi
  fi
else
  log_skip "main and ${tag} are already pushed to ${remote}"
fi

transaction_complete=1
trap - EXIT

cat <<EOF
Release prep complete for ${tag}.

Next:
  1. Run: gh api --method POST 'repos/{owner}/{repo}/dispatches' -f event_type=release -F 'client_payload[tag]=${tag}'
  2. The workflow will verify ${tag}, build every archive, and publish only after all assets and SHA256SUMS are uploaded
EOF
