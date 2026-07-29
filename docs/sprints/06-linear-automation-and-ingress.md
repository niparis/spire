# Sprint 06 — Linear Automation, Webhooks, and Cloudflare Ingress

**Last Verified:** 2026-07-28  
**Depends on:** Sprint 05  
**Unlocks:** Sprint 07

## Outcome

Signed Linear webhook events reach the homelab service through Cloudflare Tunnel,
are durably ingested, and converge with reconciliation. The orchestrator may claim
one allowlisted ticket and project lifecycle outcomes back to Linear.

## Entry criteria

- Manual harness execution and recovery are proven.
- Linear read-only reconciliation has been stable.
- Webhook hostname, secret, workflow IDs, and bot identity are approved.
- Pilot team and disposable repository are allowlisted.

## Work packages

### S06.1 Harden the webhook HTTP boundary

Implementation:

1. Add a dedicated public webhook router.
2. Enforce method, path, body-size, and content-type limits.
3. Read the raw body once.
4. Validate Linear signature with constant-time comparison.
5. Validate timestamp/replay window.
6. Extract delivery ID and event type.
7. Commit inbox row before returning success.
8. Complete within Linear's response deadline; do no canonical fetch inline.

Verification:

- Valid, invalid signature, stale timestamp, oversized body, malformed JSON, and
  duplicate delivery cases are covered.
- Logs do not contain secret or full issue body.

### S06.2 Deploy Cloudflare Tunnel

Implementation:

1. Create remotely managed tunnel and least-privilege token.
2. Route only the public webhook hostname/path to loopback Axum origin.
3. Install `cloudflared` as a systemd service.
4. Keep admin API on loopback or separate protected Access hostname.
5. Configure service ordering and restart policy.
6. Document DNS, credential rotation, health check, and removal.

Verification:

- Public webhook path is reachable over HTTPS.
- Admin endpoints are not publicly reachable.
- VM has no inbound public firewall opening.
- Tunnel restart does not affect local scheduler correctness.

### S06.3 Implement asynchronous webhook processing

Implementation:

1. Lease an inbox row.
2. Parse the normalized envelope.
3. Fetch canonical issue state through `LinearPort`.
4. Upsert current WorkItem revision.
5. Evaluate lifecycle/eligibility.
6. Commit state and outbox actions.
7. Mark inbox processed atomically.

Verification:

- Duplicated, delayed, and reordered events converge.
- Self-generated Linear events are harmless.
- Handler crash is recovered from inbox state.

### S06.4 Implement Linear mutation adapter

Operations:

- Conditional workflow transition.
- Idempotent comment publication.
- Targeted canonical issue refetch.

Rules:

- Use workflow-state IDs, not names.
- Re-read current issue before overwriting a human-visible state.
- Apply the approved human-conflict policy.
- Comments include stable Run ID and concise status, never secrets/provider raw
  output.

Verification:

- Replayed outbox action creates one visible effect.
- Unexpected current state prevents overwrite and creates operator notification.

### S06.5 Enable the root claim projection

Sequence:

1. Scheduler creates local claim and outbox.
2. Outbox transitions Ready to In Progress.
3. Outbox posts claim comment with Run ID.
4. After both effects are confirmed or reconciled, WorkItem becomes queued.
5. Scheduler starts the selected harness.

Rules:

- Harness never starts before Linear claim confirmation.
- Temporary Linear outage reserves local claim but starts no invisible work.
- Claim failure safely returns to eligible only after canonical state permits.

Verification:

- Crash in every step converges without duplicate run.
- Human state change during claim follows conflict policy.

### S06.6 Project implementation terminal outcomes

Map:

- `specs_needed` → Specs Needed plus actionable questions.
- `blocked` → Blocked plus preserved evidence reference.
- `pr_ready` → In Review after PR existence is later confirmed.
- `task_failed` → retry or Blocked according to policy.
- Capacity wait → retain In Progress/In Review based on resume phase.

Verification:

- Informal provider prose cannot choose the Linear status.
- Projection is idempotent and explainable.

### S06.7 Reconcile webhook configuration and missed events

Implementation:

1. Audit configured webhook ID and enabled status daily.
2. Alert on disabled/missing webhook.
3. Retain 10-minute relevant-ticket reconciliation.
4. Compare webhook and reconciliation changes through the same application use case.

Verification:

- Stop tunnel beyond retry window, recover it, then reconcile missed state.
- Webhook and reconciliation never create different lifecycle decisions.

### S06.8 Restrict rollout

Feature gates:

- One Linear team.
- One repository.
- One work type (`chore`).
- One active harness run during initial live verification.
- Explicit operator enable flag.

Verification:

- Non-allowlisted tickets remain untouched and explain why.
- Kill switch stops new admission but continues monitoring/recovery.

## Suggested pull-request slices

1. Signed ingress and inbox processing.
2. Cloudflare/systemd deployment.
3. Linear mutation/outbox adapter.
4. Claim/outcome projection and restricted rollout.

## Sprint demo

Move one disposable ticket to Ready, receive its signed webhook through Cloudflare,
claim it exactly once, run the selected harness, and project a controlled terminal
outcome. Then disable ingress and prove reconciliation repairs a missed change.

## Exit criteria

- Public ingress passes security and replay tests.
- Claim ordering prevents invisible work.
- Linear writes are idempotent and conflict-aware.
- Webhook and reconciliation converge.
- Rollout gates and kill switch work.

## Evidence Sources

- [`../ai_harness_implementation.md`](../ai_harness_implementation.md), webhook,
  Linear write, deployment, outbox, and reconciliation sections.

## Unknown / Unverified

- Final hostname and Cloudflare Access choice require operator configuration.
- Linear webhook administration capabilities in `lineark-sdk` may require raw
  GraphQL or manual setup.

