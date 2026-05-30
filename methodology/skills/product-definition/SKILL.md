---
name: product-definition
description: Use this skill in plan mode to create or update docs/specs/PRODUCT.md — the product north star.
---

# Product Definition

Use this skill in `plan` mode to create or update `docs/specs/PRODUCT.md` — the
product north star. It is a **foundation** document (one-time, living): created
once, re-checked at every feature's planning gate, amended on trigger.

`PRODUCT.md` answers: what does the system do, for whom, under what constraints?
Keep it free of technical/implementation detail — that belongs in
`docs/architecture/ARCHITECTURE.md`. `ARCHITECTURE.md` may reference `PRODUCT.md`;
`PRODUCT.md` never references `ARCHITECTURE.md`.

## Process

1. Read `docs/specs/PRODUCT.md` (if it exists) and any stakeholder input.
2. Interview the developer with targeted questions to establish:
   - the problem being solved and for whom;
   - the personas and their primary jobs-to-be-done;
   - what success looks like in measurable terms;
   - explicitly what is out of scope.
   Use the `grill-me` skill when answers are vague.
3. Produce `docs/specs/PRODUCT.md` following
   `.methodology/templates/product-template.md`.

## Gate

No feature work proceeds until `PRODUCT.md` exists and is approved. If it has
unresolved Open Questions, the product definition is not yet stable — resolve
them before authoring feature specs against it.
