#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: scripts/verify-release-tag.sh --repo <owner/repo> --tag <tag> [--commit <sha>]" >&2
}

repo=""
tag=""
expected_commit=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo)
      shift
      [[ $# -gt 0 ]] || { echo "error: --repo requires a value" >&2; exit 1; }
      repo="$1"
      ;;
    --tag)
      shift
      [[ $# -gt 0 ]] || { echo "error: --tag requires a value" >&2; exit 1; }
      tag="$1"
      ;;
    --commit)
      shift
      [[ $# -gt 0 ]] || { echo "error: --commit requires a value" >&2; exit 1; }
      expected_commit="$1"
      ;;
    *)
      echo "error: unknown argument: $1" >&2
      usage
      exit 1
      ;;
  esac
  shift
done

[[ -n "$repo" && -n "$tag" ]] || { usage; exit 1; }
if [[ -n "$expected_commit" && ! "$expected_commit" =~ ^[0-9a-fA-F]{40}$ ]]; then
  echo "error: invalid release commit SHA: $expected_commit" >&2
  exit 1
fi

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "${script_dir}/.." && pwd)"

ref_data="$(
  gh api "repos/${repo}/git/ref/tags/${tag}" \
    --jq '[.object.type, .object.sha] | @tsv'
)"
IFS=$'\t' read -r ref_type tag_object_sha <<<"$ref_data"
if [[ "$ref_type" != "tag" || ! "$tag_object_sha" =~ ^[0-9a-fA-F]{40}$ ]]; then
  echo "error: release tag ${tag} must be an annotated tag" >&2
  exit 1
fi

tag_data="$(
  gh api "repos/${repo}/git/tags/${tag_object_sha}" \
    --jq '[.tag, .object.type, .object.sha, (.verification.verified | tostring)] | @tsv'
)"
IFS=$'\t' read -r verified_tag target_type target_sha signature_verified <<<"$tag_data"
if [[ "$verified_tag" != "$tag" || "$target_type" != "commit" ]]; then
  echo "error: release tag ${tag} does not point directly at a commit" >&2
  exit 1
fi
if [[ "$signature_verified" != "true" ]]; then
  echo "error: GitHub did not verify the signature for release tag ${tag}" >&2
  exit 1
fi
if [[ -n "$expected_commit" && "$target_sha" != "$expected_commit" ]]; then
  echo "error: release tag ${tag} moved away from verified commit ${expected_commit}" >&2
  exit 1
fi

default_branch="$(gh api "repos/${repo}" --jq '.default_branch')"
default_head="$(
  gh api "repos/${repo}/git/ref/heads/${default_branch}" --jq '.object.sha'
)"
compare_status="$(
  gh api "repos/${repo}/compare/${target_sha}...${default_head}" --jq '.status'
)"
if [[ "$compare_status" != "ahead" && "$compare_status" != "identical" ]]; then
  echo "error: release commit ${target_sha} is not on the protected ${default_branch} branch" >&2
  exit 1
fi

if ! git -C "$repo_root" cat-file -e "${target_sha}^{commit}" 2>/dev/null; then
  echo "error: verified release commit is not available locally: ${target_sha}" >&2
  exit 1
fi
package_version="$(
  git -C "$repo_root" show "${target_sha}:Cargo.toml" |
    perl -ne 'print "$1\n" if /^version = "([^"]+)"$/' |
    head -n1
)"
if [[ "$tag" != "v${package_version}" ]]; then
  echo "error: release tag ${tag} does not match package version ${package_version}" >&2
  exit 1
fi

echo "$target_sha"
