Gate 4 verification session for changes/[feature]/.

Before verification, resolve the active feature slug.

Feature Resolution:
1. If runtime context already provides a concrete feature slug, use it.
2. Otherwise, prefer an explicit `Feature: <slug>` from session context.
3. If the slug is unknown or ambiguous, ask the human and wait.
4. Do not guess from branch names or assumptions.
5. Do not use any root-level `changes/SESSION.md`; only `changes/[feature]/SESSION.md` is valid.
6. Replace every `[feature]` placeholder below with the resolved slug.

Inputs:
1. specs/[feature].md
2. changes/[feature]/PLAN.md
3. changes/[feature]/TASKS.md
4. changes/[feature]/SESSION.md
5. agents/AGENTS.md

Output:
- changes/[feature]/VERIFICATION_REPORT.md

Required report sections:
1. TRACEABILITY MATRIX
   - For every acceptance criterion in specs/[feature].md:
     AC-n | implemented in [file:line] | tested by [test file:test name] | PASS/FAIL
2. COMMANDS RUN
   - Exact commands and output (truncate long output to last 50 lines)
3. COVERAGE SUMMARY
   - Fully covered / partially covered / not covered acceptance criteria
4. SELF-REVIEW
   - Compare implementation against spec intent and flag mismatches
5. VERDICT
   - READY FOR PR | NEEDS WORK (with required fixes)

Rules:
- Verification is a gate, not implementation. Do not implement feature code in this mode.
- If evidence is missing, mark NEEDS WORK with explicit remediation steps.
- Do not open or request a PR when verdict is NEEDS WORK.
- Preferred independence model: run verification in a separate OpenCode session
  from the implementation run.
- Minimum independence rule: verifier must not be the same active implementation
  run that produced the feature changes.
