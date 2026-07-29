# Sprint 08 — Independent Different-Harness Review

**Last Verified:** 2026-07-28  
**Depends on:** Sprint 07  
**Unlocks:** Sprint 09

## Outcome

A CI-green current PR revision receives a fresh, read-only review from a harness
different from the sticky maker. Findings route to bounded maker corrections; an
approval makes the PR human-ready but never merges it.

## Entry criteria

- Current head SHA and required CI are trustworthy.
- Both harness adapters and capacity circuits are operational.
- Review prompt/result contract and permissions are approved.

## Work packages

### S08.1 Implement review-cycle persistence

Fields:

```text
work_item_id
round
implementation_run_id
review_run_id
base_sha
head_sha
ci_state
review_state
published_comment_id
created_at
completed_at
```

Constraints:

- One active review cycle per WorkItem/head SHA.
- At most one successful approval per WorkItem/head SHA.
- New head invalidates older approval.

Verification:

- Duplicate CI-green events create one cycle.
- Old approval cannot be copied forward.

### S08.2 Resolve review dispatch from the snapshotted plan

Algorithm:

1. Load WorkItem's snapshotted review rule/candidates.
2. Filter out sticky maker harness.
3. Filter unhealthy/disabled candidates.
4. Persist every evaluation and selection.
5. If no candidate remains, set `waiting_for_provider`.
6. Acquire total and AI-initiated capacity.
7. Create AI-initiated child Run.

Verification:

- Configuration version change after root claim does not change reviewer candidates.
- Fallback never selects maker.
- Full AI slot queues review without losing CI-green evidence.

### S08.3 Build a fresh review context

Provide only:

- Ticket and acceptance criteria.
- Repository contracts/instructions.
- PR number, base SHA, and exact head SHA.
- Diff and relevant repository access.
- Required CI evidence.
- Structured review output schema.

Exclude:

- Maker conversation/session.
- Hidden reasoning or scratch state.
- Maker-specific provider artifacts.
- Write credentials.

Verification:

- Review run has a new provider session ID.
- Adapter invocation contains no maker session reference.

### S08.4 Enforce reviewer read-only authority

Implementation:

1. Use read-only worktree/repository permissions where practical.
2. Disallow push, PR mutation, and merge credentials.
3. Limit tools/commands to inspection and tests that do not mutate tracked files.
4. Detect worktree changes after review and fail the review contract.

Verification:

- Reviewer push attempt fails.
- Modified tracked file prevents approval and alerts integration failure.

### S08.5 Implement the review result contract

Result:

```text
verdict: approved | changes_required | blocked
reviewed_head_sha
summary
findings[]:
  stable_id
  severity
  file
  line?
  title
  rationale
  requested_change
```

Rules:

- `reviewed_head_sha` must equal requested/current SHA.
- Findings must be actionable and deduplicable.
- Missing/invalid structured result is an integration failure.
- Review cannot waive failed CI.

Verification:

- Schema and semantic validation tests cover malformed findings and wrong SHA.

### S08.6 Publish review evidence idempotently

Implementation:

1. Re-fetch PR head after reviewer finishes.
2. Mark stale if SHA changed.
3. For current result, publish a concise GitHub review summary/check and Linear
   comment using deterministic idempotency keys.
4. Persist external comment/check identifiers.
5. Avoid duplicating identical findings on retry.

Verification:

- Crash after publish converges.
- Stale result never appears as current approval.

### S08.7 Dispatch review corrections

On `changes_required`:

1. Increment engineering review round once.
2. Publish findings.
3. Create AI-initiated correction child Run.
4. Reuse sticky maker harness and preserved worktree/branch.
5. Include structured findings, not reviewer conversation.
6. Acquire total, AI, repository, and ticket capacity.
7. After push, invalidate old review and return to required CI.

Verification:

- Maker/checker roles never swap implicitly.
- Correction capacity wait does not consume another review round.
- Every new SHA passes CI before another review.

### S08.8 Enforce review limits and waiver

Initial policy:

```yaml
review_correction_cycles: 3
```

Implementation:

1. On exhausted limit, transition Blocked with unresolved findings.
2. Support an authenticated human waiver bound to exact head SHA, actor, reason, and
   timestamp.
3. Invalidate waiver on new SHA.
4. A waiver does not override failed CI.

Verification:

- Fourth requested-change cycle does not launch another maker.
- Unauthorized or stale waiver fails.

### S08.9 Mark human-ready without merging

Conditions:

- Current SHA unchanged.
- Required CI successful.
- Current-SHA review approved or valid human waiver.
- No active run.

Actions:

1. Record `human_ready`.
2. Publish concise status to Linear/GitHub.
3. Release all harness capacity.
4. Wait for human merge.

Verification:

- No orchestrator/harness credential invokes merge.
- Merge event is the only automatic path to Done.

## Suggested pull-request slices

1. Review-cycle model and dispatch.
2. Fresh/read-only reviewer execution and result schema.
3. Evidence publishing and maker correction.
4. Limits, waiver, and human-ready gate.

## Sprint demo

Drive one CI-green SHA through a different-harness review, request a change, run a
sticky-maker correction, pass CI again, receive a fresh approval, and stop at a
human-ready draft PR.

## Exit criteria

- Same-harness review is impossible.
- Reviewer context and credentials are isolated.
- Every result is bound to current head SHA.
- Correction loops are bounded and concurrency-controlled.
- Approval never merges.

## Evidence Sources

- [`../ai_harness_architecture.md`](../ai_harness_architecture.md), maker/checker
  decisions.
- [`../ai_harness_implementation.md`](../ai_harness_implementation.md), review
  lifecycle, result, idempotency, and waiver design.

## Unknown / Unverified

- Exact GitHub representation of the AI judgment gate remains an implementation
  choice: check run, review, or both.
- Review waiver UX and authorized actor list require operator approval.

