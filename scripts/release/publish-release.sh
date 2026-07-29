#!/usr/bin/env bash
set -euo pipefail

readonly candidate_directory="${CANDIDATE_DIR:?CANDIDATE_DIR is required}"
readonly release_tag="${RELEASE_TAG:?RELEASE_TAG is required}"
readonly assets=(
  "${candidate_directory}"/spire-"${release_tag}"-*.tar.gz
  "${candidate_directory}/SHA256SUMS"
  "${candidate_directory}/build-metadata.json"
  "${INSTALLER_PATH:?INSTALLER_PATH is required}"
)

verify_remote_assets() {
  local download_directory
  download_directory="$(mktemp -d)"
  trap 'rm -rf "${download_directory}"' RETURN

  gh release download "${release_tag}" --dir "${download_directory}" --pattern '*' --clobber

  local asset name local_size remote_size local_digest remote_digest
  for asset in "${assets[@]}"; do
    name="${asset##*/}"
    [[ -f "${download_directory}/${name}" ]] || {
      printf 'remote release is missing asset: %s\n' "${name}" >&2
      return 1
    }
    local_size="$(wc -c <"${asset}" | tr -d ' ')"
    remote_size="$(wc -c <"${download_directory}/${name}" | tr -d ' ')"
    [[ "${local_size}" == "${remote_size}" ]] || {
      printf 'remote asset size differs: %s\n' "${name}" >&2
      return 1
    }
    local_digest="$(shasum -a 256 "${asset}" | awk '{print $1}')"
    remote_digest="$(shasum -a 256 "${download_directory}/${name}" | awk '{print $1}')"
    [[ "${local_digest}" == "${remote_digest}" ]] || {
      printf 'remote asset digest differs: %s\n' "${name}" >&2
      return 1
    }
  done

  local remote_names expected_names
  remote_names="$(find "${download_directory}" -maxdepth 1 -type f -exec basename {} \; | LC_ALL=C sort)"
  expected_names="$(printf '%s\n' "${assets[@]##*/}" | LC_ALL=C sort)"
  [[ "${remote_names}" == "${expected_names}" ]] || {
    printf 'remote release has unexpected or missing assets\n' >&2
    return 1
  }
}

release_state="$(gh release view "${release_tag}" --json isDraft --jq '.isDraft' 2>/dev/null || true)"
if [[ -z "${release_state}" ]]; then
  gh release create "${release_tag}" --verify-tag --title "${release_tag}" --generate-notes --draft
  release_state=true
fi

if [[ "${release_state}" == false ]]; then
  verify_remote_assets
  printf 'published release %s is an identical verified no-op\n' "${release_tag}"
  exit 0
fi

for asset in "${assets[@]}"; do
  name="${asset##*/}"
  asset_directory="$(mktemp -d)"
  if gh release download "${release_tag}" --dir "${asset_directory}" --pattern "${name}" 2>/dev/null; then
    cmp -s "${asset}" "${asset_directory}/${name}" || {
      rm -rf "${asset_directory}"
      printf 'existing draft asset differs from candidate: %s\n' "${name}" >&2
      exit 1
    }
    rm -rf "${asset_directory}"
    continue
  fi
  rm -rf "${asset_directory}"
  gh release upload "${release_tag}" "${asset}"
done

verify_remote_assets
gh release edit "${release_tag}" --draft=false --latest=false
printf 'published verified release %s without changing latest\n' "${release_tag}"
