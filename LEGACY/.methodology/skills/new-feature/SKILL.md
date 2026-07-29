---
name: spire-new-feature
description: Scaffold a new feature spec and change folder, then interview the user to author the spec. Use at Gate 0 when starting a new feature, or when the user says "new feature" / "start a feature".
---

# New Feature (Gate 0 — Spec authoring)

Run this in `plan` mode. It scaffolds the feature and drives spec authoring
entirely inside OpenCode — no `spire` CLI call.

## Preconditions

`docs/specs/PRODUCT.md` must exist. If it does not, stop and use the
`product-definition` skill first (no feature work before the product vision).

## Scaffold

1. Determine the next incremental id `NNN` (zero-padded) by scanning existing
   `docs/specs/feature-*.md` and taking max + 1.
2. Agree a kebab-case `<slug>` with the user. The feature slug is `NNN-<slug>`.
3. Create the spec `docs/specs/feature-NNN-<slug>.md` from
   `.methodology/templates/spec-template.md`.
4. Create the change folder `docs/changes/NNN-<slug>/` and seed
   `docs/changes/NNN-<slug>/SESSION.md` from
   `.methodology/templates/session-template.md`.

## Author the spec

Interview the user to fill every section of the spec: Goal, Actors, User
Journeys (include unhappy paths), Acceptance Criteria (each independently
testable, falsifiable, free of implementation detail), Non-Functional
Requirements (specific and measurable), Out of Scope, Open Questions. Use the
`grill-me` skill when answers are vague.

A section left as a template placeholder is not done. Every Open Question BLOCKS
Gate 1 — drive them to resolution or record an owner and due date.

## Exit

A complete `docs/specs/feature-NNN-<slug>.md`. State becomes `Spec only`. Next
gate: dispatch the `spec-auditor` subagent (Gate 1).
