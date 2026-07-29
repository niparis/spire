# Release promotion and recovery

## Authority and version decision

The named release owner is the only person who creates a release tag, approves the
protected `release` environment, or changes GitHub's `latest` release pointer.
`v1.0.3` was the retired Go CLI. The Rust orchestrator therefore starts the separate,
incompatible `v2.0.0-rc.1` line; this is a SemVer release candidate, not a
backwards-compatible 1.x patch.

No workflow or harness creates a tag, changes repository rules, enables release
immutability, or merges code. Do not place a GitHub token in commands, logs, or
metadata; `gh` obtains its authenticated session from the release-owner environment.

## Prepare and promote a release

1. In a release-preparation pull request, update `Cargo.toml` and `Cargo.lock`
   together. State compatibility and supported-platform changes explicitly.
2. Run the canonical gates and `./scripts/release/test-release-contract.sh`.
3. After the pull request merges, preflight the intended tag before creating it:

   ```sh
   ./scripts/release/preflight-release.sh v2.0.0-rc.1 <merged-commit-sha>
   ```

   Run again with `SPIRE_PREFLIGHT_REMOTE=1` from an authenticated release-owner
   checkout to reject an existing remote tag or GitHub Release.
4. Create the matching protected tag on that exact merged commit and push it. The
   tag workflow validates and builds with `contents: read`, emits one immutable
   candidate artifact, and only then waits for `release` approval.
5. Approve publication. The workflow creates or resumes only the candidate draft,
   uploads the complete asset set, downloads it through GitHub, and compares every
   name, size, and SHA-256 digest before publishing it with `latest=false`.
6. Confirm public, version-pinned installer smoke tests pass on Linux x86_64, macOS
   Intel, and macOS Apple Silicon. Only then approve latest promotion. The workflow
   verifies `releases/latest/download/install.sh` and its installed binary version.

Each release contains exactly one archive for each supported target plus
`SHA256SUMS`, `build-metadata.json`, and `install.sh`. Metadata records the source
commit, lockfile digest, Rust toolchain, per-target archive digest, and workflow URL.

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

## Required GitHub configuration

The release owner configures these repository settings before the first stable
promotion:

- A tag ruleset for `v*` that limits creation to release owners and forbids deletion
  or force updates.
- The protected `release` environment with required reviewers. It is attached only
  to the publication and latest-promotion jobs.
- GitHub release immutability, enabled after a disposable candidate has demonstrated
  draft retry, publication, and recovery behavior.

Record the ruleset URL, environment reviewers, disposable-candidate evidence, and
the prior/new latest tags in the release ticket. These settings are external GitHub
configuration, so they are intentionally not changed by repository code.
