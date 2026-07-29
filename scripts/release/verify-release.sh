#!/usr/bin/env bash
set -euo pipefail

readonly repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
readonly cargo_toml="${repository_root}/Cargo.toml"

usage() {
  printf 'usage: %s <tag> [archive checksum-manifest target-triple]\n' "${0##*/}" >&2
  exit 2
}

workspace_version() {
  awk '
    /^\[workspace\.package\]$/ { in_workspace_package = 1; next }
    /^\[/ { in_workspace_package = 0 }
    in_workspace_package && /^version = "[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z][0-9A-Za-z.-]*)?"$/ {
      value = $0
      sub(/^version = "/, "", value)
      sub(/"$/, "", value)
      print value
      exit
    }
  ' "${cargo_toml}"
}

sha256_digest() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

[[ $# -eq 1 || $# -eq 4 ]] || usage

readonly tag="$1"
readonly version="$(workspace_version)"
[[ -n "${version}" ]] || {
  printf 'workspace package version is missing from %s\n' "${cargo_toml}" >&2
  exit 1
}
[[ "${version}" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z][0-9A-Za-z.-]*)?$ ]] || {
  printf 'workspace version is not a release SemVer version: %s\n' "${version}" >&2
  exit 1
}
[[ "${tag}" == "v${version}" ]] || {
  printf 'release tag %s does not match workspace version %s\n' "${tag}" "${version}" >&2
  exit 1
}

for manifest in \
  "${repository_root}/crates/spire-domain/Cargo.toml" \
  "${repository_root}/crates/spire-application/Cargo.toml" \
  "${repository_root}/crates/spire-adapters/Cargo.toml" \
  "${repository_root}/crates/spire/Cargo.toml"; do
  grep -qx 'version.workspace = true' "${manifest}" || {
    printf 'crate must inherit the workspace version: %s\n' "${manifest}" >&2
    exit 1
  }
done

if [[ $# -eq 1 ]]; then
  printf 'release metadata is valid for %s\n' "${tag}"
  exit 0
fi

readonly archive="$2"
readonly checksums="$3"
readonly target="$4"
readonly archive_name="spire-${tag}-${target}.tar.gz"

[[ -f "${archive}" ]] || {
  printf 'release archive is missing: %s\n' "${archive}" >&2
  exit 1
}
[[ -f "${checksums}" ]] || {
  printf 'checksum manifest is missing: %s\n' "${checksums}" >&2
  exit 1
}
[[ "${archive##*/}" == "${archive_name}" ]] || {
  printf 'unexpected archive name: %s\n' "${archive##*/}" >&2
  exit 1
}

readonly expected_checksum="$(sha256_digest "${archive}")  ${archive_name}"
grep -Fqx "${expected_checksum}" "${checksums}" || {
  printf 'checksum manifest does not authenticate %s\n' "${archive_name}" >&2
  exit 1
}

readonly expected_members=$'LICENSE\nVERSION\nspire'
readonly archive_members="$(tar -tzf "${archive}" | LC_ALL=C sort)"
[[ "${archive_members}" == "${expected_members}" ]] || {
  printf 'release archive must contain exactly LICENSE, VERSION, and spire\n' >&2
  exit 1
}
[[ "$(tar -xOf "${archive}" VERSION)" == "${version}" ]] || {
  printf 'release archive VERSION does not match workspace version\n' >&2
  exit 1
}

printf 'release archive is valid for %s\n' "${tag}"
