# Sprint 04 — Scheduler, Dispatch, and Admission Control

**Last Verified:** 2026-07-28  
**Depends on:** Sprint 03  
**Unlocks:** Sprint 05

## Outcome

Eligible Linear work is ordered deterministically, evaluated against dispatch and
capacity rules, and claimed exactly once in SQLite. The scheduler creates durable
run intentions but still uses a fake harness runner.

## Entry criteria

- Linear dry-run reconciliation produces stable eligible WorkItems.
- Dispatch policy and complexity mapping validate.
- SQLite constraints and `BEGIN IMMEDIATE` Unit of Work are proven.

## Work packages

### S04.1 Implement scheduler wakeups

Wake sources:

- Relevant processed event.
- Reconciliation change.
- Run completion.
- Provider circuit reset.
- Operator retry.
- Thirty-second safety tick.

Implementation:

1. Funnel wake signals into one bounded Tokio channel.
2. Coalesce duplicate wakes.
3. Run one scheduler command loop.
4. Treat channel delivery as an optimization; the safety tick preserves liveness.
5. Stop accepting wakes during graceful shutdown.

Verification:

- A burst of 1,000 wakes does not create 1,000 concurrent scheduler loops.
- Dropping a wake delays but does not strand eligible work.

### S04.2 Implement deterministic eligible-ticket ordering

Order by:

1. Manual expedite flag.
2. Linear priority.
3. Oldest Ready-for-Agent timestamp.
4. Stable Linear identifier.

Rules:

- Complexity does not alter queue priority.
- Provider health does not reorder tickets; it changes whether a ticket can start.
- Equal inputs produce stable order across restarts.

Verification:

- Property tests prove total ordering.
- Reconciliation insertion order cannot affect selection.

### S04.3 Implement transactional concurrency accounting

Configurable pilot limits:

```yaml
total_active_harness_runs: 3
ai_initiated_active_harness_runs: 1
mutating_runs_per_repository: 1
active_runs_per_ticket: 1
cleanup_global: 1
```

Implementation:

1. Count only `starting` and `running` harness Runs.
2. Ensure AI-initiated active runs are a subset of total active runs.
3. Apply repository mutation and ticket exclusivity independently.
4. Do not count reconciliation or provider health probes as harness workloads.
5. Release capacity only after a run is confirmed terminal or absent.

Verification:

- Human roots can use remaining total slots while the AI slot is full.
- Human work never exceeds the total ceiling.
- Two claims racing for the last slot produce one winner.

### S04.4 Implement provider-health selection

Implementation:

1. Load candidate health by `(harness, model, credential_profile)`.
2. Evaluate the snapshotted implementation candidate list in order.
3. Persist all skip reasons.
4. Select the first healthy candidate.
5. With no healthy candidate, set WorkItem `waiting_for_provider`, persist
   `resume_state`, and consume no harness slot.
6. Schedule a wake at the earliest known circuit reset.

Verification:

- Preferred candidate outage selects the next configured candidate.
- Unknown reset produces alertable indefinite wait, not polling storm.
- Healthy recovery returns work to eligibility once.

### S04.5 Implement immutable dispatch-plan snapshot

At root claim persist:

- Ticket revision.
- Raw estimate and complexity class.
- Dispatch policy version.
- Implementation rule ID and ordered candidates.
- Review rule ID and ordered candidates.
- Candidate health evaluation.
- Provisional selected maker triplet.

Rules:

- Later configuration changes do not mutate the plan.
- Later estimate edits do not reroute the active work.
- Requeue after explicit cancellation creates a new plan.

Verification:

- Restart under policy version 2 preserves a version 1 WorkItem plan.
- Audit command reproduces why the maker was selected.

### S04.6 Implement atomic root claim

Transaction:

1. Begin serialized `BEGIN IMMEDIATE`.
2. Re-read WorkItem and canonical revision.
3. Re-evaluate structural eligibility.
4. Evaluate candidate health.
5. Count all capacity scopes.
6. Insert dispatch decision and `Run(status=starting)`.
7. Attach lease and `active_run_id`.
8. Insert Linear-claim outbox actions, but keep their delivery disabled.
9. Commit.

Verification:

- Duplicate event and reconciliation race create one Run.
- Full capacity leaves the ticket eligible, not failed.
- Database failure creates no half-claim.

### S04.7 Implement derivative-work admission

Support run lineage:

```text
initiator
trigger_kind
parent_run_id
root_run_id
```

Rules:

- Root Ready ticket and explicit operator retry are human-initiated.
- Review, correction, and automatic continuation are AI-initiated.
- New automatic continuation acquires both total and AI capacity.
- Infrastructure retries do not consume engineering rounds.

Verification:

- Lineage remains within one WorkItem.
- AI slot prevents an automatic chain from multiplying.
- Existing live process observation does not create another Run.

### S04.8 Add scheduler explain and dry-run commands

Commands:

```text
spire scheduler once --dry-run
spire scheduler explain <issue>
spire capacity show
```

Output includes:

- Queue position.
- Every capacity count.
- Rule and candidate evaluation.
- Claim/no-claim reason.
- Earliest provider retry time.

Verification:

- Dry run is side-effect free.
- Explanation matches the next real scheduling decision under unchanged state.

## Suggested pull-request slices

1. Wake loop and deterministic ordering.
2. Transactional concurrency and lineage.
3. Dispatch-plan snapshot and provider wait.
4. Atomic claim and explain tooling.

## Sprint demo

Load several fixture tickets, fill the AI slot, open a provider circuit, run the
scheduler, and demonstrate deterministic human-root selection, provider fallback,
and exactly one transactional claim.

## Exit criteria

- All capacity scopes are race-tested.
- Dispatch audits are immutable and explainable.
- No provider candidate produces durable `waiting_for_provider`.
- Claims are exactly-once at the local consistency boundary.
- Fake runner consumes run intentions without external effects.

## Evidence Sources

- [`../ai_harness_implementation.md`](../ai_harness_implementation.md), admission,
  atomic claim, dispatch, initiation, and scheduling sections.

## Unknown / Unverified

- Scheduler throughput at pilot volume is unmeasured; correctness takes precedence.
- Manual expedite representation remains an operator-interface decision.

