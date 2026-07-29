#!/usr/bin/env bash
set -euo pipefail

readonly repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
readonly verifier="${repository_root}/scripts/release/verify-release.sh"
readonly input_directory="${INPUT_DIR:?INPUT_DIR is required}"
readonly output_directory="${DIST_DIR:?DIST_DIR is required}"
readonly release_tag="${RELEASE_TAG:?RELEASE_TAG is required}"
readonly source_commit="${SOURCE_COMMIT:?SOURCE_COMMIT is required}"
readonly workflow_run_url="${WORKFLOW_RUN_URL:?WORKFLOW_RUN_URL is required}"
readonly supported_targets="${SUPPORTED_TARGETS:?SUPPORTED_TARGETS is required}"

sha256_digest() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

"${verifier}" "${release_tag}"
[[ -d "${input_directory}" ]] || {
  printf 'candidate input directory is missing: %s\n' "${input_directory}" >&2
  exit 1
}
if [[ -e "${output_directory}" ]] && [[ -n "$(find "${output_directory}" -mindepth 1 -maxdepth 1 -print -quit)" ]]; then
  printf 'release output directory must be empty: %s\n' "${output_directory}" >&2
  exit 1
fi
mkdir -p "${output_directory}"

declare -a expected_archives=()
for target in ${supported_targets}; do
  archive="spire-${release_tag}-${target}.tar.gz"
  expected_archives+=("${archive}")
  archive_path="$(find "${input_directory}" -type f -name "${archive}" -print -quit)"
  [[ -n "${archive_path}" ]] || {
    printf 'candidate is missing target archive: %s\n' "${archive}" >&2
    exit 1
  }
  cp "${archive_path}" "${output_directory}/${archive}"
done

actual_archives="$(find "${input_directory}" -type f -name "spire-${release_tag}-*.tar.gz" -exec basename {} \; | LC_ALL=C sort)"
expected_listing="$(printf '%s\n' "${expected_archives[@]}" | LC_ALL=C sort)"
[[ "${actual_archives}" == "${expected_listing}" ]] || {
  printf 'candidate contains missing, duplicate, or unexpected target archives\n' >&2
  exit 1
}

for archive in "${expected_archives[@]}"; do
  printf '%s  %s\n' "$(sha256_digest "${output_directory}/${archive}")" "${archive}"
done | LC_ALL=C sort >"${output_directory}/SHA256SUMS"

lockfile_sha256="$(sha256_digest "${repository_root}/Cargo.lock")"
rustc_version="$(rustc --version)"
{
  printf '{\n'
  printf '  "archives": [\n'
  index=0
  for target in ${supported_targets}; do
    archive="spire-${release_tag}-${target}.tar.gz"
    digest="$(sha256_digest "${output_directory}/${archive}")"
    [[ ${index} -eq 0 ]] || printf ',\n'
    printf '    {"name": "%s", "sha256": "%s", "target": "%s"}' "${archive}" "${digest}" "${target}"
    index=$((index + 1))
  done
  printf '\n  ],\n'
  printf '  "cargo_lock_sha256": "%s",\n' "${lockfile_sha256}"
  printf '  "release_tag": "%s",\n' "${release_tag}"
  printf '  "rustc": "%s",\n' "${rustc_version}"
  printf '  "source_commit": "%s",\n' "${source_commit}"
  printf '  "workflow_run_url": "%s"\n' "${workflow_run_url}"
  printf '}\n'
} >"${output_directory}/build-metadata.json"

for target in ${supported_targets}; do
  archive="spire-${release_tag}-${target}.tar.gz"
  "${verifier}" "${release_tag}" "${output_directory}/${archive}" "${output_directory}/SHA256SUMS" "${target}"
done

if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  {
    printf 'candidate_dir=%s\n' "${output_directory}"
    printf 'checksums=%s/SHA256SUMS\n' "${output_directory}"
    printf 'metadata=%s/build-metadata.json\n' "${output_directory}"
  } >>"${GITHUB_OUTPUT}"
fi
