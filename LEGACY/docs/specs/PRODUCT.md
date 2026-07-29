# Complete Spec-Driven Development System for OpenCode

> A single coherent end-to-end system synthesising the best of the three source frameworks,
> plus the two missing layers: **Spec Quality** and **Session Continuity**.

> **Canonical operating contract:** `.methodology/agents/SPIRE.md` is the
> authoritative, always-loaded description of the workflow. This document is the
> product vision and design rationale; where details differ, SPIRE.md wins.
> The system runs on OpenCode's built-in `plan`/`build` modes plus **skills** and
> **subagents** — there are no custom primary agents. All SDD artifacts live
> under a `docs/` root.

---

## The Core Principle

```
Bad input → beautiful process → bad output.
```

Every existing framework optimises the execution layer. This system also governs what enters it.
The agent is treated as a skilled but literal contractor: it executes exactly what it reads.
Your job is to give it something worth executing.

The full pipeline is:

```
Stakeholder Intent
      │
      ▼
[GATE 0] Spec Authoring (new-feature skill, plan mode)
      │
      ▼
[GATE 1] Spec Audit (spec-auditor subagent scores & challenges the spec)
      │  ← human resolves questions before proceeding
      ▼
[GATE 2] Planning (planner subagent proposes, human approves)
      │
      ▼
[GATE 3] Implementation Loop (implementation-loop skill, build mode, TDD, SESSION.md)
      │
      ▼
[GATE 4] Verification (verifier subagent + CI)
      │
      ▼
[GATE 5] PR & Merge (human review + LLM-as-judge)
```

No gate is skippable. Each gate has an explicit entry condition and exit artefact.

---

## Repository Structure

```
repo/
│
├── docs/
│   ├── specs/                       # Persistent product knowledge
│   │   ├── PRODUCT.md               # Business vision, personas, constraints
│   │   ├── feature-001-auth.md      # Feature spec (full schema — see below)
│   │   └── _template.md             # Spec template (copy for every new feature)
│   │
│   ├── architecture/
│   │   ├── ARCHITECTURE.md           # System architecture
│   │   ├── adr-001-auth.md           # Architecture Decision Records
│   │   └── adr-002-cache.md
│   │
│   ├── changes/                      # Active work, one folder per feature
│   │   └── 001-auth/
│   │       ├── AUDIT.md              # Gate 1 spec-auditor verdict
│   │       ├── PLAN.md               # Approved plan + embedded ordered tasks
│   │       ├── SESSION.md            # Live session continuity file
│   │       └── VERIFICATION_REPORT.md
│   │
│   └── archive/                      # Completed change folders land here
│
├── .methodology/
│   ├── agents/SPIRE.md              # Global governance (always loaded)
│   ├── skills/                      # product-definition, architecture-definition,
│   │                                #   new-feature, grill-me, implementation-loop
│   └── subagents/                   # spec-auditor, planner, verifier, docs-writer, investigator
│
├── .opencode/agents/                # Subagents projected here for discovery
│
├── .github/workflows/ci.yaml
├── AGENTS.md                        # Project commands + local rules
└── opencode.json
```

There is no separate `TASKS.md` or `PROPOSAL.md` — the task list lives inside
`PLAN.md`, and the spec is the Gate-0 artifact.

`SESSION.md` is feature-scoped. There is no root-level `changes/SESSION.md`.
Canonical session state always lives at `docs/changes/[feature]/SESSION.md`.

---

## The Spec Schema (Gate 0 output)

Every feature spec must follow this exact structure. No spec that skips a section
enters the pipeline. The `_template.md` enforces this.

```markdown
# Spec: [Feature Name]
Version: 0.1 | Author: [name] | Date: YYYY-MM-DD

## 1. Goal
One sentence. What problem does this solve and for whom?

## 2. Actors
List every user type and system actor that interacts with this feature.

## 3. User Journeys
Format: Given [context] / When [action] / Then [observable outcome]
Write one GWT block per distinct journey. Include the unhappy paths.

## 4. Acceptance Criteria
Numbered list. Each criterion must be:
  - independently testable (no "and" in a single criterion)
  - falsifiable (a test can definitively pass or fail it)
  - free of implementation detail

## 5. Non-Functional Requirements
Performance, security, accessibility, scalability — specific and measurable.
"Fast" is not a valid NFR. "p95 response < 200ms under 1000 concurrent users" is.

## 6. Out of Scope
Explicit list of what this spec does NOT cover. This prevents scope creep in planning.

## 7. Open Questions
Unresolved ambiguities. Every item here BLOCKS the spec from entering Gate 1.
Format: Q1: [question] | Owner: [person] | Due: [date]
```

---

## Gate 1: Spec Audit (NEW)

This is the layer that was missing from all three source frameworks.

The `spec-auditor` subagent runs a structured audit *before* anything is proposed.
This gate exists because unresolved ambiguity in the spec is the single biggest
cause of agent thrashing, wrong implementations, and wasted cycles.

### Spec Auditor (`spec-auditor` subagent → `.methodology/agents/SPEC_AUDITOR.md`)

```markdown
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
```

### Gate 1 dispatch

Dispatch the `spec-auditor` subagent. It reads `docs/specs/feature-[feature].md`,
runs the audit, and writes `docs/changes/[feature]/AUDIT.md`. Do not proceed to
planning until the verdict is PASS; on CONDITIONAL or FAIL, stop and wait for
human input.

---

## Gate 2: Planning

Only reached after a PASS audit verdict.

Invoked from `plan` mode once the audit passes, the `planner` subagent (canonical
prompt: `.methodology/agents/FEATURE_PLANNER.md`):

1. Reads `docs/specs/feature-[feature].md`, `docs/specs/PRODUCT.md`,
   `.methodology/agents/SPIRE.md`, and any relevant
   `docs/architecture/adr-*.md`.
2. Lists remaining technical ambiguities as QUESTIONS and waits on HIGH ones.
3. Proposes 2–3 options (recommended / alternative / rejected-because).
4. Writes a single `docs/changes/[feature]/PLAN.md`: chosen approach + rationale,
   file-by-file change list, test strategy, rollback plan, CI/CD impact, and an
   **ordered task list** (each task: goal, files, tests, verification, the AC it
   satisfies). No separate `PLAN.md`.

It then presents the plan. Human approval is the gate exit condition; the planner
makes no code changes and no edits outside `docs/changes/[feature]/`.

---

## Gate 3: Implementation Loop

### Session Continuity Protocol (NEW)

This is the second missing layer. Every implementation session begins and ends
with SESSION.md. This file is the agent's working memory across sessions.

**SESSION.md structure:**

```markdown
# Session Log: [Feature Name]
Last updated: YYYY-MM-DD HH:MM | Agent: [model used]

## Status
Overall: [% complete estimate]
Current phase: [which task from PLAN.md]

## Completed (with commit refs)
- [task description] → commit abc1234
- [task description] → commit def5678

## In Progress
- [task description]
  - What's done: [...]
  - What's next: [...]
  - Blockers: [none | description]

## Closed Decisions (do not re-litigate)
- [decision]: [one-line rationale] → decided in session YYYY-MM-DD

## Discovered Constraints (not in original spec)
- [constraint]: [impact on plan]

## Failure Log (circuit-breaker)
- [test/step name]: failed [N] times. Approach tried: [...]. ESCALATE if N ≥ 3.

## Next Action
[Single, specific next step the next session should start with]
```

The session-continuity rules (SC-1 start, SC-2 end, SC-3 circuit-breaker,
SC-4 constraints) are canonical in `.methodology/agents/SPIRE.md` §6.

The loop itself runs in `build` mode under the `implementation-loop` skill
(`.methodology/skills/implementation-loop/SKILL.md`): read
`docs/changes/[feature]/SESSION.md` → `PLAN.md` →
`docs/specs/feature-[feature].md` → SPIRE.md; then per task write the failing
test, implement, run lint/typecheck/tests, commit
`type: description — satisfies AC-n`, and update `SESSION.md`. On the 3rd
identical failure, invoke SC-3. When all tasks are done, hand off to the
`verifier` subagent.

---

## Gate 4: Verification

When all tasks in PLAN.md are marked complete, dispatch the `verifier` subagent
(canonical prompt: `.methodology/agents/VERIFICATION.md`). It performs a gap
analysis (behaviour vs spec) and writes
`docs/changes/[feature]/VERIFICATION_REPORT.md` with: a traceability matrix
(`AC-n | implemented in file:line | tested by test:name | PASS/FAIL`), commands
run with evidence, coverage summary, self-review against spec intent, and a
verdict (`READY FOR PR` | `NEEDS WORK`). Never open a PR on `NEEDS WORK`.
Default is the subagent; escalate to a separate session for high-risk features.

---

## Gate 5: PR & Merge

### GitHub Actions CI (`ci.yaml` outline)

```yaml
on:
  pull_request:
  issue_comment:
    types: [created]  # triggers on /oc or /opencode comments

jobs:
  spec-validation:
    # Independent agent reads the PR diff against docs/specs/feature-[feature].md
    # and docs/changes/[feature]/VERIFICATION_REPORT.md
    # Comments on PR with: spec coverage gaps, risky changes, missing tests

  quality-gates:
    steps:
      - lint
      - typecheck
      - unit-tests
      - integration-tests
      - security-scan
      - coverage-threshold  # fail if below project minimum

  # All must pass before merge is unblocked
```

### PR Description Template

```markdown
## Feature: [name]
Spec: docs/specs/feature-[feature].md
Plan: docs/changes/[feature]/PLAN.md
Verification: docs/changes/[feature]/VERIFICATION_REPORT.md

## AC Coverage
[paste traceability matrix from verification report]

## Decisions Made
[paste closed decisions from SESSION.md]

## Discovered Constraints
[paste from SESSION.md — for future spec awareness]
```

After merge: move `docs/changes/[feature]/` to `docs/archive/[feature]/`.

---

## AGENTS.md

The operating rules (TDD, commit format, the Always/Never lists, the spec-audit
threshold, and the SC-1..SC-4 session-continuity + circuit-breaker rules) are
canonical in `.methodology/agents/SPIRE.md` (§5–§7) and must not be duplicated
here. The project-local `AGENTS.md` carries only project commands (test / lint /
typecheck), tech-stack constraints, and local rules; the methodology projects a
starter to it on `spire init` (see `.methodology/project_root/local_agents.md`).

---

## opencode.json

Work runs in OpenCode's built-in `plan` and `build` modes — there are no custom
primary agents. `opencode.json` only sets the always-loaded `instructions`
(`.methodology/agents/SPIRE.md`, `AGENTS.md`, `docs/specs/PRODUCT.md`).
Subagents (`spec-auditor`, `planner`, `verifier`, `docs-writer`, `investigator`)
are projected to `.opencode/agents/*.md` by the manifest; skills live under
`.methodology/skills/`. The canonical template is
`.methodology/project_root/opencode.json`.

---

## Complete Flow — One Page

```
HUMAN WRITES SPEC (using _template.md)
        │
        ▼
[GATE 0] Spec complete? All 7 sections filled? Open questions resolved?
        │ yes
        ▼
[GATE 1] SPEC AUDIT — spec-auditor subagent scores spec → AUDIT.md
        │ PASS (≥40/50)        ← FAIL/CONDITIONAL: human fixes spec, re-audit
        ▼
[GATE 2] PLANNING — planner subagent outputs PLAN.md (approach + ordered tasks)
        │ human approves       ← revise loop if needed
        ▼
[GATE 3] IMPLEMENTATION LOOP — implementation-loop skill (build mode)
        │  ┌─ read SESSION.md
        │  ├─ read PLAN.md current task
        │  ├─ write failing test
        │  ├─ implement
        │  ├─ lint + typecheck + test
        │  ├─ green? → commit, update SESSION.md, next task
        │  └─ 3 failures? → circuit-breaker SC-3, escalate to human
        │ all tasks done
        ▼
[GATE 4] VERIFICATION — verifier subagent produces VERIFICATION_REPORT.md
        │ READY FOR PR         ← NEEDS WORK: back to Gate 3
        ▼
[GATE 5] PR OPENED
        │  ├─ CI runs (lint, tests, security, coverage)
        │  ├─ LLM-as-judge reviews diff vs spec
        │  └─ human reviews
        │ approved
        ▼
MERGE → archive docs/changes/[feature]/ → update PRODUCT/ARCHITECTURE if behaviour changed
```

---

## What This System Gives You That the Others Don't

The three source frameworks gave you a solid execution machine. This system
adds the two layers that make it reliable in practice:

**Spec Quality (Gate 1)** means the agent never begins planning against a spec
that would cause it to thrash. The auditor is not a formality — it is a hard
gate. A score below 40 means nothing downstream runs. This single addition
eliminates most of the "the agent built the wrong thing" failures.

**Session Continuity (SESSION.md + SC rules)** means multi-day features are
first-class citizens. The agent re-orients instantly at session start, never
re-litigates closed decisions, and surfaces stuck states to humans rather than
burning cycles on hopeless retries. The circuit-breaker alone saves hours on
complex features.

Together they change the system from "a well-organised way to run an agent"
into "a reliable pipeline where quality is governed at entry, not discovered
at review."
