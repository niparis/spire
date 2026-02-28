Before planning, resolve the active feature slug.

Feature Resolution:
1. If runtime context already provides a concrete feature slug, use it.
2. Otherwise, look for an explicit `Feature: <slug>` in the relevant session context.
3. If the slug is still unknown or ambiguous, ask the human and wait.
4. Do not guess from branch names or assumptions.
5. Do not use any root-level `changes/SESSION.md`; only `changes/[feature]/SESSION.md` is valid.
6. Replace every `[feature]` placeholder below with the resolved slug.

The spec audit has passed for specs/[feature].md.

1. Read .methodology/templates/spec-template.md — this is the required spec schema contract.
2. Read specs/[feature].md, specs/product.md, agents/AGENTS.md,
   and any relevant architecture/adr-*.md files.

If specs/[feature].md deviates from the required template structure, stop and
ask for spec correction before continuing planning.

3. List any remaining technical ambiguities (not already in the spec's
   open questions — those are resolved). Output as QUESTIONS.
   Wait for human answers before continuing if any are HIGH priority.

4. Propose 2–3 implementation options with explicit tradeoffs.
   Label each: recommended / alternative / rejected-because.

5. Output changes/[feature]/PLAN.md with:
   - chosen approach and rationale
   - file-by-file change list
   - test strategy (unit / integration / e2e breakdown)
   - rollback plan
   - CI/CD impact

6. Output changes/[feature]/TASKS.md:
   - atomic tasks, each 5–10 minutes of work
   - each task has: goal, files to touch, tests to add, verification step
   - ordered by dependency

NO code changes. NO file edits outside changes/[feature]/. This is Plan mode.
