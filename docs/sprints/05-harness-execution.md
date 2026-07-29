# Sprint 05 — Workspace and Harness Execution

**Last Verified:** 2026-07-28  
**Depends on:** Sprint 04  
**Unlocks:** Sprint 06

## Outcome

The orchestrator can allocate a safe worktree, start Codex or Claude Code in a
recoverable systemd transient unit, collect a schema-validated result, classify
capacity failures, and recover after restart. Starts remain manual/admin-triggered.

## Entry criteria

- Sprint 00 captured provider fixtures and systemd behavior.
- Scheduler creates durable run intentions and leases.
- Credential and branch-authority decisions are approved.

## Work packages

### S05.1 Implement safe workspace allocation

Implementation:

1. Derive branch and directory names from validated identifiers.
2. Resolve and verify all paths under the configured repository workspace root.
3. Create a per-run ownership marker containing WorkItem and Run IDs.
4. Create/update the base clone and allocate an isolated worktree.
5. Reject dirty/unowned paths.
6. Record base SHA, branch, path, and lifecycle state in SQLite.

Verification:

- Path traversal and symlink-escape tests fail closed.
- Concurrent allocation cannot reuse a branch/path.
- Cleanup cannot target the repository cache or database.

### S05.2 Define provider-neutral run input/output schemas

Run input includes:

- Run identity and lineage.
- Selected dispatch decision.
- Ticket URL/revision/complexity.
- Repository, branch, worktree, and base SHA.
- Role-specific PR/CI/review evidence.
- Deadline and permission mode.

Output schemas distinguish:

```text
implementation: pr_ready | specs_needed | blocked | no_change | task_failed
review: approved | changes_required | blocked | task_failed
capacity: rate_limited | quota_exhausted | context_exhausted | output_limit
integration: auth_failed | model_unavailable | runner_unhealthy |
             contract_invalid | unknown_provider_failure
```

Verification:

- Provider prose cannot directly advance lifecycle state.
- Unknown and malformed results retain raw redacted evidence.

### S05.3 Implement the systemd transient runner

Implementation:

1. Create unit name `spire-run-<run-id>.service`.
2. Set working directory, environment allowlist, credentials, timeout, and resource
   controls.
3. Capture stdout/stderr or provider JSONL in a per-run evidence path.
4. Implement start, inspect, cancel, and collect operations.
5. On cancellation, send graceful stop then force kill after configured grace.
6. Record systemd unit and PID metadata.

Verification:

- Service restart rediscovers running units.
- Repeated start for the same Run does not launch twice.
- Cancellation is idempotent.

### S05.4 Implement Codex adapter

Implementation:

1. Translate selected model/profile and sandbox policy to explicit CLI arguments.
2. Invoke `codex exec --json` with an output schema.
3. Persist external session/run identifier.
4. Parse versioned JSONL fixtures and live successful output.
5. Implement resume for same-harness continuations.
6. Disable implicit/unknown model selection.
7. Map capacity and integration failures to the normalized taxonomy.

Verification:

- Every Sprint 00 Codex fixture has a contract test.
- Unknown event versions fail closed.
- Active-turn limit observation opens future-start circuit without killing the run.

### S05.5 Implement Claude Code adapter

Implementation:

1. Invoke non-interactive stream JSON with explicit model, effort, permission mode,
   and result schema.
2. Do not enable opaque `--fallback-model`.
3. Persist session ID and structured result subtype.
4. Parse structured error, HTTP status, usage, and stop reason.
5. Implement same-harness resume.
6. Map output limit separately from account quota.

Verification:

- Every Sprint 00 Claude fixture has a contract test.
- `billing_error`, `rate_limit`, `max_output_tokens`, auth, server, and unknown remain
  distinguishable.

### S05.6 Implement provider circuit breakers

Implementation:

1. Scope health to `(harness, model, credential_profile)`.
2. Open circuit from normalized refusal/failure.
3. Store reason, first/last occurrence, retry time, and raw evidence reference.
4. Wake scheduler at a known reset.
5. For unknown reset, use a bounded low-frequency probe and operator alert.
6. Close only after a successful probe/start or explicit operator action.

Verification:

- One credential failure does not disable unrelated candidates.
- Repeated refusal cannot create a fast restart loop.
- Health survives orchestrator restart.

### S05.7 Implement pre-start fallback

Algorithm:

1. Reserve a provisional slot and create immutable Run.
2. Start selected candidate.
3. If provider accepts, record external ID and make maker harness sticky.
4. If recognized capacity refusal occurs before acceptance:
   - mark Run `capacity_rejected`;
   - release slot;
   - open circuit;
   - create a new dispatch decision/Run for at most the next candidate.
5. If candidates are exhausted, set WorkItem `waiting_for_provider`.
6. Unknown failure stops and alerts.

Verification:

- Each candidate is attempted at most once per scheduling decision.
- Sticky maker is never set for a rejected pre-start attempt.
- Candidate history remains auditable.

### S05.8 Implement mid-run exhaustion and continuation

Implementation:

1. Confirm the prior external process is terminal.
2. Mark original Run `capacity_exhausted`.
3. Preserve worktree, branch, evidence, and provider session ID.
4. Set WorkItem `waiting_for_provider` with resume state.
5. When capacity returns, create an AI-initiated child Run.
6. Acquire total, AI, repository, and ticket capacity.
7. Resume or create fresh context using the same sticky harness.
8. Require operator approval for cross-harness reassignment.

Verification:

- Continuation consumes AI capacity but no engineering correction round.
- A live prior process prevents continuation.
- Reassignment cannot happen through configuration change alone.

### S05.9 Implement run monitoring and timeout

Implementation:

1. Renew lease while systemd/provider state is healthy.
2. Record heartbeat and meaningful state changes.
3. Apply role-specific absolute deadlines.
4. On lease expiry, inspect before declaring lost.
5. On timeout, cancel once and preserve evidence.
6. Normalize terminal results transactionally.

Verification:

- Controller kill/restart resumes monitoring.
- Missing unit becomes `lost`; finished unit collects result.
- Timeout cannot race into duplicate cancellation.

### S05.10 Run one manual end-to-end harness task

Use an explicitly invoked admin command:

```text
spire runs start-manual <fixture-ticket> --dry-linear --dry-github
```

The task may edit a disposable repository/worktree, but cannot mutate Linear or
GitHub.

Verification:

- Structured result, commits, logs, token usage, and worktree state are retained.
- Both Codex and Claude complete at least one fixture task.

## Suggested pull-request slices

1. Workspace and transient runner.
2. Provider-neutral schemas and Codex adapter.
3. Claude adapter and circuit breakers.
4. Fallback, continuation, monitoring, and manual demo.

## Sprint demo

Start one task per harness, restart the orchestrator during one run, trigger a
fixture capacity refusal, show fallback before start, and show same-harness
continuation after a terminal context/capacity event.

## Exit criteria

- Both adapters pass fixture and live success tests.
- systemd recovery prevents duplicate processes.
- Provider capacity is distinct from engineering failure.
- Sticky-maker and continuation rules are enforced.
- No automatic Linear/GitHub writes or scheduler-initiated harness starts are enabled.

## Evidence Sources

- [`../ai_harness_implementation.md`](../ai_harness_implementation.md), harness
  contract, capacity, runner, retry, and workspace sections.
- Sprint 00 provider/systemd fixtures, when completed.

## Unknown / Unverified

- Exact real quota signals remain limited to captured fixtures.
- Harness token usage fields may differ by authentication mode.

