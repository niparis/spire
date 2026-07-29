#!/usr/bin/env bash
set -euo pipefail

readonly release_tag="${1:?usage: promote-latest.sh <tag>}"
readonly previous_latest="$(gh release list --limit 100 --json tagName,isLatest --jq '.[] | select(.isLatest) | .tagName')"

is_draft="$(gh release view "${release_tag}" --json isDraft --jq '.isDraft')"
[[ "${is_draft}" == false ]] || {
  printf 'candidate release is still a draft: %s\n' "${release_tag}" >&2
  exit 1
}

gh release edit "${release_tag}" --latest

if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  printf 'previous_latest=%s\n' "${previous_latest}" >>"${GITHUB_OUTPUT}"
fi
printf 'promoted %s to latest; previous latest: %s\n' "${release_tag}" "${previous_latest:-none}"
