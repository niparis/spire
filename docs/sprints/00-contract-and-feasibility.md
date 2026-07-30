# Sprint 00 — Contract and Feasibility Spikes

**Last Verified:** 2026-07-28  
**Depends on:** Architecture and implementation documents  
**Unlocks:** Sprint 01

## Outcome

Replace the highest-risk assumptions with executable evidence before building the
orchestrator. This sprint produces decisions, redacted fixtures, and minimal spike
programs; it does not create production automation or mutate real tickets.

## Entry criteria

- A Linear workspace, one test team, and one disposable test issue are available.
- Codex and Claude Code can be invoked on the target operating system.
- The homelab VM distribution and systemd version are known.
- A test Git repository is available.

## Work packages

### S00.1 Record the external identity map

Implementation:

1. Record Linear organization ID, team ID, workflow-state IDs, estimate scale, and
   bot identity.
2. Record the pilot GitHub repository, base branch, required check names, and
   installation identity.
3. Record the Cloudflare account/zone owner and proposed webhook hostname.
4. Store secrets only as secret-manager/systemd credential references; documentation
   records names, never values.

Artifacts:

- `docs/decisions/external-identities.md`
- Example redacted configuration file.

Verification:

- IDs resolve through read-only API calls.
- Workflow-state IDs belong to the configured team.
- The estimate scale includes every value authors may select.

### S00.2 Define complexity classes and dispatch policy version 1

Implementation:

1. Map each actual Linear estimate to a stable class such as `small`, `medium`,
   `large`, or `xlarge`.
2. Define implementation and review candidates for every class.
3. Specify exact provider model IDs and supported effort values.
4. Ensure each implementation candidate has at least one possible review candidate
   with a different harness.
5. Decide whether same-harness model fallback is permitted after maker launch.
6. Assign `policy_version: 1`; do not use implicit defaults.

Artifacts:

- `config/spire.example.yaml`
- `docs/decisions/dispatch-policy-v1.md`
- Table of every `(role, complexity)` input and expected ordered candidates.

Verification:

- No input matches zero or multiple rules.
- Every model/effort pair is accepted by the relevant installed CLI.
- Removing either provider produces an explicit validation failure for unsupported
  maker/checker combinations.

### S00.3 Prove the Linear Rust SDK boundary

Implementation:

1. Create a disposable Rust spike using a pinned `lineark-sdk` version.
2. Fetch one issue including ID, identifier, status, estimate, labels, relations,
   assignee, timestamps, and description.
3. Query relevant issues with filters and pagination.
4. In the disposable test team only, prove status transition and comment creation.
5. Capture GraphQL errors, rate-limit headers, and pagination behavior.
6. Identify any operation that requires raw GraphQL rather than the SDK.

Artifacts:

- Redacted request/response fixtures under `tests/fixtures/linear/`.
- ADR choosing SDK calls versus raw GraphQL per operation.

Verification:

- Fixtures deserialize without network access.
- A duplicate comment test identifies whether Linear offers a native idempotency
  facility; otherwise document local idempotency behavior.

### S00.4 Capture Codex execution and capacity contracts

Implementation:

1. Run a no-op repository task through `codex exec --json` with an output schema.
2. Record event ordering, terminal event, exit status, session/run identifier, token
   usage fields, and final structured output.
3. Prove `exec resume` against a preserved session.
4. Trigger safe failures: invalid model, invalid auth profile, malformed output,
   cancellation, timeout, and unavailable network.
5. If practical without spending unexpected credit, observe a real usage-limit
   boundary; otherwise retain the case as an unverified fixture requirement.
6. Redact tokens, paths, usernames, repository remotes, and prompt content.

Artifacts:

- `tests/fixtures/harness/codex/*.jsonl`
- `docs/decisions/codex-adapter-contract.md`

The capture procedure is
[`../runbooks/harness-fixture-capture.md`](../runbooks/harness-fixture-capture.md).

Verification:

- Each captured fixture maps to exactly one normalized outcome.
- Unknown events remain parseable and do not imply success.
- An active turn is not canceled solely because future-start capacity is exhausted.

### S00.5 Capture Claude Code execution and capacity contracts

Implementation:

1. Run a no-op task with `claude -p --output-format stream-json`, explicit model,
   effort, and structured output.
2. Record `system/init`, session ID, result subtype, structured error, HTTP status,
   usage, and terminal process status.
3. Prove session resume.
4. Capture invalid model, authentication, rate-limit-shaped, output-limit,
   cancellation, and malformed-result cases.
5. Confirm whether usage-limit reset timestamps appear in structured fields or only
   messages.
6. Do not enable `--fallback-model`; fallback belongs to dispatch policy.

Artifacts:

- `tests/fixtures/harness/claude/*.jsonl`
- `docs/decisions/claude-adapter-contract.md`

The capture procedure is
[`../runbooks/harness-fixture-capture.md`](../runbooks/harness-fixture-capture.md).

Verification:

- `rate_limit`, `billing_error`, `max_output_tokens`, server, auth, and unknown
  outcomes remain distinct.
- Resume uses the recorded session without reusing hidden maker context for review.

### S00.6 Prove systemd transient-run recovery

Implementation:

1. Start a harmless long-running command as a transient unit named from a fake run
   UUID.
2. Query unit state and main PID.
3. Restart the spike controller and rediscover the unit.
4. Stop the unit gracefully, then force termination after a deadline.
5. Confirm stdout/stderr capture location and retention.
6. Define the safe unit-name encoding and allowed environment/credential injection.

Artifacts:

- `docs/decisions/systemd-runner-contract.md`
- Redacted sample unit properties.

Verification:

- Controller restart never launches a duplicate process.
- A missing unit is distinguishable from a finished unit.
- The command cannot escape its configured working directory or credential scope.

### S00.7 Prove SQLite and filesystem assumptions on the VM

Implementation:

1. Confirm the database path is a local filesystem.
2. Run WAL with concurrent readers and short `BEGIN IMMEDIATE` writes.
3. Exercise online backup and restore.
4. Measure behavior during abrupt process termination.
5. Confirm database, WAL, backups, and worktrees are on explicitly separate cleanup
   paths.

Artifacts:

- `docs/decisions/sqlite-operating-contract.md`
- Recorded filesystem type and backup destination.

Verification:

- `PRAGMA integrity_check` succeeds after restore.
- A forced process kill does not corrupt the database.
- Cleanup test cannot select the database path.

### S00.8 Close security and authority decisions

Decide and record:

- GitHub App versus bot token. Closed: GitHub App, recorded in
  [`../decisions/github-app-identity.md`](../decisions/github-app-identity.md).
- Whether the harness pushes directly or a mechanical publisher pushes.
- Exact branch/PR permissions for maker and read-only permissions for checker.
- Webhook secrets and harness credentials storage.
- Admin endpoint access: loopback only or Cloudflare Access.
- Review-waiver authority and required audit record.
- Human status-change conflict policy.

Verification:

- No credential in the proposed design can merge.
- Reviewer credentials cannot push.
- Webhook credentials cannot start arbitrary local commands.

## Suggested pull-request slices

1. Decision records and redacted configuration schema.
2. Linear SDK spike and fixtures.
3. Codex/Claude fixtures and normalized-outcome matrix.
4. systemd/SQLite VM spike evidence.

## Sprint demo

Run all fixture parsers offline, show the complete dispatch matrix, start and
rediscover a transient unit, and restore a SQLite backup. No production ticket or
repository is mutated.

## Exit criteria

- Every P0 blind spot needed by Sprints 01–05 is resolved or explicitly blocked.
- Dispatch policy version 1 has no coverage gaps.
- Both harnesses have captured successful structured output.
- Capacity errors have a versioned normalized taxonomy.
- SQLite and systemd assumptions hold on the target VM.
- Credential and merge boundaries are approved.

## Evidence Sources

- [`../ai_harness_implementation.md`](../ai_harness_implementation.md), especially
  dispatch, harness capacity, persistence, and blind spots.
- Local CLI help observations recorded in the implementation document.
- No production integration has been verified yet.

## Unknown / Unverified

- Real quota exhaustion may be impractical to force safely; if so, the adapter must
  initially fail closed on unseen variants.
- Exact identities, model IDs, and repository policies require operator input.

