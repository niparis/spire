#!/usr/bin/env bash
set -euo pipefail

readonly release_tag="${1:?usage: restore-latest.sh <prior-release-tag>}"
is_draft="$(gh release view "${release_tag}" --json isDraft --jq '.isDraft')"
[[ "${is_draft}" == false ]] || {
  printf 'cannot restore a draft as latest: %s\n' "${release_tag}" >&2
  exit 1
}
gh release edit "${release_tag}" --latest
printf 'restored %s as latest without changing release assets or tags\n' "${release_tag}"
