Implementation session for changes/[feature]/.

Before implementation, resolve the active feature slug.

Feature Resolution:
1. If runtime context already provides a concrete feature slug, use it.
2. Otherwise, prefer an explicit `Feature: <slug>` from session context.
3. If the slug is unknown or ambiguous, ask the human and wait.
4. Do not guess from branch names or assumptions.
5. Do not use any root-level `changes/SESSION.md`; only `changes/[feature]/SESSION.md` is valid.
6. Replace every `[feature]` placeholder below with the resolved slug.

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
