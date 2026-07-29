# SPIRE — Spec-Driven Development Governance

This is the single authoritative description of how software is built in this
repository. Every OpenCode session loads this file. Read it before acting.

## 1. Purpose & Golden Rule

Golden rule: **bad input → bad output**. The agent is a skilled but literal
contractor — it executes exactly what it reads. The job of this methodology is
to make sure what enters the execution layer is worth executing.

SPIRE governs the two layers most workflows skip:

- **Spec quality** — no work begins against an ambiguous spec. A spec is
  independently audited before any plan or code exists (Gate 1).
- **Session continuity** — feature-scoped working memory (`SESSION.md` + the SC
  rules) so multi-session features re-orient instantly and never drift.

## 2. Operating Model

- The only entry points are OpenCode's **built-in `plan` and `build` modes**.
  We do **not** define custom primary agents.
- All SPIRE behaviour is delivered as **skills** (loaded into a mode) and
  **subagents** (delegated units of work, invoked manually or automatically).
- **Stay in OpenCode.** Nothing in the development loop requires shelling out to
  the `spire` CLI. Every step is an agent action, a skill, or a subagent. The
  `spire` binary is for bootstrap/maintenance only (`init`, `update`, `upgrade`),
  run from the shell outside the loop.
- `plan` mode is read-and-plan only — no implementation, no edits outside the
  active feature's `docs/changes/<feature>/`. `build` mode implements.

## 3. Foundations (one-time, living)

Two documents are the project's north star:

- `docs/specs/PRODUCT.md` — product vision, personas, core use cases, business
  and technical constraints, success metrics, out-of-scope. As few technical
  details as possible.
- `docs/architecture/ARCHITECTURE.md` — system overview, component map, tech
  stack with rationale, key data flows, conventions, known constraints.

Rules:

- **Gate.** If `docs/specs/PRODUCT.md` does not exist, you MUST stop and drive
  its creation (load the `product-definition` skill in `plan` mode) before any
  feature work. Likewise establish `ARCHITECTURE.md` (via the
  `architecture-definition` skill) before the first feature that depends on it.
- **Living.** Both are created once but re-checked at every feature's planning
  gate and amended on trigger — a product pivot, or a feature that forces an
  architectural decision (which becomes an ADR; see §9).
- `ARCHITECTURE.md` may reference `PRODUCT.md`. `PRODUCT.md` never references
  `ARCHITECTURE.md`.

## 4. Repository Layout

```
docs/
  specs/
    PRODUCT.md                       # product vision (one-time, living)
    feature-001-<slug>.md            # feature spec (Gate 0)
  architecture/
    ARCHITECTURE.md                  # tech architecture (one-time, living)
    adr-001-<name>.md                # ADRs (as-needed)
  changes/
    001-<slug>/
      AUDIT.md                       # spec-auditor verdict (Gate 1 marker)
      PLAN.md                        # approach + embedded ordered tasks (Gate 2 marker)
      SESSION.md                     # live continuity (Gate 3 marker)
      VERIFICATION_REPORT.md         # verifier output (Gate 4 marker)
  archive/
    001-<slug>/                      # completed change folders (Complete)
```

The synced methodology payload lives under `.methodology/`. Skills and subagents
are referenced with that prefix (for example `.methodology/skills/new-feature/`).

The feature slug is `NNN-<slug>` (zero-padded incremental id + kebab name). The
spec file is `docs/specs/feature-NNN-<slug>.md`; the change folder is
`docs/changes/NNN-<slug>/`. Resolve `[feature]` to this slug everywhere below.

## 5. The Feature Lifecycle (Gates 0–5)

Each gate has an entry condition, an owner (mode / skill / subagent), and an exit
artifact that doubles as the lifecycle state marker (§8). No gate is skippable.

### Gate 0 — Spec authoring
- **Entry:** `PRODUCT.md` exists.
- **Owner:** `new-feature` skill in `plan` mode. It scaffolds
  `docs/specs/feature-NNN-<slug>.md` and `docs/changes/NNN-<slug>/`, then
  interviews the user to author the spec. Use the `grill-me` skill for hard cases.
- **Exit:** a complete feature spec — Goal, Actors, User Journeys (incl. unhappy
  paths), Acceptance Criteria (independently testable, falsifiable, no
  implementation detail), Non-Functional Requirements, Out of Scope, Open
  Questions. Every open question BLOCKS Gate 1.
- **State:** `Spec only`.

### Gate 1 — Spec audit
- **Entry:** spec complete, no open questions.
- **Owner:** `spec-auditor` subagent (independent — not the run that authored the
  spec). Scores the spec out of 50 across Completeness, Testability, Clarity,
  Scope, Ambiguity.
- **Exit:** `docs/changes/NNN-<slug>/AUDIT.md` with verdict
  `PASS (≥40)` | `CONDITIONAL (30–39)` | `FAIL (<30)`. CONDITIONAL lists blocking
  issues for the human to resolve; FAIL means rewrite. Only PASS proceeds.
- **State:** `Awaiting planning`.

### Gate 2 — Planning
- **Entry:** `AUDIT.md` verdict is PASS.
- **Owner:** `planner` subagent, invoked from `plan` mode after the audit passes.
  It proposes 2–3 implementation options (recommended / alternative /
  rejected-because), then writes a single `PLAN.md`: chosen approach + rationale,
  file-by-file change list, test strategy (unit / integration / e2e), rollback
  plan, CI/CD impact, and an **ordered task list** (each task: goal, files to
  touch, tests to add, verification step). The planner presents the plan; the
  human approves before implementation starts.
- **Exit:** `docs/changes/NNN-<slug>/PLAN.md`, human-approved. No `TASKS.md`, no
  `PROPOSAL.md` — the spec is the Gate-0 artifact and the plan carries its tasks.
- **State:** `Awaiting implementation`.

### Gate 3 — Implementation loop
- **Entry:** `AUDIT.md` PASS **and** `PLAN.md` approved.
- **Owner:** `build` mode driven by the `implementation-loop` skill (§6).
- **Exit:** every task in `PLAN.md` complete and `SESSION.md` current.
- **State:** `In progress`.

### Gate 4 — Verification
- **Entry:** all tasks complete.
- **Owner:** `verifier` subagent (default). Escalate to a fully separate OpenCode
  session for high-risk or large features. The verifier performs a gap analysis
  (application behaviour vs spec) and produces `VERIFICATION_REPORT.md` with: a
  traceability matrix (`AC-n | implemented in file:line | tested by test:name |
  PASS/FAIL`), commands run with evidence (truncate long output to last 50
  lines), coverage summary, self-review against spec intent, and a verdict.
- **Exit:** `docs/changes/NNN-<slug>/VERIFICATION_REPORT.md` with verdict
  `READY FOR PR` | `NEEDS WORK`. NEEDS WORK returns to Gate 3.
- **State:** `Awaiting PR`.

### Gate 5 — PR & merge
- **Entry:** verdict `READY FOR PR`.
- **Owner:** human, with CI. Open a PR referencing the spec, `PLAN.md`, and
  `VERIFICATION_REPORT.md`; merge after CI + human review.
- **Exit:** move `docs/changes/NNN-<slug>/` → `docs/archive/NNN-<slug>/`.
- **State:** `Complete`.
- Never open a PR without a verification report or while the verdict is
  NEEDS WORK.

## 6. Code Production Loop (Gate 3)

Run in `build` mode under the `implementation-loop` skill.

### Session Continuity rules
- **SC-1 (start):** before any action, resolve the active feature slug from
  runtime context or explicit human input; if unknown or ambiguous, ask and wait
  (never guess from branch names). Then read `docs/changes/[feature]/SESSION.md`
  and treat it as ground truth. Only `docs/changes/[feature]/SESSION.md` is valid
  — never a root-level `SESSION.md`, and never re-derive state from git log alone.
- **SC-2 (end):** at the end of every session (or when asked to pause/stop),
  update `SESSION.md` with current status, decisions made, and the next action.
  A session that ends without updating `SESSION.md` is incomplete.
- **SC-3 (circuit-breaker):** if a single test, lint error, or build step has
  failed 3 times with different approaches, STOP. Record it in the Failure Log
  section of `SESSION.md` and surface it to the human. Do not attempt a 4th.
- **SC-4 (constraints):** discovered constraints go into `SESSION.md`
  immediately. If one invalidates a task in `PLAN.md`, flag it before proceeding
  — do not silently work around it.

### Per-task loop
Read in order: `SESSION.md` (current state) → `PLAN.md` (work queue + approach) →
the feature spec (truth) → this file (rules). Then, for each task in `PLAN.md`'s
ordered list:

1. Write the failing test first, derived from the acceptance criterion it
   satisfies.
2. Implement until the test passes.
3. Run lint + typecheck + tests (commands from `AGENTS.md`).
4. If green: commit `type: description — satisfies AC-n`, then update
   `SESSION.md` (move the task to Completed with its commit ref).
5. If a step fails: fix and retry. On the 3rd identical failure, invoke SC-3.

### Exit & prohibitions
- When all tasks are complete, hand off to the `verifier` subagent. The
  implementation loop never writes `VERIFICATION_REPORT.md` and never issues its
  own verdict — verification is independent by construction.
- **Never:** skip Gate 1; proceed past a FAIL or unresolved CONDITIONAL verdict;
  open a PR without a verification report or on NEEDS WORK; modify
  `docs/archive/` or `docs/specs/` during implementation.

## 7. Subagent & Skill Dispatch

**Skills** (loaded into a mode):
- `product-definition` — `plan` mode; shape `PRODUCT.md`.
- `architecture-definition` — `plan` mode; shape `ARCHITECTURE.md` and ADRs.
- `new-feature` — scaffold the spec + change folder and interview the user (Gate 0).
- `grill-me` — relentless interview to resolve a design tree; available in `plan`
  mode for Gate 0 authoring and Gate 2 planning.
- `implementation-loop` — `build` mode; governs the code production loop (§6).

**Subagents** (delegated; manually or automatically invoked):
- `spec-auditor` (MUST, Gate 1) — independent spec audit → `AUDIT.md`.
- `planner` (MUST, Gate 2) — produce and present `PLAN.md`.
- `verifier` (MUST, Gate 4) — gap analysis + report → `VERIFICATION_REPORT.md`.
- `docs-writer` (SHOULD) — when API/behaviour/docs-facing changes occur; output
  doc updates + a note in `SESSION.md`.
- `investigator` (SHOULD) — when blocked by unknowns or external tradeoffs;
  output recommendation + sources into `SESSION.md`.

Dispatch rule: at each gate, the MUST subagent runs. SHOULD subagents fire on
their trigger. Log every delegation in `SESSION.md` (subagent, reason, inputs,
output, verdict).

## 8. State Model

Lifecycle state is **inferred from the filesystem** — there is no spec-header
`Status:` field to keep in sync. Each gate's exit artifact is the marker.

| State | Marker | Producer |
|---|---|---|
| Spec only | `docs/specs/feature-NNN-<slug>.md` exists | `new-feature` skill |
| Awaiting planning | `docs/changes/NNN-<slug>/AUDIT.md` (PASS) | `spec-auditor` subagent |
| Awaiting implementation | `docs/changes/NNN-<slug>/PLAN.md` | `planner` subagent |
| In progress | `SESSION.md` shows progress | `implementation-loop` skill |
| Awaiting PR | `VERIFICATION_REPORT.md` (READY FOR PR) | `verifier` subagent |
| Complete | folder moved to `docs/archive/NNN-<slug>/` | human, post-merge |

## 9. Out-of-band Changes

For hotfixes or post-planning changes, do not work around the spec silently:

- If you cannot map a change to a feature, ask.
- Small change inside an existing feature → update that feature's `PLAN.md` and
  `SESSION.md`.
- New scope → open a new feature at Gate 0.
- A change in behaviour or architecture → update `PRODUCT.md` / `ARCHITECTURE.md`,
  and record any significant architectural decision as
  `docs/architecture/adr-NNN-<name>.md` (context, options, decision, consequences,
  status), referenced from `ARCHITECTURE.md`.

## 10. Activity Cadence

- **One-time / bootstrap (shell):** `spire init`.
- **Foundational (agent-gated, living):** `PRODUCT.md`, `ARCHITECTURE.md` — no
  feature work proceeds until `PRODUCT.md` exists; amend on trigger.
- **Occasional maintenance (shell):** `spire update`, `spire upgrade`.
- **Regular per-feature loop (Gates 0–5):** `new-feature` skill → `spec-auditor`
  subagent → `planner` subagent → `implementation-loop` skill → `verifier`
  subagent → PR / merge / archive.
- **As-needed within a feature:** ADR creation, `investigator` subagent (blocked
  by unknowns), `docs-writer` subagent (docs-facing change), `grill-me` skill
  (high ambiguity), foundation re-check / amend.
