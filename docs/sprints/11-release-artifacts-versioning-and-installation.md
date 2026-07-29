# Sprint 11 — Release Artifacts, Versioning, and Installation

**Last Verified:** 2026-07-29  
**Depends on:** Sprint 09 exit criteria  
**Unlocks:** Sprint 12 and repeatable operator installation/upgrade of the Spire
binary

## Outcome

Every validated revision produces a downloadable GitHub Actions artifact. A signed-off
SemVer tag produces a stable GitHub Release archive and checksum that an operator can
install with a documented `curl` command. The binary reports the same release version
at runtime through `spire --version`.

Actions artifacts and release assets have deliberately different roles:

| Surface | Purpose | Retention and access | Installation contract |
|---|---|---|---|
| GitHub Actions artifact | CI evidence and short-lived retrieval for each workflow run | GitHub-configured retention; access may require GitHub authentication | Not a stable installer URL |
| GitHub Release asset | Immutable, tagged operator distribution | Repository release retention; public only if the repository/release is public | Stable `curl` download target |

This sprint does not introduce package-manager distribution, automatic upgrades,
container images, or changes to the orchestrator's Linear, GitHub, or harness
authority. Sprint 11A extends this original Linux artifact contract with native macOS
targets and staged release promotion.

## Entry criteria

- Sprint 09 has demonstrated a stable pilot and documented upgrade/rollback runbook.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and
  `cargo test --workspace` pass from a clean checkout.
- `Cargo.lock` is committed and the current production target is confirmed as Linux
  x86_64.
- A human release owner and the GitHub repository's release permission policy are
  named.

## Release and versioning contract

### Canonical version

1. Keep one canonical SemVer version in `[workspace.package] version` in the root
   `Cargo.toml`.
2. Change every crate to `version.workspace = true`; no crate may carry an independent
   package version while releases are workspace-wide.
3. Release tags are exactly `v<SemVer>` and must equal the workspace version with the
   leading `v` removed. For example, workspace version `2.0.0-rc.1` requires tag
   `v2.0.0-rc.1`.
4. The former Go CLI ended at public `v1.0.3`; this Rust orchestrator is an
   incompatible generation. The explicit continuity decision is `v2.0.0-rc.1`.
   It accepts SemVer prerelease identifiers while the release pipeline is proven;
   stable 2.x releases use normal SemVer major/minor/patch rules.
5. Only clean, merged commits receive release tags. Development builds must not claim
   a release tag; if a development suffix is introduced later, it must be generated
   from the commit SHA and be visibly distinct from a release version.

### Runtime version contract

- The root binary crate supplies its package version to Clap, so `spire --version`
  and `spire -V` print `spire <workspace version>` without reading configuration,
  the database, or the network.
- A test invokes the built binary and asserts both flags succeed and match
  `CARGO_PKG_VERSION` exactly.
- The release workflow invokes `spire --version` after packaging and compares the
  result with the validated tag version. A mismatch fails publication.

## Work packages

### S11.1 Establish release metadata and version checks

Implementation:

1. Move the workspace version to the root workspace package metadata and inherit it
   from all four crates.
2. Add a checked-in release-validation script or equivalent Rust test that validates
   SemVer, tag-to-Cargo-version equality, archive naming, and checksum manifest
   contents.
3. Configure Clap explicitly with `#[command(version)]` so the binary version
   derives only from Cargo package metadata.
4. Document the human release sequence: update version, update release notes, merge,
   tag, and approve the release workflow.

Verification:

- A mismatched tag and package version fail before a release is created.
- Changing a member crate version independently fails the repository check.
- `spire --version` and `spire -V` succeed without a configuration file.

### S11.2 Create the build-and-artifact GitHub Actions workflow

Create `.github/workflows/build.yml` for pull requests and pushes to `main`.

Implementation:

1. Pin the runner image, Rust toolchain, and third-party Actions to immutable
   revisions; grant only `contents: read` to the ordinary build job.
2. Check out the exact workflow SHA and run the canonical workspace gates:
   `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and
   `cargo test --workspace`.
3. Cache Cargo registry/Git downloads and `target/` artifacts by runner OS, target
   triple, lockfile, and Rust toolchain. Restore a compatible source-only fallback;
   never cache credentials or release outputs.
4. Build the distributable binary with
   `cargo build --locked --release --package spire --target x86_64-unknown-linux-musl`
   for the confirmed initial target.
5. Package only the `spire` executable, `LICENSE`, and a generated
   `VERSION` file into a deterministic `tar.gz` archive named
   `spire-v<version>-x86_64-unknown-linux-musl.tar.gz`.
6. Generate a SHA-256 manifest named `SHA256SUMS` beside the archive.
7. Upload the archive, manifest, and build metadata as one Actions artifact named
   `spire-v<version>-x86_64-unknown-linux-musl`.
8. Set artifact retention explicitly and document its value; do not rely on the
   repository default.

Verification:

- A pull request exposes an artifact only after all canonical gates pass.
- Downloading the artifact and verifying `SHA256SUMS` succeeds.
- The archive contains the release-mode executable and no target directory,
  credentials, configuration, or build-cache material.
- A failed test, lint, or package-version check uploads no distributable artifact.

### S11.3 Publish immutable release assets from validated tags

Create a separate tag-triggered release job or workflow. It may use `contents: write`
only in that job, and only for the `v*.*.*` tag path.

Implementation:

1. Re-run the locked build and all canonical gates from the tag SHA; never promote a
   binary copied from another workflow run.
2. Verify the tag/version contract before archive creation.
3. Attach the exact archive and `SHA256SUMS` to GitHub Release `v<version>`.
4. Make release publication idempotent: a retry may update an incomplete draft for
   the same tag, but it must fail if an existing published release has different
   bytes or checksums.
5. Publish only after the named human release owner approves the protected release
   environment. Never create a release from a pull-request or fork workflow.

Verification:

- An untagged `main` build cannot obtain release-write permissions.
- A tag whose version differs from Cargo cannot publish.
- Rebuilding the same tag produces the expected archive name and matching checksum;
  any byte difference blocks publication.
- The release asset can be fetched without a GitHub token when the repository and
  release are public; private-repository access requirements are documented instead
  of implied.

### S11.4 Write the root README and installation contract

Create `README.md` as a concise operator-facing entry point. It must contain:

1. A one-paragraph description of Spire as a single-node orchestrator for bounded
   Codex/Claude Code work, plus the no-autonomous-merge boundary.
2. Links to the architecture, implementation design, sprint roadmap, and operator
   runbooks.
3. Prerequisites and supported-platform statement: Sprint 11 establishes Linux
   x86_64 (`x86_64-unknown-linux-musl`); Sprint 11A adds native macOS target
   validation. Configuration and provider credentials remain operator-owned.
4. Source verification commands and the canonical CI gates.
5. A shell-installer command following the standard one-line pattern:
   `curl -LsSf https://github.com/niparis/spire/releases/latest/download/install.sh | sh`.
   The release workflow publishes `install.sh` with each release; the script resolves
   the latest tag, verifies its corresponding GitHub Release archive by SHA-256, and
   supports `SPIRE_BIN_DIR` while failing clearly for an unsupported platform.
6. A version-pinned, checksum-verified manual installation example using the GitHub
   Release asset, not an Actions artifact URL:

```sh
version=v2.0.0-rc.1 # replace with the chosen release tag
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

7. A note that `latest` URLs are intentionally not used in the primary command, so
   installations are reproducible and a human controls upgrades.
8. Upgrade, rollback, and service-restart links to the Sprint 09 runbook. Do not
   place real credentials, tokens, or production configuration in the README.

Verification:

- The README command works in a clean Linux x86_64 environment for a published test
  release and verifies the exact archive before installation.
- The installed binary reports the requested version.
- Markdown links resolve and the README contains no secret-bearing examples.

### S11.5 Add release failure and provenance coverage

Implementation:

1. Add deterministic tests/fixtures for malformed tags, version mismatches, missing
   archive members, checksum failure, and duplicate-release retry behavior.
2. Record the source commit SHA, Rust toolchain, target triple, Cargo lockfile hash,
   archive SHA-256, and workflow run URL in the release notes or attached metadata.
3. Ensure logs and workflow summaries contain identifiers and checksums but never
   credentials, authorization headers, or raw provider events.

Verification:

- A corrupted archive is rejected by the documented checksum command.
- The archive provenance identifies the exact source revision and lockfile.
- Release retry cannot silently overwrite a published asset.

## Suggested pull-request slices

1. Workspace version inheritance, explicit `--version`, and version tests.
2. Pull-request/main build workflow and Actions artifact packaging.
3. Protected tag release workflow, checksum/provenance enforcement, and failure tests.
4. Root README and upgrade/rollback documentation.

## Sprint demo

Merge a version bump, run the pull-request/main workflow to download and verify its
Actions artifact, then create a protected matching tag. After human approval, fetch
the published release archive using the README's `curl` commands in a clean Linux
x86_64 environment, verify its checksum, install it, and show that `spire --version`
matches the tag.

## Exit criteria

- Canonical CI gates build the release-mode Rust binary and upload a versioned,
  checksum-bearing Actions artifact.
- A protected, matching SemVer tag produces immutable GitHub Release assets without
  promoting arbitrary Actions-run output.
- The workspace has one canonical version and `spire --version` reports it exactly.
- The root README explains the product, supported platform, source checks, safe
  version-pinned installation, upgrade, and rollback boundaries.
- Artifact/release publication uses least privilege and has deterministic failure
  coverage.

## Evidence Sources

- [`../../README.md`](../../README.md), the operator installation contract.
- [`../ai_harness_architecture.md`](../ai_harness_architecture.md), repository
  artifact and CI boundaries.
- [`../ai_harness_implementation.md`](../ai_harness_implementation.md), deployment
  model and configuration ownership.
- Sprint 01 canonical workspace verification gates.
- Sprint 09 deployment, upgrade, rollback, and operator-runbook evidence.

## Unknown / Unverified

- Linux ARM64, Windows, package managers, containers, and systemd package formats
  require separate decisions. The Sprint 11A macOS targets remain unverified until
  their native tag-workflow runs pass.
- The repository's release visibility must be confirmed before enabling public
  installation guidance. Actions artifacts retain for 14 days.
- Sigstore/cosign signing and SBOM/provenance attestations are deferred; SHA-256
  checksums are integrity verification, not publisher identity verification.
- The selected GitHub Actions and Rust toolchain pins need an explicit maintenance
  policy as they age.
