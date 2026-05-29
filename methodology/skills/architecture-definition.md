# Architecture Definition

Use this skill in `plan` mode to create or update
`docs/architecture/ARCHITECTURE.md` and Architecture Decision Records. It is a
**foundation** document (one-time, living): established once, re-checked at every
feature's planning gate, amended on trigger.

`ARCHITECTURE.md` answers: how is the system built to fulfil the product
requirements? Keep user goals and business rules out of it — those live in
`docs/specs/PRODUCT.md`. If implementation detail has crept into `PRODUCT.md`,
move it here; if user goals or business rules appear here, move them to
`PRODUCT.md`.

## Process

1. Read `docs/specs/PRODUCT.md`, the current `ARCHITECTURE.md` (if any), and
   existing `docs/architecture/adr-*.md` files.
2. For a new or substantially changed architecture, propose 2–3 approaches with
   explicit tradeoffs, each labelled recommended / alternative /
   rejected-because. Wait for human selection before writing.
3. Produce `docs/architecture/ARCHITECTURE.md` following
   `.methodology/templates/architecture-template.md` (system overview, component
   map, tech stack with rationale, key data flows, external dependencies,
   conventions, known constraints, open architectural questions).

## ADRs

For any significant architectural decision, write
`docs/architecture/adr-NNN-<name>.md` with: context (what forced the decision),
options considered, decision and rationale, consequences and tradeoffs accepted,
and status (PROPOSED | ACCEPTED | SUPERSEDED). Add a reference line in
`ARCHITECTURE.md`'s ADR index.

## Gate

Feature specs must not be written against areas covered by unresolved open
architectural questions. Resolve them first.
