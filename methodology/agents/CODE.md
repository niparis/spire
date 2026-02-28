# Implementation session 

## Feature Slug

Before implementation, resolve the active feature slug.

Feature Resolution:
1. If runtime context already provides a concrete feature slug, use it.
2. Otherwise, prefer an explicit `Feature: <slug>` from session context.
3. If the slug is unknown or ambiguous, ask the human and wait.
4. Do not guess from branch names or assumptions.
5. Replace every `[feature]` placeholder below with the resolved slug.

## Operating Rules

### Session Continuity Rules

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

### Always
- Read SESSION.md at session start, update at session end (SC-1, SC-2)
- Write failing tests before implementation (TDD)
- Run lint + typecheck + tests after every logical changeset
- Commit with format: "type: description — satisfies AC-n"
- Log all decisions in SESSION.md under Closed Decisions
- Run Gate 4 verification and produce changes/[feature]/VERIFICATION_REPORT.md before PR
- Prefer running Gate 4 verification in a separate OpenCode session from implementation


### Never
- Skip Gate 1 (spec audit) for any new feature
- Proceed past a FAIL or unresolved CONDITIONAL audit verdict
- Attempt the same failing fix more than 3 times (invoke SC-3 instead)
- Open a PR without a VERIFICATION_REPORT.md
- Open or request a PR when verification verdict is NEEDS WORK
- Reuse the same active implementation run as the verifier for final Gate 4 verdict
- Modify files in archive/ or specs/ during implementation

## Implementation Documentatio

1. Read .methodology/templates/session-template.md — this is the required SESSION.md structure.
2. Read changes/[feature]/SESSION.md (if exists) — this is your current state.
3. Read changes/[feature]/TASKS.md — this is your work queue.
4. Read specs/[feature].md — this is your truth.
5. Read agents/SPIRE.md — these are your operating rules.

If changes/[feature]/SESSION.md does not exist, create it using
.methodology/templates/session-template.md before starting tasks.

For each task:
  a. Write the failing test first (derived from the acceptance criterion it satisfies).
  b. Implement until the test passes.
  c. Run: [your lint command] + [your typecheck command] + [your test command].
  d. If all green: commit with message "task: [task name] — satisfies AC-[n]".
  e. If any fail: attempt fix. If same failure occurs 3 times, invoke SC-3.

After each task, update SESSION.md while preserving the template sections.

At session end, update SESSION.md with full status and next action.

Gate handoff:
- The code agent does not produce the final Gate 4 verdict.
- When implementation tasks are complete, hand off to the verification agent to
  produce `changes/[feature]/VERIFICATION_REPORT.md`.
- Do not open or request a PR from this mode when verification is pending.

KEEP WORKING UNTIL YOU REACH A STOP CONDITION DESCRIBED IN THIS PROMPT
