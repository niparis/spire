#!/usr/bin/env sh
set -eu

repository="niparis/spire"
bin_dir="${SPIRE_BIN_DIR:-${HOME}/.local/bin}"

fail() {
  printf 'spire installer: %s\n' "$*" >&2
  exit 1
}

[ "$#" -eq 0 ] || fail "this installer takes no arguments"

command -v curl >/dev/null 2>&1 || fail "curl is required"

version="${SPIRE_VERSION:-}"
if [ -z "${version}" ]; then
  latest_release_url="$(curl -LsSf -o /dev/null -w '%{url_effective}' "https://github.com/${repository}/releases/latest")"
  version="${latest_release_url##*/}"
fi
printf '%s\n' "${version}" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+$' \
  || fail "could not resolve a valid latest release tag"

target="x86_64-unknown-linux-musl"
archive="spire-${version}-${target}.tar.gz"
base_url="https://github.com/${repository}/releases/download/${version}"

if [ "${SPIRE_INSTALL_DRY_RUN:-}" = 1 ]; then
  printf 'archive: %s/%s\n' "${base_url}" "${archive}"
  printf 'checksums: %s/SHA256SUMS\n' "${base_url}"
  printf 'destination: %s/spire\n' "${bin_dir}"
  exit 0
fi

[ "$(uname -s)" = Linux ] || fail "only Linux x86_64 is supported"
case "$(uname -m)" in
  x86_64 | amd64) ;;
  *) fail "only Linux x86_64 is supported" ;;
esac

command -v sha256sum >/dev/null 2>&1 || fail "sha256sum is required"
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
actual_checksum="$(sha256sum "${temporary_directory}/${archive}" | awk '{print $1}')"
[ "${actual_checksum}" = "${expected_checksum}" ] || fail "checksum verification failed"

tar -xzf "${temporary_directory}/${archive}" -C "${temporary_directory}"
[ -x "${temporary_directory}/spire" ] || fail "release archive does not contain an executable spire binary"

mkdir -p "${bin_dir}"
install -m 0755 "${temporary_directory}/spire" "${bin_dir}/spire"
printf 'installed Spire %s to %s/spire\n' "${version}" "${bin_dir}"
"${bin_dir}/spire" --version
