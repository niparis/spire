---
name: implementation-loop
description: Governs the Gate 3 code production loop in build mode — TDD per task, commits, SESSION.md continuity, and the SC circuit-breaker. Use when implementing an approved feature plan.
---

# Implementation Loop (Gate 3)

Run this in `build` mode. The canonical rules live in `.methodology/agents/SPIRE.md`
§6; this skill is the operational checklist.

## Entry

`docs/changes/[feature]/AUDIT.md` = PASS **and** `docs/changes/[feature]/PLAN.md`
approved by the human. Resolve the active feature slug first (SC-1); if unknown
or ambiguous, ask and wait — never guess from branch names.

## Session start (SC-1)

Read in order, treating each as authoritative for its purpose:
1. `docs/changes/[feature]/SESSION.md` — current state (ground truth). Only this
   feature-scoped file is valid; never a root-level `SESSION.md`.
2. `docs/changes/[feature]/PLAN.md` — approach + ordered task list.
3. `docs/specs/feature-[feature].md` — the truth.
4. `.methodology/agents/SPIRE.md` and `AGENTS.md` — the rules + project commands.

If `SESSION.md` does not exist, create it from
`.methodology/templates/session-template.md` before starting.

## Per task (PLAN.md ordered list)

1. Write the failing test first, derived from the acceptance criterion it satisfies.
2. Implement until the test passes.
3. Run lint + typecheck + tests (commands from `AGENTS.md`).
4. Green → commit `type: description — satisfies AC-n`; update `SESSION.md`
   (move the task to Completed with its commit ref).
5. Fail → fix and retry. On the 3rd identical failure, invoke **SC-3**: STOP, log
   it in the Failure Log, escalate. No 4th attempt.

Record discovered constraints in `SESSION.md` immediately (**SC-4**); if one
invalidates a task, flag it before proceeding.

## Session end (SC-2)

Update `SESSION.md` (status, closed decisions, next action). Non-optional.

## Exit & prohibitions

When all tasks are complete, hand off to the `verifier` subagent (Gate 4). This
loop never writes `VERIFICATION_REPORT.md` and never issues its own verdict —
verification is independent.

Never: skip Gate 1; proceed past a FAIL or unresolved CONDITIONAL verdict; open a
PR without a verification report or on NEEDS WORK; modify `docs/archive/` or
`docs/specs/` during implementation.
