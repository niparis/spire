# Complete Spec-Driven Development System for OpenCode

> A single coherent end-to-end system synthesising the best of the three source frameworks,
> plus the two missing layers: **Spec Quality** and **Session Continuity**.

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
[GATE 0] Spec Authoring (human + interview agent)
      │
      ▼
[GATE 1] Spec Audit (plan agent scores & challenges the spec)
      │  ← human resolves questions before proceeding
      ▼
[GATE 2] Planning (plan agent proposes, human approves)
      │
      ▼
[GATE 3] Implementation Loop (coder agent, TDD, SESSION.md)
      │
      ▼
[GATE 4] Verification (tester/reviewer agent + CI)
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
├── specs/                          # Persistent product knowledge
│   ├── product.md                  # Business vision, personas, constraints
│   ├── feature-auth.md             # Feature spec (full schema — see below)
│   └── _template.md                # Spec template (copy for every new feature)
│
├── architecture/
│   ├── adr-001-auth.md             # Architecture Decision Records
│   └── adr-002-cache.md
│
├── changes/                        # Active work, one folder per feature
│   └── auth/
│       ├── PROPOSAL.md             # Design options + chosen direction
│       ├── PLAN.md                 # Approved implementation plan
│       ├── TASKS.md                # Atomic task list (5–10 min each)
│       ├── SESSION.md              # ← NEW: live session continuity file
│       └── VERIFICATION_REPORT.md  # Test + traceability output
│
├── archive/                        # Completed change folders land here
│
├── agents/
│   ├── AGENTS.md                   # Global rules, boundaries, routing
│   └── skills/
│       ├── backend.md
│       ├── frontend.md
│       ├── testing.md
│       ├── security.md
│       └── spec-auditor.md         # ← NEW: spec quality rules
│
├── .github/
│   └── workflows/
│       └── ci.yaml
│
└── opencode.json
```

`SESSION.md` is feature-scoped. There is no root-level `changes/SESSION.md`.
Canonical session state always lives at `changes/[feature]/SESSION.md`.

---

## The Spec Schema (Gate 0 output)

Every feature spec must follow this exact structure. No spec that skips a section
enters the pipeline. The `_template.md` enforces this.

```markdown
# Spec: [Feature Name]
Version: 0.1 | Status: DRAFT | Author: [name] | Date: YYYY-MM-DD

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

The plan agent runs a structured audit *before* it is allowed to propose anything.
This gate exists because unresolved ambiguity in the spec is the single biggest
cause of agent thrashing, wrong implementations, and wasted cycles.

### Spec Auditor Skill (`agents/skills/spec-auditor.md`)

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

### Gate 1 Prompt

```
Read specs/[feature].md.
Load agents/skills/spec-auditor.md.
Run a full spec audit and output the AUDIT REPORT.
Do not proceed to planning until the verdict is PASS.
If CONDITIONAL or FAIL, stop and wait for human input.
```

---

## Gate 2: Planning

Only reached after a PASS audit verdict.

### Plan Agent Prompt

```
Before planning, resolve the active feature slug.

Feature Resolution:
1. If runtime context already provides a concrete feature slug, use it.
2. Otherwise, look for explicit `Feature: <slug>` context from prior session artifacts.
3. If slug is unknown or ambiguous, ask the human and wait.
4. Do not guess from branch names or assumptions.
5. Do not use any root-level `changes/SESSION.md`; only `changes/[feature]/SESSION.md` is valid.

The spec audit has passed for specs/[feature].md.

1. Read specs/[feature].md, specs/PRODUCT.md, agents/SPIRE.md,
   and any relevant architecture/adr-*.md files.

2. List any remaining technical ambiguities (not already in the spec's
   open questions — those are resolved). Output as QUESTIONS.
   Wait for human answers before continuing if any are HIGH priority.

3. Propose 2–3 implementation options with explicit tradeoffs.
   Label each: recommended / alternative / rejected-because.

4. Output changes/[feature]/PLAN.md with:
   - chosen approach and rationale
   - file-by-file change list
   - test strategy (unit / integration / e2e breakdown)
   - rollback plan
   - CI/CD impact

5. Output changes/[feature]/TASKS.md:
   - atomic tasks, each 5–10 minutes of work
   - each task has: goal, files to touch, tests to add, verification step
   - ordered by dependency

NO code changes. NO file edits outside changes/[feature]/. This is Plan mode.
```

Human reviews PLAN.md and TASKS.md. Written approval ("APPROVED" comment or commit
message) is the gate exit condition. The agent does not proceed without it.

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
Current phase: [which task from TASKS.md]

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

**Rules in AGENTS.md for session continuity:**

```markdown
## Session Continuity Rules

RULE SC-1: At the START of any implementation session, before any action,
resolve the active feature slug from runtime context or explicit human input.
If slug is unknown/ambiguous, ask and wait. Then read
changes/[feature]/SESSION.md if it exists. Treat it as ground truth for
current state. Do not use a root-level changes/SESSION.md and do not
re-derive state from git log alone.

RULE SC-2: At the END of every session (or when asked to pause/stop),
update SESSION.md with current status, decisions made, and the next action.
This is not optional. A session that ends without updating SESSION.md is incomplete.

RULE SC-3 (Circuit-breaker): If any single test, lint error, or build step
has failed 3 or more times with different approaches, STOP. Record it in
the Failure Log section of SESSION.md. Do not attempt a 4th approach.
Surface it to the human with a summary of what was tried.

RULE SC-4: Discovered constraints go in SESSION.md immediately.
If a discovered constraint invalidates a task in TASKS.md, flag it before
proceeding. Do not silently work around it.
```

### Implementation Prompt (per session)

```
Implementation session for changes/[feature]/.

Before implementation, resolve the active feature slug.

Feature Resolution:
1. If runtime context already provides a concrete feature slug, use it.
2. Otherwise, prefer an explicit `Feature: <slug>` from session context.
3. If the slug is unknown or ambiguous, ask the human and wait.
4. Do not guess from branch names or assumptions.
5. Do not use any root-level `changes/SESSION.md`; only `changes/[feature]/SESSION.md` is valid.

1. Read changes/[feature]/SESSION.md (if exists) — this is your current state.
2. Read changes/[feature]/TASKS.md — this is your work queue.
3. Read specs/[feature].md — this is your truth.
4. Read agents/SPIRE.md — these are your operating rules.

For each task:
  a. Write the failing test first (derived from the acceptance criterion it satisfies).
  b. Implement until the test passes.
  c. Run: [your lint command] + [your typecheck command] + [your test command].
  d. If all green: commit with message "task: [task name] — satisfies AC-[n]".
  e. If any fail: attempt fix. If same failure occurs 3 times, invoke SC-3.

After each task, update SESSION.md.

At session end, update SESSION.md with full status and next action.
```

---

## Gate 4: Verification

When all tasks in TASKS.md are marked complete:

### Verification Agent Prompt

```
All tasks complete for changes/[feature]/.

Produce changes/[feature]/VERIFICATION_REPORT.md with:

1. TRACEABILITY MATRIX
   For every acceptance criterion in specs/[feature].md:
   AC-n | implemented in [file:line] | tested by [test file:test name] | PASS/FAIL

2. COMMANDS RUN
   Exact commands and full output (truncated to last 50 lines if long).

3. COVERAGE SUMMARY
   Which ACs are fully covered, partially covered, or not covered.

4. SELF-REVIEW
   Compare the code diff against specs/[feature].md.
   Flag any implementation that satisfies the letter but not the intent of the spec.

5. VERDICT
   READY FOR PR | NEEDS WORK (list what)

Do not open a PR if verdict is NEEDS WORK.
```

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
    # Independent agent reads the PR diff against specs/[feature].md
    # and changes/[feature]/VERIFICATION_REPORT.md
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
Spec: specs/[feature].md
Plan: changes/[feature]/PLAN.md
Verification: changes/[feature]/VERIFICATION_REPORT.md

## AC Coverage
[paste traceability matrix from verification report]

## Decisions Made
[paste closed decisions from SESSION.md]

## Discovered Constraints
[paste from SESSION.md — for future spec awareness]
```

After merge: run `/openspec archive` (or manually move `changes/[feature]/` to `archive/`).

---

## AGENTS.md (Complete Starter)

```markdown
# AGENTS.md — Global Agent Governance

## Project Context
[2–3 sentences: what this repo is, tech stack, critical constraints]

## Operating Rules

### Always
- Read SESSION.md at session start, update at session end (SC-1, SC-2)
- Write failing tests before implementation (TDD)
- Run lint + typecheck + tests after every logical changeset
- Commit with format: "type: description — satisfies AC-n"
- Log all decisions in SESSION.md under Closed Decisions

### Ask Before
- Modifying any file outside the current feature's task scope
- Changing shared configuration (opencode.json, package.json, CI config)
- Adding new dependencies
- Deleting files

### Never
- Skip Gate 1 (spec audit) for any new feature
- Proceed past a FAIL or unresolved CONDITIONAL audit verdict
- Attempt the same failing fix more than 3 times (invoke SC-3 instead)
- Open a PR without a VERIFICATION_REPORT.md
- Modify files in archive/ or specs/ during implementation

## Commands
Test:      [npm test / pytest / go test ./...]
Lint:      [eslint . / ruff check . / golangci-lint run]
Typecheck: [tsc --noEmit / mypy . / go vet ./...]
CI local:  [act / make ci]

## Spec Audit Threshold
Minimum score to proceed: 40/50
Blocking issue count to halt: any

## Session Continuity
See SC-1 through SC-4 in Session Continuity Rules section above.

## Circuit-Breaker
3 failures on the same step = STOP and escalate. See SC-3.

## Skills
Load conditionally via opencode.json instructions array:
- agents/skills/spec-auditor.md  (always loaded in plan agent)
- agents/skills/backend.md       (load for backend tasks)
- agents/skills/frontend.md      (load for frontend tasks)
- agents/skills/testing.md       (always loaded)
- agents/skills/security.md      (load for auth/data tasks)
```

---

## opencode.json (Complete Starter)

```json
{
  "instructions": [
    "agents/SPIRE.md",
    "specs/PRODUCT.md"
  ],
  "agents": {
    "plan": {
      "instructions": [
        "agents/SPIRE.md",
        "agents/skills/spec-auditor.md",
        "specs/PRODUCT.md"
      ],
      "permissions": {
        "edit": "ask",
        "bash": "ask"
      }
    },
    "default": {
      "instructions": [
        "agents/SPIRE.md",
        "agents/skills/testing.md",
        "specs/PRODUCT.md"
      ],
      "permissions": {
        "bash": {
          "git *":      "allow",
          "npm test":   "allow",
          "npm run *":  "allow",
          "pytest *":   "allow",
          "rm *":       "deny",
          "curl *":     "ask"
        }
      }
    }
  },
  "mcp": {
    "filesystem": { "enabled": true },
    "git":        { "enabled": true },
    "test-runner":{ "enabled": true },
    "browser":    { "enabled": false }
  }
}
```

---

## Complete Flow — One Page

```
HUMAN WRITES SPEC (using _template.md)
        │
        ▼
[GATE 0] Spec complete? All 7 sections filled? Open questions resolved?
        │ yes
        ▼
[GATE 1] SPEC AUDIT — plan agent scores spec (spec-auditor.md)
        │ PASS (≥40/50)        ← FAIL/CONDITIONAL: human fixes spec, re-audit
        ▼
[GATE 2] PLANNING — plan agent outputs PLAN.md + TASKS.md
        │ human approves       ← revise loop if needed
        ▼
[GATE 3] IMPLEMENTATION LOOP
        │  ┌─ read SESSION.md
        │  ├─ read TASKS.md current task
        │  ├─ write failing test
        │  ├─ implement
        │  ├─ lint + typecheck + test
        │  ├─ green? → commit, update SESSION.md, next task
        │  └─ 3 failures? → circuit-breaker SC-3, escalate to human
        │ all tasks done
        ▼
[GATE 4] VERIFICATION — tester/reviewer agent produces VERIFICATION_REPORT.md
        │ READY FOR PR         ← NEEDS WORK: back to Gate 3
        ▼
[GATE 5] PR OPENED
        │  ├─ CI runs (lint, tests, security, coverage)
        │  ├─ LLM-as-judge reviews diff vs spec
        │  └─ human reviews
        │ approved
        ▼
MERGE → archive changes/[feature]/ → update specs if behaviour changed
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
