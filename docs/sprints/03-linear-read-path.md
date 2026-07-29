# Sprint 03 — Linear Read Path and Dry-Run Reconciliation

**Last Verified:** 2026-07-28  
**Depends on:** Sprint 02  
**Unlocks:** Sprint 04

## Outcome

The orchestrator can fetch and normalize canonical Linear issues, evaluate
eligibility, follow pagination and rate limits, and reconcile relevant work into
SQLite without making any Linear write.

## Entry criteria

- `LinearPort` and domain DTOs exist.
- SQLite repositories, inbox/outbox, and reconciliation cursors work.
- Sprint 00 Linear fixtures and IDs are available.

## Work packages

### S03.1 Implement authentication and client construction

Implementation:

1. Load Linear credentials from the approved secret reference.
2. Construct the pinned `lineark-sdk` client inside `spire-adapters`.
3. Add explicit request timeout, user agent, tracing span, and response-size limit.
4. Redact authorization headers and issue descriptions from default logs.
5. Expose an adapter health diagnostic without making API availability a liveness
   dependency.

Verification:

- Invalid authentication maps to a typed adapter error.
- No credential appears in logs or serialized errors.

### S03.2 Define the canonical Linear issue projection

Normalize:

```text
issue ID and identifier
team ID
workflow-state ID
estimate
priority
labels
relations/blockers
description and acceptance-criteria evidence
assignee/creator metadata needed for conflict policy
createdAt and updatedAt
repository mapping input
```

Rules:

- SDK objects never enter the application layer.
- Missing optional values remain explicit `None`, not invented defaults.
- Preserve a canonical revision using `updatedAt` plus a content hash if needed.
- Treat the issue description as untrusted content.

Verification:

- Sprint 00 fixtures deserialize into stable domain DTOs.
- Unknown labels and new GraphQL fields do not break parsing.

### S03.3 Implement complexity normalization

Implementation:

1. Read the Linear estimate from the canonical issue.
2. Map it through configured `complexity_mapping`.
3. Return `complexity_missing` or `complexity_unmapped` as eligibility reasons.
4. Persist both the raw estimate and normalized class.
5. Do not treat complexity as priority.

Verification:

- Every configured estimate maps exactly once.
- A changed estimate updates an unclaimed observation but never mutates an active
  work item's snapshotted dispatch plan.

### S03.4 Implement repository and ticket-type mapping

Implementation:

1. Define allowlisted repository mapping from stable Linear metadata.
2. Require exactly one repository in the initial domain.
3. Normalize supported work types.
4. Reject `architecture`, `adr`, and `spike`.
5. Detect ambiguous or absent repository mapping.

Verification:

- Fixtures cover valid, missing, ambiguous, and disabled repositories.
- Mapping is configuration, not hard-coded issue-title parsing.

### S03.5 Implement eligibility evaluation

Evaluate all conditions:

- Ready-for-Agent status.
- Supported work type.
- Acceptance criteria present.
- Complexity mapped.
- Repository mapped and enabled.
- Blocking dependencies complete.
- No active local ownership.
- Ticket not locally terminal.
- Dispatch policy covers implementation and review.

Return a structured decision:

```text
eligible
ineligible(reason, operator_detail)
waiting_for_dependency
```

Verification:

- A table-driven test covers every individual failure and combinations.
- Eligibility is a pure application operation over canonical issue and local state.

### S03.6 Implement filtered pagination

Implementation:

1. Query only configured teams and relevant workflow states.
2. Follow cursors until completion.
3. Persist the last successful reconciliation watermark/cursor strategy.
4. Bound page size and request concurrency.
5. Record Linear request and complexity-limit headers.
6. Honor reset headers before issuing another request.

Verification:

- Multi-page fixtures produce each issue once.
- A rate limit pauses without advancing the cursor incorrectly.
- A page failure resumes from a safe point.

### S03.7 Implement read-only reconciliation

Processing:

1. Fetch relevant issue page.
2. Normalize each issue.
3. Upsert observed WorkItem and ticket revision.
4. Evaluate eligibility.
5. Record proposed action in a dry-run report.
6. Never insert a Linear-mutating outbox action.
7. Mark locally missing previously active issues for targeted canonical lookup
   before any state change.

Verification:

- Repeated reconciliation converges.
- Older webhook-shaped fixture cannot overwrite a newer canonical issue.
- Dry-run reports new eligibility, changed eligibility, and drift separately.

### S03.8 Add reconciliation CLI and audit report

Commands:

```text
spire linear get <issue>
spire linear reconcile --dry-run
spire linear explain <issue>
```

The explanation includes:

- Canonical revision.
- Status, estimate, complexity class, repository, and type.
- Every eligibility check.
- Matching dispatch rule IDs without starting work.
- Local orchestration state and conflicts.

Verification:

- Operator can explain why a ticket is or is not eligible without reading logs.
- Output supports JSON for automated comparison.

## Suggested pull-request slices

1. Client, authentication, normalization, and fixtures.
2. Complexity/repository mapping and eligibility.
3. Pagination, rate limits, reconciliation, and explain CLI.

## Sprint demo

Run read-only reconciliation against the disposable Linear team, show deterministic
eligibility explanations, then run it again to demonstrate zero drift and zero
Linear writes.

## Exit criteria

- Canonical issue normalization is fixture-tested.
- Complexity and eligibility decisions are explainable.
- Pagination and rate-limit recovery work.
- Reconciliation has run safely for several cycles in dry-run mode.
- No Linear mutation path is enabled.

## Evidence Sources

- [`../ai_harness_implementation.md`](../ai_harness_implementation.md), Linear,
  eligibility, event, and reconciliation sections.
- Sprint 00 Linear adapter fixtures, when completed.

## Unknown / Unverified

- Acceptance-criteria detection may need a stricter repository/team convention.
- Exact blocker semantics remain subject to the configured Linear workspace.

