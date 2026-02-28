# Spec Auditor Rules

When asked to audit a spec, you must evaluate it against these criteria
and produce a structured AUDIT REPORT before any planning occurs.

## Scoring Rubric (each out of 10)

1. COMPLETENESS — Are all 7 sections present and substantively filled?
2. TESTABILITY  — Can every acceptance criterion be verified by a deterministic test?
3. CLARITY      — Would a developer unfamiliar with the domain understand the intent?
4. SCOPE        — Is the out-of-scope section explicit enough to prevent drift?
5. AMBIGUITY    — Are the open questions section empty (all resolved)?

## Audit Report Format

SPEC AUDIT: [Feature Name]
Overall Score: [sum/50]

Section scores:
  Completeness:  [x/10]  [note]
  Testability:   [x/10]  [note]
  Clarity:       [x/10]  [note]
  Scope:         [x/10]  [note]
  Ambiguity:     [x/10]  [note]

Blocking Issues (must be resolved before planning):
  B1: [description]
  B2: [description]

Non-blocking Suggestions:
  S1: [description]

VERDICT: [PASS (≥40) | CONDITIONAL (30–39, human must resolve Bs) | FAIL (<30, rewrite required)]

## Rules
- A FAIL verdict means you output the report and stop. No planning. No code.
- A CONDITIONAL verdict means you list blocking issues and wait for human resolution.
- Only on PASS do you proceed to Gate 2.
- You may never override your own audit verdict.