SPEC AUDIT: Spire Prefix for Skill Names and Methodology Cleanup
Overall Score: 49/50

Section scores:
  Completeness:  10/10  All 7 required sections are present and substantively filled. The Goal clearly frames two distinct bugs, Actors and User Journeys cover both the prefix change and the cleanup logic, and Acceptance Criteria map directly to the journeys.
  Testability:   10/10  Every acceptance criterion is deterministic and independently verifiable: frontmatter values can be asserted by file inspection, stale-file removal by pre/post state comparison, empty-directory cleanup by directory listing, idempotency by repeated execution, and stdlib-only compliance by module analysis.
  Clarity:        9/10  The intent is understandable to a developer unfamiliar with the domain. The distinction between "human-invoked" and "auto-loaded" skills is inferred from the Out of Scope list rather than formally defined, which is acceptable but slightly indirect.
  Scope:         10/10  The Out of Scope section is precise and explicit: it names the 4 skills receiving the prefix, excludes the 2 auto-loaded skills, rules out behavior changes, new skills, `--force` modifications, and cleaning outside the target directories. This effectively prevents scope drift.
  Ambiguity:     10/10  The Open Questions section is empty ("None."), and no unresolved ambiguity blocks planning or implementation.

Blocking Issues (must be resolved before planning):
  None.

Non-blocking Suggestions:
  S1: AC 7 states "Files matching `.spire-sync-state.json` or `.spire-source.json` are never deleted..." The word "matching" could be tightened to "named exactly" to remove any possibility of glob/pattern interpretation.

VERDICT: PASS (≥40)
