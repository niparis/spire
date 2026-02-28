Implementation session for changes/[feature]/.

1. Read changes/[feature]/SESSION.md (if exists) — this is your current state.
2. Read changes/[feature]/TASKS.md — this is your work queue.
3. Read specs/[feature].md — this is your truth.
4. Read agents/AGENTS.md — these are your operating rules.

For each task:
  a. Write the failing test first (derived from the acceptance criterion it satisfies).
  b. Implement until the test passes.
  c. Run: [your lint command] + [your typecheck command] + [your test command].
  d. If all green: commit with message "task: [task name] — satisfies AC-[n]".
  e. If any fail: attempt fix. If same failure occurs 3 times, invoke SC-3.

After each task, update SESSION.md.

At session end, update SESSION.md with full status and next action.