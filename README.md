# Spire

Spire is a single-node orchestrator for bounded Codex and Claude Code work on
ready Linear tickets. It persists workflow state in SQLite, uses GitHub CI and
independent review as gates, and stops at a human-owned merge; Spire and its
harnesses never merge a pull request.

The operating model is defined in the [architecture](docs/ai_harness_architecture.md),
[implementation design](docs/ai_harness_implementation.md), and
[sprint roadmap](docs/sprints/README.md). Operational deployment, recovery, and
rollback are documented in the [pilot runbook](docs/runbooks/operations-and-pilot.md).

## Supported binary and prerequisites

The initial release is a Linux x86_64 binary for
`x86_64-unknown-linux-musl`. It is intended for an operator-managed host; its
configuration, systemd credentials, Linear credentials, GitHub credentials, and
harness credentials are never bundled in a release archive.

Install `curl`, `tar`, `sha256sum`, and `install` before downloading a release. Add
`$HOME/.local/bin` to `PATH` if it is not already present.

## Install a release

Use an explicit version rather than a moving `latest` URL so that installation and
rollback always identify the exact bytes being used.

For a Poetry/Oh My Zsh-style installer, fetch the installer script from the same
immutable release tag as the binary. It verifies the downloaded release checksum
before installing to `~/.local/bin`:

```sh
version=v0.1.0 # replace with the chosen release tag
curl --proto '=https' --tlsv1.2 --fail --location --silent --show-error \
  "https://raw.githubusercontent.com/niparis/spire/${version}/install.sh" \
  | sh -s -- --version "${version}"
```

Set `SPIRE_BIN_DIR` before running the command to choose another installation
directory. Use `--dry-run` after the tag to display the exact asset URLs without
making changes.

For environments where executing a downloaded script is not acceptable, install the
same release manually:

```sh
version=v0.1.0 # replace with the chosen release tag
base_url="https://github.com/niparis/spire/releases/download/${version}"
archive="spire-${version}-x86_64-unknown-linux-musl.tar.gz"

curl --fail --location --remote-name "${base_url}/${archive}"
curl --fail --location --remote-name "${base_url}/SHA256SUMS"
sha256sum --check --ignore-missing SHA256SUMS
tar -xzf "${archive}"
mkdir -p "$HOME/.local/bin"
install -m 0755 spire "$HOME/.local/bin/spire"
spire --version
```

GitHub Actions artifacts are short-lived CI evidence and may require GitHub
authentication. The versioned GitHub Release asset above is the installation
contract. For a private repository, authenticate to GitHub before downloading a
release asset instead of treating the URL as public.

To upgrade, repeat the commands with the approved new tag, validate the installed
version, then follow the [service restart procedure](docs/runbooks/operations-and-pilot.md#install-and-start).
To roll back, reinstall the prior approved tag and follow the
[recovery and rollback runbook](docs/runbooks/operations-and-pilot.md#recovery-and-rollback).

## Build and verify from source

Spire uses the Rust toolchain pinned in [rust-toolchain.toml](rust-toolchain.toml).
From a clean checkout, run:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
./scripts/release/test-release-contract.sh
```

The GitHub Actions build workflow runs those commands, packages the release-mode
binary, creates a SHA-256 manifest, and uploads all three release files as a
14-day Actions artifact. A protected matching `v<major>.<minor>.<patch>` tag
rebuilds the archive and publishes immutable GitHub Release assets. Configure the
repository's `release` environment with required reviewers before enabling tag
publication; that environment is the human approval gate.

## Versioning and releases

The root workspace `Cargo.toml` is the sole version source; each crate inherits it.
Spire currently uses `0.y.z` SemVer: breaking changes increment `y`, while
backwards-compatible changes and fixes increment `z`. A release tag must equal the
workspace version prefixed with `v`, for example `0.1.0` and `v0.1.0`.

Before requesting a release, update the workspace version and release notes, run the
source verification commands, merge the change, create the matching tag on the merged
commit, and approve the `release` environment. The workflow rejects mismatched tags,
invalid archives, and attempts to overwrite a published release.
