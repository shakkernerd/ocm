#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: scripts/publish-release.sh --repo <owner/repo> --tag <tag> --asset-dir <dir>" >&2
}

repo=""
tag=""
asset_dir=""
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
    --asset-dir)
      shift
      [[ $# -gt 0 ]] || { echo "error: --asset-dir requires a value" >&2; exit 1; }
      asset_dir="$1"
      ;;
    *)
      echo "error: unknown argument: $1" >&2
      usage
      exit 1
      ;;
  esac
  shift
done

[[ -n "$repo" ]] || { usage; exit 1; }
[[ -n "$tag" ]] || { usage; exit 1; }
[[ -n "$asset_dir" ]] || { usage; exit 1; }

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
"${script_dir}/prepare-release-assets.sh" "$asset_dir" >/dev/null

if release_state="$(gh release view "$tag" --repo "$repo" --json isDraft --jq '.isDraft' 2>/dev/null)"; then
  if [[ "$release_state" != "true" ]]; then
    echo "error: release ${tag} is already public; refusing to replace published assets" >&2
    exit 1
  fi
else
  gh release create "$tag" \
    --repo "$repo" \
    --draft \
    --verify-tag \
    --title "$tag" \
    --generate-notes
fi

assets=(
  "${asset_dir}/ocm-aarch64-apple-darwin.tar.gz"
  "${asset_dir}/ocm-x86_64-apple-darwin.tar.gz"
  "${asset_dir}/ocm-x86_64-unknown-linux-gnu.tar.gz"
  "${asset_dir}/install.sh"
  "${asset_dir}/SHA256SUMS"
)
gh release upload "$tag" --repo "$repo" --clobber "${assets[@]}"

expected_assets="$(printf '%s\n' \
  "SHA256SUMS" \
  "ocm-aarch64-apple-darwin.tar.gz" \
  "ocm-x86_64-apple-darwin.tar.gz" \
  "ocm-x86_64-unknown-linux-gnu.tar.gz" \
  "install.sh" | sort)"
actual_assets="$(gh release view "$tag" --repo "$repo" --json assets --jq '.assets[].name' | sort)"
if [[ "$actual_assets" != "$expected_assets" ]]; then
  echo "error: draft release assets are incomplete; leaving ${tag} as a draft" >&2
  printf 'expected:\n%s\nactual:\n%s\n' "$expected_assets" "$actual_assets" >&2
  exit 1
fi

release_flags=(--draft=false)
if [[ "${tag#v}" == *-* ]]; then
  release_flags+=(--prerelease --latest=false)
else
  release_flags+=(--latest)
fi
gh release edit "$tag" --repo "$repo" "${release_flags[@]}"
