#!/usr/bin/env bash
set -euo pipefail

BINARY_NAME="spire"
REPO="${SPIRE_REPO:-opencode-spire/opencode-spire}"
VERSION="${SPIRE_VERSION:-latest}"
INSTALL_DIR="${SPIRE_INSTALL_DIR:-}"
DRY_RUN="${SPIRE_DRY_RUN:-0}"

log() {
  printf "%s\n" "$*"
}

warn() {
  printf "warning: %s\n" "$*" >&2
}

fail() {
  printf "error: %s\n" "$*" >&2
  exit 1
}

detect_os() {
  case "$(uname -s)" in
    Darwin) echo "darwin" ;;
    Linux) echo "linux" ;;
    *) fail "unsupported operating system: $(uname -s)" ;;
  esac
}

detect_arch() {
  case "$(uname -m)" in
    arm64|aarch64) echo "arm64" ;;
    x86_64|amd64) echo "amd64" ;;
    *) fail "unsupported architecture: $(uname -m)" ;;
  esac
}

in_path() {
  local dir="$1"
  case ":$PATH:" in
    *":${dir}:"*) return 0 ;;
    *) return 1 ;;
  esac
}

pick_install_dir() {
  local os_name="$1"

  if [[ -n "$INSTALL_DIR" ]]; then
    mkdir -p "$INSTALL_DIR"
    echo "$INSTALL_DIR"
    return
  fi

  local preferred
  if [[ "$os_name" == "darwin" ]]; then
    preferred="/usr/local/bin"
  else
    preferred="${HOME}/.local/bin"
  fi

  if [[ -d "$preferred" && -w "$preferred" ]]; then
    echo "$preferred"
    return
  fi

  if [[ ! -d "$preferred" ]]; then
    mkdir -p "$preferred" 2>/dev/null || true
    if [[ -d "$preferred" && -w "$preferred" ]]; then
      echo "$preferred"
      return
    fi
  fi

  local fallback="${HOME}/bin"
  mkdir -p "$fallback"
  echo "$fallback"
}

build_download_url() {
  local asset="$1"
  if [[ "$VERSION" == "latest" ]]; then
    echo "https://github.com/${REPO}/releases/latest/download/${asset}"
    return
  fi

  local tag="$VERSION"
  if [[ "$tag" != v* ]]; then
    tag="v${tag}"
  fi
  echo "https://github.com/${REPO}/releases/download/${tag}/${asset}"
}

download_binary() {
  local url="$1"
  local destination="$2"

  if [[ "$DRY_RUN" == "1" ]]; then
    log "[dry-run] curl -fsSL \"${url}\" -o \"${destination}\""
    return
  fi

  curl -fsSL "$url" -o "$destination"
  chmod +x "$destination"
}

main() {
  local os_name arch asset url target_dir target_file
  os_name="$(detect_os)"
  arch="$(detect_arch)"

  target_dir="$(pick_install_dir "$os_name")"
  target_file="${target_dir}/${BINARY_NAME}"
  asset="${BINARY_NAME}_${os_name}_${arch}"
  url="$(build_download_url "$asset")"

  log "Installing ${BINARY_NAME} (${os_name}/${arch})"
  log "Source: ${url}"
  log "Target: ${target_file}"

  download_binary "$url" "$target_file"

  if [[ "$DRY_RUN" == "1" ]]; then
    log "[dry-run] installation skipped"
    exit 0
  fi

  if ! in_path "$target_dir"; then
    warn "${target_dir} is not in PATH"
    log "Add this to your shell profile:"
    log "  export PATH=\"${target_dir}:\$PATH\""
  fi

  log "Installed ${BINARY_NAME}."
  log "Run: ${BINARY_NAME} --version"
}

main "$@"
