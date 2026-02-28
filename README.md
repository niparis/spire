# Spire

`spire` bootstraps and maintains a Spec-Driven Development workflow in your repository.

It operationalizes two reliability layers from the product spec:
- Spec Quality (gate spec clarity before planning/implementation)
- Session Continuity (feature-scoped session state, no context drift)

## Why The Name "Spire"

We chose `Spire` because it feels intentional and architectural: a high point built
on strong structure, which matches this tool's goal (precise specs -> reliable delivery).

- Short and memorable for CLI use (`spire init`, `spire new`, `spire status`).
- Signals quality and precision instead of a temporary codename.
- Fits the product positioning: premium developer experience for agentic workflows.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/niparis/spire/main/scripts/install.sh | bash
```

Supported installer targets:
- macOS Apple Silicon (`darwin/arm64`)
- Windows Intel x64 (`windows/amd64`, manual binary usage)

## Workflow in 60 Seconds

1. Initialize SDD scaffolding in a project:
   ```bash
   spire init
   ```
2. Create the next feature scaffold:
   ```bash
   spire new
   ```
3. Run your audit/planning flow from the generated spec.
4. Implement in build mode, then run Gate 4 verification in a separate session.
5. Check pipeline state at any time:
   ```bash
   spire status
   ```

## Developer Workflow

1. Initialize once per repository with `spire init`.
2. Optionally run product/architecture planning if those foundations are missing.
3. Run `spire new` and complete the generated feature spec.
4. Use the Plan agent for spec audit + planning, then approve `PLAN.md` and `TASKS.md`.
5. Use the Build agent to implement task-by-task with TDD and keep `changes/<feature>/SESSION.md` updated.
6. Use the Verification agent in a separate session to produce `VERIFICATION_REPORT.md`.
7. Open a PR only when verification verdict is `READY FOR PR`, then merge after CI + human review.

## Full Gate Flow

1. **Bootstrap**
   - Run `spire init` once in the repository to install methodology scaffolding.
   - Use `spire update` whenever you want the latest methodology payload.

2. **Gate 0 - Spec Authoring**
   - Run `spire new` to create a feature spec and session log scaffold.
   - Complete the feature spec fully (goal, journeys, acceptance criteria, NFRs, out-of-scope, open questions).

3. **Gate 1 - Spec Audit**
   - Run the Plan agent to audit the spec before planning.
   - If verdict is `FAIL` or `CONDITIONAL`, resolve issues and re-audit.
   - Only proceed when verdict is `PASS`.

4. **Gate 2 - Planning**
   - Plan mode produces `changes/<feature>/PLAN.md` and `changes/<feature>/TASKS.md`.
   - Human reviews and explicitly approves plan/tasks before implementation starts.

5. **Gate 3 - Implementation Loop**
   - Build mode executes `TASKS.md` in small, test-first steps.
   - For each task: write failing test, implement, run lint/typecheck/tests, then commit.
   - Keep `changes/<feature>/SESSION.md` updated after each task and at session end.

6. **Gate 4 - Verification**
   - Run verification in a separate OpenCode session from implementation.
   - Produce `changes/<feature>/VERIFICATION_REPORT.md` with traceability, command evidence, and verdict.
   - If verdict is `NEEDS WORK`, return to Gate 3.

7. **Gate 5 - PR and Merge**
   - Open PR only when Gate 4 verdict is `READY FOR PR`.
   - Include references to spec, plan, and verification report.
   - Merge after CI + human review, then archive completed change artifacts.

## Command Reference

| Command | Behavior |
|---|---|
| `spire init` | Downloads methodology from the canonical Spire GitHub source, syncs it into `.methodology/`, applies root projections via manifest (for example, `AGENTS.md`), and avoids overwriting existing root files |
| `spire update` | Detects local edits in `.methodology/`, prompts in interactive mode, safely aborts in non-interactive mode, refreshes payload using `.methodology/.spire-source.json` (with canonical fallback), and reports protected-file notices |
| `spire new` | Creates the next numbered feature spec (`max+1`) and `changes/<feature>/SESSION.md` from templates |
| `spire status` | Scans feature artifacts and prints inferred lifecycle state (`Spec only` -> `Awaiting PR` -> `Complete`) |

## File Model

- `.methodology/` is the synced methodology payload managed by `spire`.
- `.methodology/project_root/manifest.json` controls which files are projected to repository root.
- `.methodology/.spire-source.json` stores where methodology was fetched from for deterministic updates.
- Canonical session continuity file is always `changes/[feature]/SESSION.md`.

`spire init` and `spire update` do not require `SPIRE_METHODOLOGY_SOURCE`.

## Versioning and Distribution

- Tags follow `vX.Y.Z` and trigger release builds.
- Release assets are published to GitHub Releases for supported targets.
- Installer defaults to latest release unless overridden via installer env vars.

## Troubleshooting

- `Run spire init first.`: initialize the repository before `update`/feature flows.
- Installer succeeded but `spire` not found: add install directory to your `PATH`.
- `spire update` blocked by local edits: stash or revert local `.methodology/` changes first.

## Verification Independence

- Preferred: run Gate 4 verification in a separate OpenCode session from build mode.
- Minimum: final Gate 4 verdict should not come from the same active implementation run.
- Never open a PR when `VERIFICATION_REPORT.md` verdict is `NEEDS WORK`.

## Related Docs

- `specs/PRODUCT.md`
