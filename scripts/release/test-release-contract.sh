#!/usr/bin/env bash
set -euo pipefail

readonly repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
readonly verifier="${repository_root}/scripts/release/verify-release.sh"
readonly installer="${repository_root}/install.sh"
readonly version="$(awk '
  /^\[workspace\.package\]$/ { in_workspace_package = 1; next }
  /^\[/ { in_workspace_package = 0 }
  in_workspace_package && /^version = "[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z][0-9A-Za-z.-]*)?"$/ {
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
  printf '#!/usr/bin/env sh\nprintf "spire %s\\n" "%s"\n' "${version}" "${version}" >"${stage}/spire"
  chmod +x "${stage}/spire"
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

make_fake_uname() {
  local os="$1"
  local architecture="$2"
  local destination="$3"
  printf '#!/bin/sh\ncase "$1" in\n  -s) printf "%%s\\n" "%s" ;;\n  -m) printf "%%s\\n" "%s" ;;\nesac\n' \
    "${os}" "${architecture}" >"${destination}/uname"
  chmod +x "${destination}/uname"
}

assert_mapping() {
  local os="$1"
  local architecture="$2"
  local expected_target="$3"
  local fake_bin="${scratch}/fake-${os}-${architecture}"
  mkdir -p "${fake_bin}"
  make_fake_uname "${os}" "${architecture}" "${fake_bin}"
  output="$(PATH="${fake_bin}:${PATH}" SPIRE_VERSION="${tag}" SPIRE_INSTALL_DRY_RUN=1 "${installer}")"
  grep -Fqx "target: ${expected_target}" <<<"${output}"
  grep -Fqx "archive: https://github.com/niparis/spire/releases/download/${tag}/spire-${tag}-${expected_target}.tar.gz" <<<"${output}"
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

assert_mapping Linux x86_64 x86_64-unknown-linux-musl
assert_mapping Linux amd64 x86_64-unknown-linux-musl
assert_mapping Darwin arm64 aarch64-apple-darwin
assert_mapping Darwin aarch64 aarch64-apple-darwin
assert_mapping Darwin x86_64 x86_64-apple-darwin
assert_mapping Darwin amd64 x86_64-apple-darwin

mac_checksum_bin="${scratch}/mac-checksum-bin"
mkdir -p "${mac_checksum_bin}"
make_fake_uname Darwin arm64 "${mac_checksum_bin}"
printf '#!/bin/sh\nexec /usr/bin/grep "$@"\n' >"${mac_checksum_bin}/grep"
printf '#!/bin/sh\nexit 0\n' >"${mac_checksum_bin}/shasum"
printf '#!/bin/sh\nexit 0\n' >"${mac_checksum_bin}/curl"
chmod +x "${mac_checksum_bin}/grep" "${mac_checksum_bin}/shasum" "${mac_checksum_bin}/curl"
mac_checksum_output="$(PATH="${mac_checksum_bin}" SPIRE_VERSION="${tag}" SPIRE_INSTALL_DRY_RUN=1 /bin/sh "${installer}")"
grep -Fqx 'checksum command: shasum -a 256' <<<"${mac_checksum_output}"

unsupported_bin="${scratch}/unsupported-bin"
mkdir -p "${unsupported_bin}"
make_fake_uname FreeBSD x86_64 "${unsupported_bin}"
printf '#!/bin/sh\ntouch "%s"\nexit 1\n' "${scratch}/curl-was-called" >"${unsupported_bin}/curl"
chmod +x "${unsupported_bin}/curl"
if PATH="${unsupported_bin}:${PATH}" "${installer}" >/dev/null 2>&1; then
  printf 'unknown OS unexpectedly passed installer detection\n' >&2
  exit 1
fi
[[ ! -e "${scratch}/curl-was-called" ]] || {
  printf 'unsupported platform attempted a download\n' >&2
  exit 1
}

windows_bin="${scratch}/windows-bin"
mkdir -p "${windows_bin}"
make_fake_uname MINGW64_NT-10.0 x86_64 "${windows_bin}"
if windows_output="$(PATH="${windows_bin}:${PATH}" SPIRE_VERSION="${tag}" SPIRE_INSTALL_DRY_RUN=1 "${installer}" 2>&1)"; then
  printf 'native Windows unexpectedly passed installer detection\n' >&2
  exit 1
fi
if ! grep -q 'PowerShell installer' <<<"${windows_output}"; then
  printf 'native Windows guidance is missing\n' >&2
  exit 1
fi

arm_bin="${scratch}/linux-arm-bin"
mkdir -p "${arm_bin}"
make_fake_uname Linux arm64 "${arm_bin}"
if PATH="${arm_bin}:${PATH}" SPIRE_VERSION="${tag}" SPIRE_INSTALL_DRY_RUN=1 "${installer}" >/dev/null 2>&1; then
  printf 'unverified Linux ARM64 unexpectedly passed installer detection\n' >&2
  exit 1
fi

if SPIRE_VERSION=invalid SPIRE_INSTALL_DRY_RUN=1 "${installer}" >/dev/null 2>&1; then
  printf 'invalid installer version unexpectedly passed validation\n' >&2
  exit 1
fi

printf 'installer platform fixtures pass\n'
