# Sprint 11A — Release Promotion and Multi-Platform Installation

**Last Verified:** 2026-07-29  
**Depends on:** Sprint 11 release artifacts and installation contract  
**Unlocks:** The next public Spire release and a reliable
`releases/latest/download/install.sh` installation path  
**Release gate:** Complete this sprint before publishing the next stable release

## Outcome

Spire releases use an explicit, recoverable promotion pipeline. A release owner
chooses a version consistent with the public release history, a read-only stage
validates and builds every supported target, a protected write stage publishes the
validated bytes without immediately changing `latest`, and public installation
smoke tests pass before the new release becomes the default.

The shell installer maps the host operating system and architecture to a release
target instead of assuming Linux x86_64. It downloads only an asset produced by the
same release workflow, verifies that asset against the shared checksum manifest,
and fails before mutation for every unsupported platform.

This sprint hardens release delivery. It does not add automatic application
upgrades, package-manager distribution, container images, autonomous rollback of a
running Spire service, or broader orchestrator authority.

## Version-continuity decision

The Rust orchestrator is not backwards-compatible with the public `v1.0.3` Go CLI:
the executable, configuration, workflow model, and supported platforms are a new
product generation. The selected first release is therefore `v2.0.0-rc.1`, using
standard SemVer prerelease syntax while this promotion pipeline is proven. A stable
2.x tag requires a separate compatibility decision; no 1.x tag will be reused.

## Pre-implementation evidence

- GitHub's latest published release is `v1.0.3`. Its assets do not include
  `install.sh`, `SHA256SUMS`, or the Linux archive expected by the current README.
- The Rust workspace version is `0.1.0`, while public release history already
  contains `v0.1.0` through `v1.0.3`.
- The current release workflow has not yet completed a tag-triggered run.
- `install.sh` hard-codes `x86_64-unknown-linux-musl`. It checks `uname` only to
  reject other hosts and performs that check after its dry-run output has already
  selected the Linux archive.
- The release workflow gives its only job `contents: write` and protects the whole
  job with the `release` environment. Human approval therefore occurs before tag
  validation, tests, and packaging rather than immediately before publication.
- The current workflow publishes a draft before testing the uploaded assets or the
  public installer path.
- Release metadata records the source SHA, Rust toolchain, target, and lockfile
  checksum, but not the archive checksum and workflow-run URL required by Sprint 11.

## Entry criteria

- Sprint 11's local release-contract fixtures pass.
- The default branch passes the canonical gates:
  `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, and
  `cargo test --workspace`.
- The repository is public, or authenticated installation requirements are
  explicitly selected and documented.
- A named release owner can configure the protected `release` environment and
  repository tag rules.
- No next-release tag has been created or pushed.

## Release invariants

1. A public release tag is immutable and identifies one source commit and one asset
   set.
2. Existing public tags and versions are never reused, moved, or silently
   overwritten.
3. Version selection is a product-compatibility decision, not merely the next
   available number.
4. Validation and packaging run with `contents: read`. Release-write permission is
   available only after validation and human approval.
5. The publication stage promotes the exact artifact set produced by the validated
   tag workflow. It does not rebuild or substitute binaries after approval.
6. Draft and uploaded assets are verified before publication. Version-specific
   public URLs are verified before the release becomes `latest`.
7. A failed candidate never displaces the prior known-good `latest` release.
8. Every supported installer mapping has a corresponding release asset and a native
   end-to-end smoke test.
9. Unsupported platforms fail with the detected OS and architecture before
   downloading or installing a binary.
10. All downloads are checksum-verified. Logs and metadata contain no credentials or
    authorization headers.

## Promotion pipeline

```text
release-preparation PR
  → merged commit and matching protected tag
  → read-only tag/version validation
  → build and test target matrix
  → assemble and verify one immutable candidate artifact set
  → human approval
  → create/update draft and upload exact candidate bytes
  → download and verify uploaded draft assets
  → publish without changing latest
  → public version-pinned installation smoke tests
  → mark release latest
  → verify latest installer alias and installed version
```

### Failure and recovery policy

| Failure point | Required result |
|---|---|
| Before draft creation | No release mutation; fix through a new commit and tag |
| During draft upload | Retain an incomplete draft for an idempotent retry of the same tag and SHA |
| Draft asset verification | Do not publish; repair only the draft for the same tag and SHA |
| Public version-pinned smoke test | Do not mark latest; retain or classify the failed candidate for investigation |
| Latest-alias verification | Restore the prior known-good release as latest, alert the release owner, and do not move either tag |
| Defect found after promotion | Restore the prior release as latest and cut a new version; never replace published bytes |

Release retry behavior must compare the candidate and remote asset names and SHA-256
digests. An identical published release is a successful no-op followed by
verification. A differing published asset fails closed.

## Supported-platform contract

The installer derives a Rust release target from normalized `uname -s` and
`uname -m` values. The final supported set is evidence-based: a row becomes
supported only when its native build and end-to-end installation smoke test pass in
CI.

| Host OS | Host architecture aliases | Candidate release target | Shell installer |
|---|---|---|---|
| Linux | `x86_64`, `amd64` | `x86_64-unknown-linux-musl` | Required; already built |
| Linux | `aarch64`, `arm64` | `aarch64-unknown-linux-musl` | Candidate; requires cross/native build evidence |
| macOS | `arm64`, `aarch64` | `aarch64-apple-darwin` | Candidate; requires a native macOS runner |
| macOS | `x86_64`, `amd64` | `x86_64-apple-darwin` | Candidate; requires a native macOS runner |
| Windows | native Windows architectures | No shell target in this sprint | Explicitly unsupported by `install.sh`; assess a separate PowerShell installer |

Running the shell installer inside WSL is a Linux installation and selects a Linux
target. Native Windows support must not be inferred from the presence of Git Bash,
MSYS, or an `.exe` asset.

The release asset contract is:

```text
spire-v<version>-<target>.tar.gz
SHA256SUMS
build-metadata.json
install.sh
```

`SHA256SUMS` authenticates every target archive in that release. Metadata contains a
per-target archive name and SHA-256 digest plus the release tag, source commit,
Cargo lockfile digest, Rust toolchain, target, and workflow-run URL.

## Work packages

### S11A.1 Decide version continuity and prerelease policy

Implementation:

1. Compare the Rust orchestrator's CLI, configuration, behavior, and supported
   platforms with the public `v1.0.3` contract.
2. Record one explicit decision:
   - use `v1.0.4` only for a backwards-compatible continuation of 1.x; or
   - use `v2.0.0` for an incompatible stable generation; or
   - support SemVer prerelease identifiers and begin with `v2.0.0-rc.1` when the
     new release workflow or product contract still needs proving.
3. Update the root README, Sprint 11, and every affected release example so the
   repository has one versioning policy.
4. Update `Cargo.toml` and `Cargo.lock` together in a release-preparation PR.
5. Add a read-only preflight check that the proposed tag and release do not already
   exist and that the version advances the selected public release line.

Verification:

- A reused tag or release fails before merge or tag creation.
- A prerelease is either accepted consistently by the workflow, verifier,
  installer, and documentation or rejected consistently everywhere.
- The release PR states compatibility and supported-platform changes explicitly.

### S11A.2 Split validation, approval, and publication authority

Implementation:

1. Split the tag workflow into:
   - a read-only validation/build job;
   - a read-only candidate-assembly and verification job;
   - a protected publication job with `contents: write`;
   - public smoke-test jobs; and
   - a protected latest-promotion job.
2. Attach the `release` environment only to jobs that mutate GitHub release state.
3. Set `persist-credentials: false` on read-only checkouts.
4. Transfer the candidate between jobs as a workflow artifact produced by the same
   tag run. Do not rebuild after human approval.
5. Bind every job to the triggering tag name and source SHA and include both in job
   summaries.

Verification:

- Invalid tags, failed tests, or failed packaging never request publication
  approval and never receive release-write permission.
- Changing candidate bytes after approval is impossible without starting a new
  workflow run and approval.
- An untagged or fork-triggered workflow cannot enter a publication job.

### S11A.3 Build and assemble the supported target matrix

Implementation:

1. Confirm each candidate in the supported-platform table using a pinned native or
   documented cross-compilation runner.
2. Build with `cargo build --locked --release --package spire --target <target>`.
3. Run native `spire --version` validation for every supported target. Cross-built
   binaries require a separate native execution job before support is claimed.
4. Refactor packaging so parallel target jobs produce isolated archives and
   metadata fragments without racing on `dist/SHA256SUMS`.
5. Assemble one release-level checksum manifest and metadata document from the
   verified target outputs.
6. Reject missing, duplicate, unexpected, or differently named target assets.

Verification:

- Every supported matrix row has exactly one archive and checksum entry.
- A target whose binary cannot execute natively is excluded from the published
  support matrix and installer mapping.
- Failure of any required target blocks candidate assembly and publication.

### S11A.4 Implement deterministic installer platform selection

Implementation:

1. Move platform detection before archive construction and dry-run output.
2. Normalize the `uname -s` and `uname -m` aliases listed in the supported-platform
   table and map them to exactly one release target.
3. Keep detection and mapping as a side-effect-free shell function that fixtures can
   exercise using a controlled fake `uname` executable on `PATH`; do not add a
   production environment variable that silently overrides host detection.
4. Select an available checksum command:
   - `sha256sum` on GNU/Linux when present;
   - `shasum -a 256` on macOS or other supported hosts when present;
   - otherwise fail before installation.
5. Preserve `SPIRE_VERSION`, `SPIRE_BIN_DIR`, TLS restrictions, checksum
   verification, temporary-directory cleanup, and executable/version verification.
6. Make dry-run output include normalized OS, architecture, selected target, archive
   URL, checksum URL, and destination.
7. Include the detected OS and architecture in unsupported-platform errors.

Verification:

- Deterministic fixtures cover every alias and supported mapping.
- Unknown OS, unknown architecture, and known OS with unsupported architecture fail
  before the first asset download.
- macOS fixtures use `shasum -a 256` when `sha256sum` is unavailable.
- Dry-run output selects the same archive as a real installation on that host.
- WSL selects the supported Linux target; native Windows-like environments fail
  with PowerShell guidance.

### S11A.5 Stage, verify, publish, and promote releases

Implementation:

1. Create or resume a draft bound to the exact tag and source SHA.
2. Upload the complete candidate asset set.
3. Download every uploaded draft asset using authenticated GitHub access into a
   clean directory and compare names, sizes, and SHA-256 digests with the candidate.
4. Publish the verified release without marking it latest.
5. Run public, unauthenticated, version-pinned installation tests on every supported
   native platform using a temporary `SPIRE_BIN_DIR`.
6. Assert the installed binary reports the tag version and that no files outside the
   temporary installation directory changed.
7. Mark the release latest only after all required public smoke tests pass.
8. Fetch `releases/latest/download/install.sh`, confirm it resolves to the promoted
   release, install in a clean supported environment, and verify the binary version.

Verification:

- A missing public asset, wrong checksum, corrupt archive, wrong binary version, or
  unsupported native execution blocks latest promotion.
- Until promotion succeeds, the previous release remains latest and its installer
  URL remains unchanged.
- The latest alias resolves to a release containing every required asset.

### S11A.6 Complete provenance, immutability, and recovery controls

Implementation:

1. Add archive SHA-256 and workflow-run URL to release metadata.
2. Emit a workflow summary containing the tag, source SHA, targets, archive digests,
   draft/release URL, approval result, smoke-test result, and previous/new latest
   tags.
3. Configure a repository ruleset that prevents unauthorized creation, movement,
   and deletion of release tags.
4. Enable GitHub release immutability after draft-retry and promotion behavior has
   been demonstrated in a disposable candidate release.
5. Document the command and authority required to restore the prior release as
   latest without changing either release's assets or tag.
6. Add deterministic fixtures for identical retry, differing published bytes,
   incomplete draft recovery, failed public smoke, and failed latest-alias
   verification.

Verification:

- Published release assets cannot be overwritten.
- An identical rerun is a verified no-op; differing bytes fail closed.
- The release owner can restore the previous latest pointer using the runbook.
- Release logs and metadata expose no credential material.

## Suggested pull-request slices

1. Version-continuity decision, release examples, and unique-version preflight.
2. Installer target detection, checksum portability, and deterministic fixtures.
3. Multi-target build/package matrix and aggregate manifest/provenance.
4. Read-only validation and protected publication job split.
5. Draft round-trip verification, public smoke matrix, latest promotion, and
   recovery runbook.
6. Tag rules, release immutability, and retry/failure fixtures.

## Sprint demo

1. Open and merge a release-preparation PR containing the chosen version and
   compatibility statement.
2. Push a matching protected candidate tag.
3. Show validation, target builds, packaging, and candidate assembly completing
   without release-write permission.
4. Approve the protected publication stage.
5. Download the uploaded draft assets and show digest equality.
6. Publish without changing latest and run version-pinned installation tests on
   every supported native platform.
7. Promote the candidate to latest.
8. On a clean supported host, run:

```sh
curl -LsSf https://github.com/niparis/spire/releases/latest/download/install.sh | sh
spire --version
```

9. Show that an unsupported platform fixture fails before download and that the
   prior release can be restored as latest without changing tags or assets.

## Exit criteria

- Version continuity with the existing public release history is explicitly
  decided and reflected consistently in Cargo metadata, tags, validation, and
  documentation.
- Release validation and target builds run read-only before human approval.
- Only the protected publication and promotion jobs have release-write authority.
- Every supported OS/architecture mapping has a corresponding verified asset and
  native end-to-end installation test.
- `install.sh` detects the host, selects the correct supported archive and checksum
  tool, and fails before mutation for unsupported hosts.
- Draft assets are downloaded and verified before publication.
- Version-pinned public installation succeeds before the candidate becomes latest.
- The latest installer alias is verified after promotion.
- Failed candidates do not displace the prior known-good latest release.
- Published tags and assets are immutable, provenance is complete, and retry and
  recovery behavior have deterministic coverage.

## Evidence Sources

- [`../../install.sh`](../../install.sh), current Linux x86_64-only installer.
- [`../../.github/workflows/release.yml`](../../.github/workflows/release.yml),
  current single-job tag release workflow.
- [`../../scripts/release/package-release.sh`](../../scripts/release/package-release.sh),
  current single-target packager and metadata producer.
- [`../../scripts/release/verify-release.sh`](../../scripts/release/verify-release.sh),
  current numeric SemVer and archive verifier.
- [`11-release-artifacts-versioning-and-installation.md`](11-release-artifacts-versioning-and-installation.md),
  original release artifact and installation contract.
- Public GitHub release history and release-asset metadata, verified 2026-07-29.

## Unknown / Unverified

- Whether Linux ARM64 is a required operator platform and which pinned builder can
  produce and natively execute its musl binary.
- Whether both macOS architectures remain required, and the availability and cost
  of pinned native macOS runners.
- Whether native Windows distribution is required. If required, it needs a separate
  archive and PowerShell installer contract.
- Whether the repository's protected `release` environment, tag ruleset, and
  immutable-release setting are currently configured as documented.
- Whether GitHub artifact attestations or Sigstore signing should become a release
  requirement; SHA-256 checksums verify integrity but not publisher identity.
