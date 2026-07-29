# Sprint 07 — GitHub and Required CI

**Last Verified:** 2026-07-28  
**Depends on:** Sprint 06  
**Unlocks:** Sprint 08

## Outcome

The orchestrator resolves the pull request created by a maker, tracks its exact head
SHA, evaluates required GitHub checks canonically, invalidates stale evidence, and
routes bounded CI correction work back to the sticky maker.

## Entry criteria

- One Linear ticket can be claimed and executed safely.
- GitHub identity and publication boundary are approved.
- Pilot repository required checks and branch protection are configured.

## Work packages

### S07.1 Implement GitHub authentication and repository registry

Implementation:

1. Construct installation/token credentials through the approved mechanism.
2. Restrict access to allowlisted repositories.
3. Map repository name to base branch, required checks, and workspace root.
4. Add request timeout, pagination, rate-limit handling, and redacted tracing.
5. Verify credentials cannot merge.

Verification:

- Unauthorized repository lookup fails closed.
- Permission test confirms maker/publisher can push as designed and reviewer cannot.

### S07.2 Normalize pull-request facts

`GitHubPort` returns:

```text
repository
pull request number and URL
state and draft state
base branch and base SHA
head branch and head SHA
merge state
author
updated timestamp
```

Implementation:

1. Find PR by recorded number.
2. Fallback to exact orchestrated branch lookup only when number is absent.
3. Reject ambiguous multiple PRs.
4. Verify PR repository and branch belong to the WorkItem.

Verification:

- Open, draft, closed, merged, missing, and ambiguous fixtures are covered.

### S07.3 Resolve maker result to canonical PR state

Sequence:

1. Receive normalized `pr_ready`.
2. Canonically fetch PR.
3. Confirm branch and repository.
4. Persist PR number, URL, and exact head SHA.
5. Transition WorkItem to `waiting_for_ci`.
6. Project Linear to In Review.

Rules:

- Provider-reported SHA/URL is a hint, not truth.
- No PR leaves the ticket In Progress/Blocked according to result policy.

Verification:

- Fabricated or stale provider PR data cannot advance state.

### S07.4 Implement GitHub webhook ingress

Subscribe to relevant PR, push/check-suite/check-run/workflow events as appropriate.

Implementation:

1. Verify GitHub webhook signature over raw body.
2. Persist delivery in the shared inbox with source `github`.
3. Normalize event only after durable receipt.
4. Fetch canonical PR/check state before lifecycle transition.
5. Treat event ordering as untrusted.

Verification:

- Duplicate and out-of-order deliveries converge.
- Unrelated repositories and branches are ignored.

### S07.5 Implement required-check evaluation

Implementation:

1. Resolve configured required check names for the base branch.
2. Collect statuses for the exact current head SHA.
3. Normalize to `pending`, `failed`, or `succeeded`.
4. Treat missing required check as pending, not success.
5. Record workflow/check URLs and completion timestamps.
6. Re-fetch PR head before committing the gate result.

Verification:

- Green checks for an old SHA cannot advance a new SHA.
- Optional check failure does not fail required CI.
- Renamed/missing required check produces actionable configuration error.

### S07.6 Implement head-change invalidation

On any canonical new head:

1. Persist new SHA.
2. Invalidate old CI evidence.
3. Invalidate any review result/approval for the old SHA.
4. Cancel or mark stale a running reviewer for the old SHA.
5. Return to `waiting_for_ci`.

Verification:

- Push during CI and push during review both converge to the new SHA.
- Stale reviewer output can be published as diagnostic only if clearly marked stale;
  it can never approve.

### S07.7 Implement bounded CI correction dispatch

Policy:

- Correct only failures classified as plausibly code-addressable.
- Infrastructure/canceled/missing-run failures wait or alert.
- Code-addressable failure creates an AI-initiated child Run.
- Reuse sticky maker harness/model policy.
- Acquire total, AI, repository, and ticket slots.
- Provide failing check names and URLs.
- Maximum initial CI correction cycles: two.

Verification:

- Correction waits when AI slot is occupied.
- Capacity retry does not increment CI correction cycle.
- Third engineering CI failure moves to Blocked with evidence.

### S07.8 Implement active-PR reconciliation

Every five minutes:

1. Query only PRs referenced by nonterminal WorkItems.
2. Reconcile PR state, head SHA, required checks, close, and merge.
3. Repair missed GitHub events.
4. Do not enumerate the entire organization.

Verification:

- Missed check completion and merge events are repaired.
- PR closed without merge maps according to approved canceled/blocked policy.

### S07.9 Handle human repository actions

Define deterministic responses for:

- Human pushes to the orchestrated branch.
- PR retargeted to another base branch.
- PR converted from/to draft.
- PR closed/reopened.
- Branch deleted.
- Required checks configuration changed.

Verification:

- The orchestrator never overwrites or force-pushes human work automatically.
- Conflicts create visible operator state.

## Suggested pull-request slices

1. GitHub client, authentication, and PR normalization.
2. Webhook ingress and required-check evaluation.
3. SHA invalidation and CI correction.
4. Reconciliation and human-action policy.

## Sprint demo

Resolve a maker-created draft PR, fail required CI, dispatch one sticky-maker
correction under the AI slot, make CI green, then push a new SHA and demonstrate
that all old evidence is invalidated.

## Exit criteria

- PR and CI truth is canonical and SHA-bound.
- GitHub webhook and reconciliation converge.
- CI correction is bounded and uses sticky maker.
- Stale CI/review evidence cannot advance state.
- Credentials cannot merge.

## Evidence Sources

- [`../ai_harness_implementation.md`](../ai_harness_implementation.md), GitHub, CI,
  correction, reconciliation, and authority sections.

## Unknown / Unverified

- Exact GitHub check APIs depend on the pilot repository's Actions configuration.
- Merge queue behavior remains outside the initial pilot unless already required.

