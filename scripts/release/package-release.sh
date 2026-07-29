#!/usr/bin/env bash
set -euo pipefail

readonly repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
readonly target="${TARGET_TRIPLE:-x86_64-unknown-linux-musl}"
readonly output_directory="${DIST_DIR:-${repository_root}/dist}"
readonly release_tag="${RELEASE_TAG:-}"

sha256_digest() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

if [[ -z "${release_tag}" ]]; then
  version="$({
    awk '
      /^\[workspace\.package\]$/ { in_workspace_package = 1; next }
      /^\[/ { in_workspace_package = 0 }
      in_workspace_package && /^version = "[0-9]+\.[0-9]+\.[0-9]+"$/ {
        value = $0
        sub(/^version = "/, "", value)
        sub(/"$/, "", value)
        print value
        exit
      }
    ' "${repository_root}/Cargo.toml"
  })"
  release_tag="v${version}"
fi

"${repository_root}/scripts/release/verify-release.sh" "${release_tag}"

if [[ -e "${output_directory}" ]] && [[ -n "$(find "${output_directory}" -mindepth 1 -maxdepth 1 -print -quit)" ]]; then
  printf 'release output directory must be empty: %s\n' "${output_directory}" >&2
  exit 1
fi
mkdir -p "${output_directory}"

(
  cd "${repository_root}"
  cargo build --locked --release --package spire --target "${target}"
)

readonly binary_path="${repository_root}/target/${target}/release/spire"
[[ -x "${binary_path}" ]] || {
  printf 'release binary is missing or not executable: %s\n' "${binary_path}" >&2
  exit 1
}

readonly archive_name="spire-${release_tag}-${target}.tar.gz"
readonly archive_path="${output_directory}/${archive_name}"
readonly staging_directory="$(mktemp -d)"
trap 'rm -rf "${staging_directory}"' EXIT

install -m 0755 "${binary_path}" "${staging_directory}/spire"
install -m 0644 "${repository_root}/LICENSE" "${staging_directory}/LICENSE"
printf '%s\n' "${release_tag#v}" >"${staging_directory}/VERSION"

readonly source_date_epoch="${SOURCE_DATE_EPOCH:-$(git -C "${repository_root}" show -s --format=%ct HEAD)}"
if tar --version 2>/dev/null | grep -q 'GNU tar'; then
  tar \
    --sort=name \
    --mtime="@${source_date_epoch}" \
    --owner=0 \
    --group=0 \
    --numeric-owner \
    -C "${staging_directory}" \
    -czf "${archive_path}" \
    LICENSE VERSION spire
else
  tar -C "${staging_directory}" -czf "${archive_path}" LICENSE VERSION spire
fi

(
  cd "${output_directory}"
  printf '%s  %s\n' "$(sha256_digest "${archive_name}")" "${archive_name}" >SHA256SUMS
)

readonly lockfile_sha256="$(sha256_digest "${repository_root}/Cargo.lock")"
readonly source_commit="$(git -C "${repository_root}" rev-parse HEAD)"
readonly rustc_version="$(rustc --version)"
cat >"${output_directory}/build-metadata.json" <<EOF
{
  "archive": "${archive_name}",
  "cargo_lock_sha256": "${lockfile_sha256}",
  "release_tag": "${release_tag}",
  "rustc": "${rustc_version}",
  "source_commit": "${source_commit}",
  "target": "${target}"
}
EOF

"${repository_root}/scripts/release/verify-release.sh" \
  "${release_tag}" \
  "${archive_path}" \
  "${output_directory}/SHA256SUMS" \
  "${target}"

if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  {
    printf 'archive=%s\n' "${archive_path}"
    printf 'archive_name=%s\n' "${archive_name}"
    printf 'artifact_name=spire-%s-%s\n' "${release_tag}" "${target}"
    printf 'binary=%s\n' "${binary_path}"
  } >>"${GITHUB_OUTPUT}"
fi
