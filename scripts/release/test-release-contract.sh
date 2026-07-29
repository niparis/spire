#!/usr/bin/env bash
set -euo pipefail

readonly repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
readonly verifier="${repository_root}/scripts/release/verify-release.sh"
readonly installer="${repository_root}/install.sh"
readonly version="$(awk '
  /^\[workspace\.package\]$/ { in_workspace_package = 1; next }
  /^\[/ { in_workspace_package = 0 }
  in_workspace_package && /^version = "[0-9]+\.[0-9]+\.[0-9]+"$/ {
    value = $0
    sub(/^version = "/, "", value)
    sub(/"$/, "", value)
    print value
    exit
  }
' "${repository_root}/Cargo.toml")"
readonly tag="v${version}"
readonly target="release-contract-test"
readonly scratch="$(mktemp -d)"
trap 'rm -rf "${scratch}"' EXIT

make_archive() {
  local name="$1"
  local include_license="$2"
  local stage="${scratch}/stage-${name}"
  mkdir -p "${stage}"
  printf 'binary fixture\n' >"${stage}/spire"
  printf '%s\n' "${version}" >"${stage}/VERSION"
  if [[ "${include_license}" == true ]]; then
    printf 'license fixture\n' >"${stage}/LICENSE"
    tar -C "${stage}" -czf "${scratch}/${name}" LICENSE VERSION spire
  else
    tar -C "${stage}" -czf "${scratch}/${name}" VERSION spire
  fi
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${scratch}/${name}" | sed "s#${scratch}/##" >"${scratch}/SHA256SUMS"
  else
    shasum -a 256 "${scratch}/${name}" | sed "s#${scratch}/##" >"${scratch}/SHA256SUMS"
  fi
}

readonly archive_name="spire-${tag}-${target}.tar.gz"
make_archive "${archive_name}" true
"${verifier}" "${tag}" "${scratch}/${archive_name}" "${scratch}/SHA256SUMS" "${target}"

if "${verifier}" "v999.0.0" >/dev/null 2>&1; then
  printf 'mismatched tag unexpectedly passed validation\n' >&2
  exit 1
fi

readonly incomplete_target="${target}-incomplete"
readonly incomplete_archive="spire-${tag}-${incomplete_target}.tar.gz"
make_archive "${incomplete_archive}" false
if "${verifier}" \
  "${tag}" \
  "${scratch}/${incomplete_archive}" \
  "${scratch}/SHA256SUMS" \
  "${incomplete_target}" >/dev/null 2>&1; then
  printf 'incomplete archive unexpectedly passed validation\n' >&2
  exit 1
fi

printf 'release contract fixtures pass\n'

"${installer}" --help >/dev/null
"${installer}" --version "${tag}" --dry-run >/dev/null
if "${installer}" --version invalid --dry-run >/dev/null 2>&1; then
  printf 'invalid installer version unexpectedly passed validation\n' >&2
  exit 1
fi

printf 'installer contract fixtures pass\n'
