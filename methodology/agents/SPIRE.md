# SPIRE.md — Global Agent Governance

## Session Continuity Rules

RULE SC-1: At the START of any implementation session, before any action,
resolve the active feature slug from runtime context or explicit human input.
If slug is unknown/ambiguous, ask and wait. Then read
changes/[feature]/SESSION.md if it exists. Treat it as ground truth for
current state. Do not use a root-level changes/SESSION.md and do not
re-derive state from git log alone.

RULE SC-2: At the END of every session (or when asked to pause/stop),
update SESSION.md with current status, decisions made, and the next action.
This is not optional. A session that ends without updating SESSION.md is incomplete.

RULE SC-3 (Circuit-breaker): If any single test, lint error, or build step
has failed 3 or more times with different approaches, STOP. Record it in
the Failure Log section of SESSION.md. Do not attempt a 4th approach.
Surface it to the human with a summary of what was tried.

RULE SC-4: Discovered constraints go in SESSION.md immediately.
If a discovered constraint invalidates a task in TASKS.md, flag it before
proceeding. Do not silently work around it.

## Operating Rules

### Always
- Read SESSION.md at session start, update at session end (SC-1, SC-2)
- Write failing tests before implementation (TDD)
- Run lint + typecheck + tests after every logical changeset
- Commit with format: "type: description — satisfies AC-n"
- Log all decisions in SESSION.md under Closed Decisions
- Run Gate 4 verification and produce changes/[feature]/VERIFICATION_REPORT.md before PR
- Prefer running Gate 4 verification in a separate OpenCode session from implementation

### Ask Before
- Modifying any file outside the current feature's task scope
- Changing shared configuration (opencode.json, package.json, CI config)
- Adding new dependencies
- Deleting files

### Never
- Skip Gate 1 (spec audit) for any new feature
- Proceed past a FAIL or unresolved CONDITIONAL audit verdict
- Attempt the same failing fix more than 3 times (invoke SC-3 instead)
- Open a PR without a VERIFICATION_REPORT.md
- Open or request a PR when verification verdict is NEEDS WORK
- Reuse the same active implementation run as the verifier for final Gate 4 verdict
- Modify files in archive/ or specs/ during implementation

## Commands
Test:      [npm test / pytest / go test ./...]
Lint:      [eslint . / ruff check . / golangci-lint run]
Typecheck: [tsc --noEmit / mypy . / go vet ./...]
CI local:  [act / make ci]

## Spec Audit Threshold
Minimum score to proceed: 40/50
Blocking issue count to halt: any

## Session Continuity
See SC-1 through SC-4 in Session Continuity Rules section above.

## Circuit-Breaker
3 failures on the same step = STOP and escalate. See SC-3.

## Skills
Load conditionally via opencode.json instructions array:
- skills/spec-auditor/SKILL.md      (always loaded in plan agent)
- skills/product-definition.md      (load for product work)
- skills/architecture-definition.md (load for architecture work)
- skills/verification.md            (load for verification work)

## Subagents (When to Invoke)

- `verifier` (MUST): before PR or merge decision; output `changes/[feature]/VERIFICATION_REPORT.md`; if verdict is NEEDS WORK, stop.
- `reviewer` (MUST): after major module completion or SC-3 failure; output `changes/[feature]/REVIEW_REPORT.md`; unresolved HIGH issues block progress.
- `docs-writer` (SHOULD): when API/behavior/docs-facing changes occur; output doc updates + note in `SESSION.md`.
- `investigator` (SHOULD): when blocked by unknowns or external tradeoffs; output recommendation + sources in `SESSION.md`.

Dispatch rule: pick the first matching MUST; if none, pick highest-value SHOULD.
Log every delegation in `changes/[feature]/SESSION.md` (agent, reason, inputs, output, verdict).
