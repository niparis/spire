# Spire

Spire is a single-node orchestrator for bounded Codex and Claude Code work on
ready Linear tickets. It persists workflow state in SQLite, uses GitHub CI and
independent review as gates, and stops at a human-owned merge; Spire and its
harnesses never merge a pull request.

The operating model is defined in the [architecture](docs/ai_harness_architecture.md),
[implementation design](docs/ai_harness_implementation.md), and
[sprint roadmap](docs/sprints/README.md). Operational deployment, recovery, and
rollback are documented in the [pilot runbook](docs/runbooks/operations-and-pilot.md).

## Supported binaries and prerequisites

Spire `v2.0.0-rc.1` starts a new, incompatible Rust-orchestrator release line; it
does not continue the earlier Go CLI's `v1.0.3` compatibility contract. Release
candidates use normal SemVer prerelease identifiers, and stable 2.x releases use
normal SemVer once the candidate contract has been proven.

The installer supports Linux x86_64 (`x86_64-unknown-linux-musl`), macOS Intel
(`x86_64-apple-darwin`), and macOS Apple Silicon (`aarch64-apple-darwin`). Linux
ARM64 is deliberately not published until it has native CI execution evidence.
Native Windows is deliberately unsupported by `install.sh`; it needs a separate
PowerShell installer. Spire is intended for an operator-managed host; configuration,
systemd credentials, Linear credentials, GitHub credentials, and harness credentials
are never bundled in a release archive.

Install `curl`, `tar`, `install`, and either `sha256sum` or `shasum` before
downloading a release. Add `$HOME/.local/bin` to `PATH` if it is not already present.

## Install a release

Install the latest release with one command. The installer verifies the downloaded
release checksum before installing to `~/.local/bin`:

```sh
curl -LsSf https://github.com/niparis/spire/releases/latest/download/install.sh | sh
```

Set `SPIRE_BIN_DIR` before running the command to choose another installation
directory.

For a pinned installation or environments where executing a downloaded script is not
acceptable, install a specific release manually:

```sh
version=v2.0.0-rc.1 # replace with the chosen release tag
base_url="https://github.com/niparis/spire/releases/download/${version}"
target="x86_64-unknown-linux-musl" # choose the target for your supported host
archive="spire-${version}-${target}.tar.gz"

curl --fail --location --remote-name "${base_url}/${archive}"
curl --fail --location --remote-name "${base_url}/SHA256SUMS"
expected_checksum="$(awk -v name="${archive}" '$2 == name { print $1; exit }' SHA256SUMS)"
test -n "${expected_checksum}"
if command -v sha256sum >/dev/null 2>&1; then
  actual_checksum="$(sha256sum "${archive}" | awk '{print $1}')"
else
  actual_checksum="$(shasum -a 256 "${archive}" | awk '{print $1}')"
fi
test "${actual_checksum}" = "${expected_checksum}"
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

The GitHub Actions build workflow runs those commands and creates isolated target
artifacts. A protected matching SemVer tag (including an allowed prerelease) runs
read-only validation and native target builds, assembles one candidate, then asks for
approval only before release mutation. The workflow verifies draft-uploaded bytes,
publishes without changing `latest`, runs public version-pinned installation smoke
tests, and then asks for approval to promote `latest`. Configure the repository's
`release` environment with required reviewers before enabling publication and latest
promotion. See the [release-promotion runbook](docs/runbooks/release-promotion.md).

## Versioning and releases

The root workspace `Cargo.toml` is the sole version source; each crate inherits it.
`v2.0.0-rc.1` is the explicit incompatible-generation decision because `v1.0.3`
describes the retired Go CLI, not this Rust orchestrator. A release tag must equal the
workspace version prefixed with `v`, including a prerelease suffix when present.

Before requesting a release, update `Cargo.toml` and `Cargo.lock` together, document
compatibility and platform changes, run the source verification commands, and run
`./scripts/release/preflight-release.sh v<version> <merged-sha>`. Then create the
matching protected tag on the merged commit and approve the `release` environment.
The workflow rejects mismatched tags, invalid archives, and attempts to overwrite
published bytes.
