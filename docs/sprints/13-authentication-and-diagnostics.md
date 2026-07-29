# Sprint 13 — Authentication and Diagnostics

**Last Verified:** 2026-07-29
**Depends on:** Sprint 12 exit criteria
**Unlocks:** Sprint 14

## Outcome

The default runtime user can establish, inspect, rotate, and diagnose Linear and
GitHub service authentication while Spire reuses provider-native Codex, Claude
Code, Git, and SSH authentication. `spire doctor` evaluates the actual service
context and produces a redacted, actionable readiness report. No Linear project,
webhook, ticket, or GitHub repository is mutated.

## Entry criteria

- User-scoped paths and service execution survive logout/reboot.
- Configuration no longer requires harness credential references.
- The GitHub identity is a GitHub App, recorded in
  [`../decisions/github-app-identity.md`](../decisions/github-app-identity.md).
- Captured, redacted CLI fixtures define supported Codex and Claude Code
  authentication-status behavior.

## Target command surface

```text
spire auth status
spire auth login linear
spire auth login github
spire auth rotate linear
spire auth remove linear
spire doctor
spire doctor --format json
```

## Work packages

### S13.1 Define authentication and diagnostic ports

Add application-owned interfaces for:

- `SecretStorePort`;
- `ServiceAuthenticationProbePort`;
- `HarnessProbePort`;
- `GitTransportProbePort`;
- `ServiceContextProbePort`.

Define provider-neutral results:

```text
configured
authenticated
expired
permission_denied
unavailable
ambiguous
unsupported
```

Implementation rules:

1. Ports return capability and remediation data, never raw credentials.
2. Harness probes report executable, version, authentication state, supported
   model/effort evidence, and probe confidence.
3. Git probes distinguish local repository validity, fetch access, push evidence,
   default branch, remote identity, and ephemeral-agent risk.
4. Diagnostic aggregation is a pure application use case.

Verification:

- Application tests use deterministic fakes and contain no process or filesystem
  imports.
- An unknown provider response is `ambiguous`, never treated as authenticated.
- Diagnostic severity and exit status are table-tested.

### S13.2 Implement the user secret store

Use one Spire-managed, user-only `secrets.env` bundle below the resolved config
root for Linear and GitHub service credentials. Codex, Claude Code, and SSH
secrets do not enter this store.

Implementation:

1. Place the bundle below the resolved user configuration root.
2. Create it with mode `0600` and verify owner, type, and absence of symlinks on
   every read.
3. Parse a strict dotenv subset as data; never source or evaluate it as shell
   text.
4. Reject NULs, unbounded values, duplicate names, unknown keys, and malformed
   records.
5. Write rotations through fsync plus atomic rename.
6. Preserve the previous complete bundle if rotation fails.
7. Redact values from `Debug`, errors, tracing, crash reports, and JSON output.
8. Define a separate adapter for the advanced system installation rather than
   weakening user-store permissions.

Verification:

- Permission, owner, symlink, partial-write, and concurrent-rotation tests fail
  closed.
- A repository-wide secret sentinel never appears in logs or snapshots.
- Removing one credential cannot corrupt unrelated credentials.

### S13.3 Implement Linear authentication lifecycle

Implementation:

1. Accept a personal API key for the initial single-operator installation.
2. Reserve OAuth as a later adapter without changing the application port.
3. Prompt without echo and persist only after a successful `viewer`/organization
   probe.
4. Record non-secret identity metadata separately from secret material.
5. Make rotation verify the replacement before atomically activating it.
6. Keep the prior credential active when verification fails.
7. Support removal only when Spire is stopped or the resulting configuration
   remains explicitly non-ready.

Verification:

- Valid, invalid, revoked, rate-limited, and transport-failure fixtures map to
  distinct outcomes.
- Authentication tests retain no raw provider response containing secrets.
- Rotation during a read uses either the old or new complete credential.

### S13.4 Implement GitHub authentication lifecycle

Implementation:

1. Implement the GitHub App identity only. Authority, permissions, stored parts,
   and rotation behavior are defined in
   [`../decisions/github-app-identity.md`](../decisions/github-app-identity.md);
   do not restate them here.
2. Separate GitHub API authentication from Git transport/SSH authentication.
3. Verify app installation identity and granted permissions without mutating a
   repository.
4. Own installation-token minting and refresh inside the adapter; the private key
   is the durable secret and a short-lived installation token is never stored.
5. Register the App through the manifest flow; the operator never authors the
   private key or webhook secret by hand.
6. Report merge capability as an unsafe configuration; branch protection remains
   an independent enforcement boundary.

Verification:

- Expiry and refresh behavior use an injected clock.
- Token refresh races produce one active result and no corrupted cache.
- Missing PR/check/comment permissions are named individually.
- Authentication cannot call merge or force-push APIs.

### S13.5 Implement provider-native harness probes

Implementation:

1. Locate the configured Codex and Claude Code executable without invoking a shell.
2. Run only captured, approved version/authentication probes with bounded timeout,
   output-size limit, and redaction.
3. Execute as the configured runtime user with its normal home and provider state.
4. Do not inject Spire-managed harness credentials or copy provider auth files.
5. Validate configured model and effort against supported probe evidence when the
   CLI exposes it.
6. Mark undocumented or changed output as `ambiguous`.

Verification:

- Deterministic process fixtures cover authenticated, logged-out, missing,
  timeout, malformed, and unknown-version outcomes.
- Probe output cannot select a model or alter dispatch.
- No probe starts a mutating harness session.

### S13.6 Implement Git and SSH diagnostics

Implementation:

1. Inspect remotes using explicit Git argument vectors.
2. Normalize supported GitHub SSH and HTTPS remotes into `owner/repository`.
3. Use the runtime user's existing `~/.ssh/config`, keys, and agent.
4. Perform bounded, non-mutating `git ls-remote` verification.
5. Detect an SSH agent socket that is unavailable to the user service or unlikely
   to survive logout/reboot.
6. Treat push capability as unverified unless an approved non-mutating probe proves
   it; never claim push readiness from fetch success alone.

Verification:

- Fixtures cover SSH aliases, HTTPS remotes, multiple remotes, missing origin,
  moved repositories, host-key failure, agent-only access, and command timeout.
- Remote normalization rejects non-GitHub hosts unless a future adapter is
  explicitly added.
- No test contacts a live repository.

### S13.7 Implement `spire auth`

Implementation:

1. Add status, login, rotate, and remove commands.
2. Separate interactive prompting from application use cases through a prompt
   adapter.
3. Add `--format json` for status without accepting secrets through command-line
   arguments.
4. Require stdin/TTY or an explicit protected-file input for non-interactive
   secret installation.
5. Print identity, expiry, permissions, and remediation—not secret references.

Verification:

- Shell history and process arguments never contain credentials.
- Interrupted login leaves no partial active credential.
- JSON output is stable and redacted.
- Repeated status calls perform only bounded read/probe operations.

### S13.8 Implement `spire doctor`

Aggregate checks for:

- resolved paths and ownership;
- configuration and dispatch coverage;
- SQLite migrations and integrity;
- Linear and GitHub authentication;
- Codex and Claude Code executable/auth status;
- Git/SSH runtime context;
- service unit installation and active state;
- rollout kill switch.

Implementation:

1. Assign stable diagnostic codes and severity.
2. Support human and JSON output.
3. Exit nonzero when a required prerequisite is failed or ambiguous.
4. Keep external writes and harness execution disabled.
5. Include exact remediation commands when safe.

Verification:

- Golden reports cover ready, degraded, and blocked installations.
- A passing interactive-shell check cannot hide a failing service-context check.
- Output includes no ticket text, raw provider response, path outside the Spire
  installation, or secret material.

## Suggested pull-request slices

1. Authentication/diagnostic ports and secret-store adapter.
2. Linear and approved GitHub authentication lifecycle.
3. Harness and Git/SSH probes.
4. `spire auth`, `spire doctor`, and redaction/failure coverage.

## Sprint demo

On the target VM, authenticate Linear and GitHub, reuse existing Codex, Claude
Code, and SSH state from the login user, then run `spire doctor` from both the
interactive shell and user service context. Rotate a Linear credential, simulate
an expired GitHub credential and ephemeral SSH agent, and show redacted,
actionable results without any provider mutation.

## Exit criteria

- Service credentials can be installed and rotated without manual
  `credential_ref` editing.
- Harness authentication remains provider-native and works as the runtime user.
- GitHub API and Git/SSH identities are diagnosed separately.
- `spire doctor` fails closed on every required ambiguous state.
- No diagnostic or authentication operation mutates Linear or GitHub work data.
- Secret and process fixtures prove redaction and bounded execution.

## Evidence Sources

- [`../decisions/first-run-onboarding-and-project-mapping.md`](../decisions/first-run-onboarding-and-project-mapping.md)
- [`../decisions/github-app-identity.md`](../decisions/github-app-identity.md)
- [`../decisions/security-and-authority.md`](../decisions/security-and-authority.md)
- Sprint 00 provider fixtures and the target-VM runtime-user evidence.

## Unknown / Unverified

- Whether the merge endpoint requires `contents: write`, and whether an App
  review satisfies a required approving review.
- Whether GitHub accepts a loopback `redirect_url` in the App manifest flow.
- Stable Codex and Claude Code authentication-status command contracts.
- A portable non-mutating proof of Git push authority.
