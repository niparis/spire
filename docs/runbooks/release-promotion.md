# Release promotion and recovery

**Status:** Repository automation is implemented. The GitHub `release` environment,
required reviewers, tag ruleset, and release-immutability setting are external
operator configuration and must be verified before the first tag is pushed.

**Scope:** This is the canonical procedure for preparing, tagging, publishing,
verifying, promoting, and recovering a Spire GitHub Release.

## Authority and version decision

The named release owner is the only person who creates a release tag or approves
the protected `release` environment. After that approval, the workflow alone may
change GitHub's `latest` release pointer, and only after the published candidate
passes every public installer smoke test. `v1.0.3` was the retired Go CLI. The Rust
orchestrator therefore starts the separate, incompatible `v2.0.0-rc.1` line; this
is a SemVer release candidate, not a backwards-compatible 1.x patch.

No workflow or harness creates a tag, changes repository rules, enables release
immutability, or merges code. Do not place a GitHub token in commands, logs, or
metadata; `gh` obtains its authenticated session from the release-owner environment.

## Preconditions

Complete these checks before creating a tag:

1. The release-preparation pull request is merged and remote `main` is green.
2. Root `Cargo.toml` and `Cargo.lock` contain the intended version.
3. No tag or GitHub Release already uses that version.
4. The GitHub `release` environment exists and names its required reviewers.
5. A `v*` tag ruleset limits tag creation and prevents deletion or force updates.
6. The release owner is authenticated with `gh auth status`.
7. The release is operated from a clean checkout of remote `main`, not from an
   unmerged feature branch or a worktree containing unrelated changes.

The current first Rust-orchestrator release is `v2.0.0-rc.1`. Substitute the
version selected by the release-preparation pull request for later releases.

## Prepare the merged revision

Update a clean release-owner checkout:

```sh
git fetch origin main
git switch main
git pull --ff-only origin main
git status --short
```

`git status --short` must produce no output. Record the immutable inputs:

```sh
release_tag=v2.0.0-rc.1
release_sha="$(git rev-parse HEAD)"
printf 'tag=%s sha=%s\n' "${release_tag}" "${release_sha}"
```

Run the canonical repository and release gates:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
./scripts/release/test-release-contract.sh
./scripts/release/test-release-promotion.sh
```

Run both local and authenticated remote preflight checks:

```sh
./scripts/release/preflight-release.sh "${release_tag}" "${release_sha}"
SPIRE_PREFLIGHT_REMOTE=1 \
  ./scripts/release/preflight-release.sh "${release_tag}" "${release_sha}"
```

Stop if the checkout is dirty, a gate fails, the tag does not match Cargo metadata,
or the remote preflight finds an existing tag or release.

## Create the release tag

Create an annotated tag on the exact preflighted commit and push only that tag:

```sh
git tag -a "${release_tag}" "${release_sha}" -m "release: ${release_tag}"
git push origin "refs/tags/${release_tag}"
```

Never move or recreate the tag after it is pushed. A transient runner or GitHub
failure may rerun the same tag workflow against the same source and candidate
bytes. A repository defect that requires a source change consumes that version;
fix it through a new release-preparation pull request and a new version.

The push starts `.github/workflows/release.yml`. Confirm that the expected run uses
the recorded tag and SHA:

```sh
gh run list \
  --workflow release.yml \
  --event push \
  --limit 10
```

## Build, approve, publish, and promote

1. The tag workflow validates the tag and source commit, runs the canonical gates,
   and builds each supported target with `contents: read`.
2. It assembles one candidate containing the target archives, `SHA256SUMS`,
   `build-metadata.json`, and `install.sh`.
3. Inspect the validation, native build, and candidate-assembly jobs. Do not approve
   publication if a required job is skipped, cancelled, or failed.
4. Approve the protected `release` environment for the publication job. The
   workflow creates or resumes only the matching draft, uploads the exact candidate,
   downloads every remote asset, and verifies names, sizes, and SHA-256 digests.
5. The workflow publishes the verified candidate with `latest=false`.
6. Public, version-pinned installer smoke tests run on Linux x86_64, macOS Intel,
   and macOS Apple Silicon.
7. After all public smoke tests pass, the workflow automatically changes the
   latest pointer, downloads
   `releases/latest/download/install.sh`, installs into a temporary directory, and
   verifies the installed version. If this verification fails, it restores the
   previous latest pointer.

In summary, the workflow:

1. Validates and builds with read-only repository authority.
2. Waits for human publication approval.
3. Publishes a non-latest candidate and verifies it publicly.
4. Automatically promotes and verifies the latest alias after smoke tests pass,
   restoring the prior pointer on failure.

Each release contains exactly one archive for each supported target plus
`SHA256SUMS`, `build-metadata.json`, and `install.sh`. Metadata records the source
commit, lockfile digest, Rust toolchain, per-target archive digest, and workflow URL.

## Verify the published release

After the workflow completes, confirm release state and assets:

```sh
gh release view "${release_tag}" \
  --json tagName,isDraft,isPrerelease,publishedAt,assets,url
gh release list --limit 1 \
  --json tagName,isLatest,isDraft,isPrerelease,publishedAt
```

The release view must report the intended tag as published and not a draft. The
first release-list entry must report the same tag with `"isLatest": true`. The
expected assets are:

```text
spire-<tag>-x86_64-unknown-linux-musl.tar.gz
spire-<tag>-x86_64-apple-darwin.tar.gz
spire-<tag>-aarch64-apple-darwin.tar.gz
SHA256SUMS
build-metadata.json
install.sh
```

Verify the version-pinned installer independently before relying on `latest`:

```sh
installer_directory="$(mktemp -d)"
curl --fail --location --silent --show-error \
  "https://github.com/niparis/spire/releases/download/${release_tag}/install.sh" \
  --output "${installer_directory}/install.sh"
SPIRE_VERSION="${release_tag}" \
SPIRE_BIN_DIR="${installer_directory}/bin" \
  sh "${installer_directory}/install.sh"
"${installer_directory}/bin/spire" --version
```

Then verify the public convenience path:

```sh
curl -LsSf \
  https://github.com/niparis/spire/releases/latest/download/install.sh |
  SPIRE_BIN_DIR="${installer_directory}/latest-bin" sh
"${installer_directory}/latest-bin/spire" --version
```

Both commands must report the release version. A 404 from the latest installer means
that the expected release was not promoted, the latest pointer still names an older
release without `install.sh`, or publication did not attach the complete asset set.

## Recovery

A failed candidate remains a draft or a published non-latest release. Never replace
an asset or move a tag. An identical retry is acceptable only after the workflow
downloads and verifies all remote bytes; a different byte fails closed.

If latest-alias verification fails, restore the prior known-good tag immediately:

```sh
gh auth status
./scripts/release/restore-latest.sh <prior-known-good-tag>
```

This changes only GitHub's latest pointer. It does not modify either tag or release
asset set. For a defect found after promotion, restore the previous latest pointer,
open a new release-preparation pull request, and cut a new tag.

Do not delete or move a failed tag to make a retry appear successful. Record the
failed workflow URL and consumed version in the release ticket. Correct repository
code through a pull request, select a new version, and repeat this runbook from
preflight.

## Required GitHub configuration

The release owner configures these repository settings before creating the first
tag for this workflow:

- A tag ruleset for `v*` that limits creation to release owners and forbids deletion
  or force updates.
- The protected `release` environment with required reviewers. It is attached only
  to the publication job; successful public smoke tests authorize automatic latest
  promotion.
- GitHub release immutability, enabled after a disposable candidate has demonstrated
  draft retry, publication, and recovery behavior.

Record the ruleset URL, environment reviewers, disposable-candidate evidence, and
the prior/new latest tags in the release ticket. These settings are external GitHub
configuration, so they are intentionally not changed by repository code.
