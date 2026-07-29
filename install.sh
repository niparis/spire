#!/usr/bin/env sh
set -eu

repository="niparis/spire"
bin_dir="${SPIRE_BIN_DIR:-${HOME}/.local/bin}"

fail() {
  printf 'spire installer: %s\n' "$*" >&2
  exit 1
}

[ "$#" -eq 0 ] || fail "this installer takes no arguments"

detect_platform() {
  detected_os="$(uname -s)"
  detected_architecture="$(uname -m)"

  case "${detected_os}:${detected_architecture}" in
    Linux:x86_64 | Linux:amd64)
      normalized_os="linux"
      normalized_architecture="x86_64"
      target="x86_64-unknown-linux-musl"
      ;;
    Darwin:arm64 | Darwin:aarch64)
      normalized_os="macos"
      normalized_architecture="aarch64"
      target="aarch64-apple-darwin"
      ;;
    Darwin:x86_64 | Darwin:amd64)
      normalized_os="macos"
      normalized_architecture="x86_64"
      target="x86_64-apple-darwin"
      ;;
    Linux:aarch64 | Linux:arm64)
      fail "unsupported platform: OS=${detected_os} architecture=${detected_architecture}; Linux ARM64 is not published yet"
      ;;
    MINGW*:* | MSYS*:* | CYGWIN*:*)
      fail "unsupported platform: OS=${detected_os} architecture=${detected_architecture}; native Windows requires the separate PowerShell installer"
      ;;
    *)
      fail "unsupported platform: OS=${detected_os} architecture=${detected_architecture}"
      ;;
  esac
}

select_checksum_command() {
  if command -v sha256sum >/dev/null 2>&1; then
    checksum_command="sha256sum"
  elif command -v shasum >/dev/null 2>&1; then
    checksum_command="shasum -a 256"
  else
    fail "a SHA-256 command is required (sha256sum or shasum -a 256)"
  fi
}

sha256_file() {
  if [ "${checksum_command}" = "sha256sum" ]; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

detect_platform
select_checksum_command

command -v curl >/dev/null 2>&1 || fail "curl is required"

version="${SPIRE_VERSION:-}"
if [ -z "${version}" ]; then
  latest_release_url="$(curl -LsSf -o /dev/null -w '%{url_effective}' "https://github.com/${repository}/releases/latest")"
  version="${latest_release_url##*/}"
fi
printf '%s\n' "${version}" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z][0-9A-Za-z.-]*)?$' \
  || fail "could not resolve a valid latest release tag"

archive="spire-${version}-${target}.tar.gz"
base_url="https://github.com/${repository}/releases/download/${version}"

if [ "${SPIRE_INSTALL_DRY_RUN:-}" = 1 ]; then
  printf 'os: %s\n' "${normalized_os}"
  printf 'architecture: %s\n' "${normalized_architecture}"
  printf 'target: %s\n' "${target}"
  printf 'archive: %s/%s\n' "${base_url}" "${archive}"
  printf 'checksums: %s/SHA256SUMS\n' "${base_url}"
  printf 'checksum command: %s\n' "${checksum_command}"
  printf 'destination: %s/spire\n' "${bin_dir}"
  exit 0
fi

command -v tar >/dev/null 2>&1 || fail "tar is required"
command -v install >/dev/null 2>&1 || fail "install is required"

temporary_directory="$(mktemp -d)"
trap 'rm -rf "${temporary_directory}"' EXIT INT TERM

curl --proto '=https' --tlsv1.2 --fail --location --silent --show-error \
  --output "${temporary_directory}/${archive}" \
  "${base_url}/${archive}"
curl --proto '=https' --tlsv1.2 --fail --location --silent --show-error \
  --output "${temporary_directory}/SHA256SUMS" \
  "${base_url}/SHA256SUMS"

expected_checksum="$(awk -v name="${archive}" '$2 == name { print $1; exit }' "${temporary_directory}/SHA256SUMS")"
[ -n "${expected_checksum}" ] || fail "checksum for ${archive} is missing"
actual_checksum="$(sha256_file "${temporary_directory}/${archive}")"
[ "${actual_checksum}" = "${expected_checksum}" ] || fail "checksum verification failed"

tar -xzf "${temporary_directory}/${archive}" -C "${temporary_directory}"
[ -x "${temporary_directory}/spire" ] || fail "release archive does not contain an executable spire binary"
installed_version="$("${temporary_directory}/spire" --version)" || fail "release binary could not report its version"
[ "${installed_version}" = "spire ${version#v}" ] || fail "release binary version does not match ${version}"

mkdir -p "${bin_dir}"
install -m 0755 "${temporary_directory}/spire" "${bin_dir}/spire"
printf 'installed Spire %s to %s/spire\n' "${version}" "${bin_dir}"
[ "$("${bin_dir}/spire" --version)" = "spire ${version#v}" ] || fail "installed binary version does not match ${version}"
