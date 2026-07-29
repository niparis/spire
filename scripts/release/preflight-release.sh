#!/usr/bin/env bash
set -euo pipefail

readonly repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
readonly verifier="${repository_root}/scripts/release/verify-release.sh"

usage() {
  printf 'usage: %s <tag> [source-sha]\n' "${0##*/}" >&2
  exit 2
}

[[ $# -eq 1 || $# -eq 2 ]] || usage
readonly tag="$1"
source_sha="${2:-}"

"${verifier}" "${tag}"

if git -C "${repository_root}" rev-parse -q --verify "refs/tags/${tag}" >/dev/null; then
  printf 'release tag already exists locally: %s\n' "${tag}" >&2
  exit 1
fi

if [[ -n "${source_sha}" ]]; then
  git -C "${repository_root}" cat-file -e "${source_sha}^{commit}"
fi

if [[ "${SPIRE_PREFLIGHT_REMOTE:-0}" == 1 ]]; then
  command -v gh >/dev/null 2>&1 || {
    printf 'gh is required for remote release preflight\n' >&2
    exit 1
  }
  if git -C "${repository_root}" ls-remote --exit-code --tags origin "refs/tags/${tag}" >/dev/null 2>&1; then
    printf 'release tag already exists on origin: %s\n' "${tag}" >&2
    exit 1
  fi
  if gh release view "${tag}" >/dev/null 2>&1; then
    printf 'GitHub release already exists: %s\n' "${tag}" >&2
    exit 1
  fi
fi

printf 'release preflight passed for %s\n' "${tag}"
