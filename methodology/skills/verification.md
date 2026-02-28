# Verification

Use this skill at Gate 4 to validate implementation completeness after coding.

## Required Output

Produce `changes/[feature]/VERIFICATION_REPORT.md` with these sections in order:

1. TRACEABILITY MATRIX
   - For every acceptance criterion in `specs/[feature].md`:
     `AC-n | implemented in [file:line] | tested by [test file:test name] | PASS/FAIL`

2. COMMANDS RUN
   - Exact commands executed.
   - Include output evidence (truncate to last 50 lines per command when long).

3. COVERAGE SUMMARY
   - Classify ACs as fully covered, partially covered, or not covered.

4. SELF-REVIEW
   - Compare implementation against spec intent (not only literal wording).
   - Flag mismatches, shortcuts, and hidden risk.

5. VERDICT
   - `READY FOR PR` or `NEEDS WORK`.
   - If `NEEDS WORK`, list mandatory remediation items.

## Rules

- Verification is a gate, not implementation work.
- Do not mark `READY FOR PR` when any AC is uncovered or failing.
- Do not open or request a PR when verdict is `NEEDS WORK`.
