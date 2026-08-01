#!/usr/bin/env bash
set -euo pipefail

readonly repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
readonly assembler="${repository_root}/scripts/release/assemble-release.sh"
readonly publisher="${repository_root}/scripts/release/publish-release.sh"
readonly release_workflow="${repository_root}/.github/workflows/release.yml"
readonly version="$(awk '
  /^\[workspace\.package\]$/ { active = 1; next }
  /^\[/ { active = 0 }
  active && /^version = / { gsub(/"/, "", $3); print $3; exit }
' "${repository_root}/Cargo.toml")"
readonly tag="v${version}"
readonly targets=(x86_64-unknown-linux-musl x86_64-apple-darwin aarch64-apple-darwin)
readonly scratch="$(mktemp -d)"
trap 'rm -rf "${scratch}"' EXIT

digest() {
  shasum -a 256 "$1" | awk '{print $1}'
}

make_archive() {
  local target="$1"
  local archive="spire-${tag}-${target}.tar.gz"
  local stage="${scratch}/stage-${target}"
  mkdir -p "${stage}"
  printf '#!/bin/sh\nprintf "spire %s\\n"\n' "${version}" >"${stage}/spire"
  chmod +x "${stage}/spire"
  printf '%s\n' "${version}" >"${stage}/VERSION"
  printf 'version: test\nproviders: {}\n' >"${stage}/model-catalog.yaml"
  printf 'license fixture\n' >"${stage}/LICENSE"
  tar -C "${stage}" -czf "${scratch}/input/${target}/${archive}" LICENSE VERSION model-catalog.yaml spire
}

mkdir -p "${scratch}/input"
for target in "${targets[@]}"; do
  mkdir -p "${scratch}/input/${target}"
  make_archive "${target}"
done

INPUT_DIR="${scratch}/input" \
DIST_DIR="${scratch}/candidate" \
RELEASE_TAG="${tag}" \
SOURCE_COMMIT="$(git -C "${repository_root}" rev-parse HEAD)" \
WORKFLOW_RUN_URL="https://example.invalid/actions/123" \
SUPPORTED_TARGETS="${targets[*]}" \
"${assembler}"

for target in "${targets[@]}"; do
  archive="spire-${tag}-${target}.tar.gz"
  grep -Fqx "$(digest "${scratch}/candidate/${archive}")  ${archive}" "${scratch}/candidate/SHA256SUMS"
done
grep -Fq '"workflow_run_url": "https://example.invalid/actions/123"' "${scratch}/candidate/build-metadata.json"

printf '#!/bin/sh\nprintf "fixture installer\\n"\n' >"${scratch}/candidate/install.sh"
chmod +x "${scratch}/candidate/install.sh"

mkdir -p "${scratch}/fake-bin" "${scratch}/remote"
cat >"${scratch}/fake-bin/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
readonly root="${FAKE_GH_ROOT:?}"
readonly state="${root}/state"
command="$1"
subcommand="$2"
shift 2

case "${command}:${subcommand}" in
  release:view)
    [[ -f "${state}" ]] || exit 1
    cat "${state}"
    ;;
  release:create)
    printf 'true\n' >"${state}"
    ;;
  release:upload)
    tag="$1"
    asset="$2"
    mkdir -p "${root}/assets"
    cp "${asset}" "${root}/assets/${asset##*/}"
    ;;
  release:download)
    tag="$1"
    shift
    destination=""
    pattern=""
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --dir) destination="$2"; shift 2 ;;
        --pattern) pattern="$2"; shift 2 ;;
        --clobber) shift ;;
        *) shift ;;
      esac
    done
    [[ -d "${root}/assets" ]] || exit 1
    mkdir -p "${destination}"
    shopt -s nullglob
    matches=("${root}/assets"/${pattern})
    [[ ${#matches[@]} -gt 0 ]] || exit 1
    cp "${matches[@]}" "${destination}/"
    ;;
  release:edit)
    tag="$1"
    shift
    for argument in "$@"; do
      if [[ "${argument}" == --draft=false ]]; then
        printf 'false\n' >"${state}"
      fi
    done
    ;;
  *)
    printf 'unexpected fake gh invocation: %s %s\n' "${command}" "${subcommand}" >&2
    exit 1
    ;;
esac
EOF
chmod +x "${scratch}/fake-bin/gh"

run_publisher() {
  PATH="${scratch}/fake-bin:${PATH}" \
  FAKE_GH_ROOT="${scratch}/remote" \
  CANDIDATE_DIR="${scratch}/candidate" \
  INSTALLER_PATH="${scratch}/candidate/install.sh" \
  RELEASE_TAG="${tag}" \
  "${publisher}"
}

# Simulate a crash after one draft asset upload. The retry may repair this draft only
# when the existing byte is identical to the candidate.
mkdir -p "${scratch}/remote/assets"
printf 'true\n' >"${scratch}/remote/state"
first_archive="${scratch}/candidate/spire-${tag}-${targets[0]}.tar.gz"
cp "${first_archive}" "${scratch}/remote/assets/${first_archive##*/}"

run_publisher
[[ "$(<"${scratch}/remote/state")" == false ]]
run_publisher

archive_to_corrupt="${scratch}/candidate/spire-${tag}-${targets[0]}.tar.gz"
printf 'different bytes\n' >>"${archive_to_corrupt}"
if run_publisher >/dev/null 2>&1; then
  printf 'differing published bytes unexpectedly passed retry verification\n' >&2
  exit 1
fi

printf 'release promotion fixtures pass\n'

package_step="$(
  awk '
    /- name: Package the native target/ { active = 1 }
    /- name: Verify the packaged binary version/ { active = 0 }
    active { print }
  ' "${release_workflow}"
)"
if grep -Fq 'steps.package.outputs.binary' <<<"${package_step}"; then
  printf 'package step consumes its own unavailable output\n' >&2
  exit 1
fi

grep -Fq 'BINARY: ${{ steps.package.outputs.binary }}' "${release_workflow}" || {
  printf 'separate packaged-binary verification step is missing\n' >&2
  exit 1
}

publication_job="$(
  awk '
    /^  publish:/ { active = 1 }
    /^  smoke:/ { active = 0 }
    active { print }
  ' "${release_workflow}"
)"
grep -Fq 'environment: release' <<<"${publication_job}" || {
  printf 'publication job must retain the protected release environment\n' >&2
  exit 1
}
[[ "$(grep -c '^    environment: release$' "${release_workflow}")" == 1 ]] || {
  printf 'release workflow must contain exactly one protected environment gate\n' >&2
  exit 1
}

promotion_job="$(
  awk '
    /^  promote_latest:/ { active = 1 }
    active { print }
  ' "${release_workflow}"
)"
if grep -Fq 'environment: release' <<<"${promotion_job}"; then
  printf 'latest promotion must run automatically after public smoke tests\n' >&2
  exit 1
fi
checkout_line="$(
  grep -n -m1 'uses: actions/checkout@' <<<"${promotion_job}" | cut -d: -f1
)"
promotion_line="$(
  grep -n -m1 'run: ./scripts/release/promote-latest.sh' <<<"${promotion_job}" | cut -d: -f1
)"
[[ -n "${checkout_line}" && -n "${promotion_line}" && "${checkout_line}" -lt "${promotion_line}" ]] || {
  printf 'latest-promotion job must check out the validated source before running repository scripts\n' >&2
  exit 1
}

printf 'release workflow structure fixtures pass\n'
