# Detailed SPIRE Workflow — Rewrite of `methodology/agents/SPIRE.md`

## Context

`SPIRE.md` is the governance doc that every OpenCode session loads (via `opencode.json` `instructions`). Today it is thin and internally inconsistent: it describes a `docs/`-rooted layout while the rest of the methodology + the Go CLI use root-level `specs/`+`changes/`; it still references a deleted `spec-auditor` skill and a non-existent `reviewer` agent; and it leans on custom **primary agents** (`build-feature`, `productengineer`) that the user wants to eliminate.

This round produces **one deliverable: a rewritten, detailed `methodology/agents/SPIRE.md`** that is the single authoritative description of the intended workflow. It must be precise about (a) the code-production loop, (b) which activities are one-time vs regular vs as-needed, and (c) a model built **only on OpenCode's built-in `build`/`plan` modes + skills + manually/auto-invoked subagents** — no custom primary agents.

Realigning the other methodology files, `opencode.json`, the manifest, the Go CLI, README, and PRODUCT.md is **out of scope this round** (tracked as follow-ups below).

## Decisions locked (from the grill)

1. **Layout:** finish the migration to a `docs/` root.
2. **No new agents:** entry points are OpenCode's built-in `build` + `plan` modes (configured in `opencode.json`). All spire logic lives in **skills** (loaded into a mode) and **subagents** (delegated). `build-feature` and `productengineer` primaries are retired.
3. **Stay in OpenCode:** nothing in the dev loop may require shelling out to the `spire` CLI. Everything is an agent action, a skill, or a manually-invoked subagent. The `spire` binary is bootstrap/maintenance only (`init`/`update`/`upgrade`).
4. **Code loop:** runs in `build` mode driven by an `implementation-loop` skill; independent checks happen at **gate boundaries only** (no mid-loop subagent calls).
5. **Gate order:** Spec → **audit** → **plan** → implement → verify → PR. (Audit-before-plan; matches the CLI's existing state precedence.)
6. **Planner is a subagent** invoked from `plan` mode after the audit passes; it produces the plan and presents it; human approves, then switches to `build` mode.
7. **Single `PLAN.md`** with an embedded ordered task list. No `TASKS.md`. `SESSION.md` tracks live progress.
8. **Single `verifier` subagent** does gap-analysis (app behaviour vs spec) **and** writes `VERIFICATION_REPORT.md` (traceability matrix + commands run + coverage + verdict). `review_against_spec` + `VERIFICATION.md` agent are folded in. Subagent is the default; escalate to a fully separate session only for high-risk features.
9. **No `PROPOSAL.md`** — the feature spec is the Gate-0 artifact.
10. **State = filesystem inference**, not a spec-header `Status:` field.
11. **Foundations are living:** PRODUCT.md + ARCHITECTURE.md are created once (agent-gated — no feature work until PRODUCT.md exists) and re-checked/amended on trigger during each feature's planning.
12. **Product vision** creation is **agent-initiated** (the agent refuses feature work until it exists). **Feature specs** are created by a **`new-feature` skill** that scaffolds the folder and interviews the user.

## Target file layout (`docs/` root)

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

`.methodology/` (synced payload) and the path-prefix rule stay: skills/subagents are referenced as `.methodology/...`.

## Lifecycle & state model (filesystem-inferred)

| State | Marker that produces it | Producer |
|---|---|---|
| Spec only | `docs/specs/feature-*.md` exists | `new-feature` skill |
| Awaiting planning | `docs/changes/<f>/AUDIT.md` (PASS) | `spec-auditor` subagent |
| Awaiting implementation | `docs/changes/<f>/PLAN.md` | `planner` subagent |
| In progress | `SESSION.md` shows progress | `implementation-loop` skill (build mode) |
| Awaiting PR | `VERIFICATION_REPORT.md` (READY) | `verifier` subagent |
| Complete | folder moved to `docs/archive/<f>/` | human, post-merge |

This order already matches the CLI's `infer.go` precedence (AUDIT → PLAN → SESSION → VERIFICATION); only the **paths** change for the `docs/` migration.

## Activity classification (the one-time vs regular ask)

**One-time / bootstrap (shell OK):** `spire init`.
**Foundational (agent-initiated, living — amend on trigger):** PRODUCT.md, ARCHITECTURE.md. Agent blocks feature work until PRODUCT.md exists.
**Occasional maintenance (shell OK):** `spire update`, `spire upgrade`.
**Regular per-feature loop (Gates 0–5):** new-feature skill → spec-auditor subagent → planner subagent → implementation-loop skill → verifier subagent → PR/merge/archive.
**As-needed / triggered within a feature:** ADR creation (`adr-NNN`), `investigator` subagent (blocked by unknowns), `docs-writer` subagent (docs-facing change), `grill-me` skill (high ambiguity in authoring/planning), foundation re-check/amend.

## Skills & subagents inventory (canonical names)

**Skills (loaded into a built-in mode):**
- `product-definition` — `plan` mode; shape PRODUCT.md.
- `architecture-definition` — `plan` mode; shape ARCHITECTURE.md + ADRs.
- `new-feature` — scaffolds `docs/specs/feature-*.md` + `docs/changes/<f>/` and interviews the user to author the spec.
- `grill-me` — relentless interview; available in `plan` mode for Gate 0 authoring and Gate 2 planning.
- `implementation-loop` — `build` mode; governs the code-production loop (below).

**Subagents (manually or auto invoked):**
- `spec-auditor` (Gate 1) — independent spec audit (completeness/testability/clarity/scope/ambiguity, score /50); writes `AUDIT.md`; PASS ≥ 40, blocks on FAIL/CONDITIONAL. Consolidates the deleted `spec-auditor` skill + `specs_reviewer`.
- `planner` (Gate 2) — produces single `PLAN.md`, presents it. (Repurposed `featureplanner`.)
- `verifier` (Gate 4) — gap-analysis + traceability + commands + verdict; writes `VERIFICATION_REPORT.md`. Consolidates `review_against_spec` + `VERIFICATION.md`.
- `docs-writer` (as-needed) — doc updates + SESSION note.
- `investigator` (as-needed) — research; recommendation into SESSION.md.

## Code production loop (Gate 3 — `build` mode + `implementation-loop` skill)

**Entry:** `AUDIT.md` = PASS **and** `PLAN.md` approved by human.
**Session start (SC-1):** read `SESSION.md` (ground truth) → `PLAN.md` (task list + approach) → feature spec (truth) → SPIRE.md (rules). Resolve the feature slug; if ambiguous, ask.
**Per task (ordered list in `PLAN.md`):**
1. Write the failing test, derived from the AC it satisfies.
2. Implement until it passes.
3. Run lint + typecheck + tests (commands from AGENTS.md).
4. Green → commit `type: description — satisfies AC-n`; update `SESSION.md` (task → Completed + commit ref).
5. Fail → fix. Same failure 3× ⇒ **SC-3 circuit-breaker**: STOP, log in Failure Log, escalate. No 4th attempt.
6. Discovered constraint ⇒ **SC-4**: record in `SESSION.md` immediately; if it invalidates a task, flag before proceeding.
**Session end (SC-2):** update `SESSION.md` (status, closed decisions, next action). Non-optional.
**Exit:** all tasks complete ⇒ hand off to `verifier` subagent. The loop never writes `VERIFICATION_REPORT.md` or its own verdict (independence).
**Never:** skip the audit gate; proceed past FAIL/CONDITIONAL; open a PR without `VERIFICATION_REPORT.md`; open a PR on NEEDS WORK; modify `docs/archive/` or `docs/specs/` during implementation.

## New `SPIRE.md` — section outline (what gets written)

1. **Purpose & golden rule** — "bad input → bad output"; spec quality + session continuity are the two governed layers.
2. **Operating model** — built-in `build`/`plan` modes only; skills + subagents; no custom primaries; stay in OpenCode (no `spire` CLI mid-loop).
3. **Foundations (one-time, living)** — PRODUCT.md + ARCHITECTURE.md; agent blocks feature work until PRODUCT.md exists; amend-on-trigger rule.
4. **Repository layout** — the `docs/` tree above.
5. **The feature lifecycle (Gates 0–5)** — one subsection per gate: entry condition, who runs it (mode/skill/subagent), exit artifact = state marker.
6. **Code production loop** — the SC-1..SC-4 rules + per-task loop above.
7. **Subagent & skill dispatch rules** — when each fires; MUST vs SHOULD; log every delegation in `SESSION.md`.
8. **State model** — the filesystem table; no `Status:` header.
9. **Out-of-band changes** — hotfixes / post-planning changes: ask where to map; update the feature's PLAN/SESSION or open a new feature; keep PRODUCT/ARCHITECTURE in sync.
10. **Activity cadence** — the one-time / regular / as-needed classification.

## Out of scope this round (follow-ups)

- Realign `methodology/agents/*` (CODE→implementation-loop skill, ARCHITECTURE, FEATURE_PLANNER, VERIFICATION, DOCS_WRITER, INVESTIGATOR) and delete retired primaries.
- Create/rename subagent + skill files to the canonical names; fix `opencode.json` (`grill_me` prompt path, agent vs subagent modes) and `manifest.json` mappings.
- Migrate the Go CLI (`new.go`, `infer.go`, scaffold paths) to the `docs/` root and reconcile `spire new`/`status` with the skill-based path.
- Update README + `docs/specs/PRODUCT.md` to match.

## Verification (for the doc itself)

Since the deliverable is a document, verify by: (1) re-reading the new `SPIRE.md` against the 12 locked decisions — each must be reflected and none contradicted; (2) confirming every path uses the `docs/` root; (3) confirming no reference to a custom primary agent, the deleted `spec-auditor` skill, `reviewer`, `PROPOSAL.md`, `TASKS.md`, or a spec-header `Status:` field; (4) confirming every named skill/subagent appears in the inventory; (5) optionally invoking `grill-me` on the draft to stress-test it.
