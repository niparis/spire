# Changelog

## 1.0.2

- Restructure `product-definition` and `architecture-definition` skills from flat files to `skill-name/SKILL.md` subfolders with YAML frontmatter, matching the convention used by other skills.
- Reword `spire update` dirty-files warning to explain that continuing will overwrite local changes with upstream versions.

## 1.0.1

- `spire update` now removes stale projections from `.opencode/agents/` and `.opencode/skills/` that are no longer in the manifest or human-invoked skills list.
- Human-invoked skills (`product-definition`, `new-feature`, `grill-me`, `architecture-definition`) are now copied to `.opencode/skills/spire-*/SKILL.md` during `spire init` and `spire update`, making them discoverable by OpenCode.
- Sync state (`.spire-sync-state.json`) now tracks projected files to distinguish spire-managed files from user-created ones.

## 1.0.0

Breaking changes — the methodology is rebuilt around OpenCode's built-in modes:

- Run entirely on the built-in `plan` and `build` modes plus **skills** and **subagents**; custom primary agents (`build-feature`, `productengineer`) are removed.
- SDD artifacts now live under a `docs/` root (`docs/specs/`, `docs/architecture/`, `docs/changes/`, `docs/archive/`).
- Removed the `spire new` and `spire status` commands. Feature scaffolding is now the `new-feature` skill and lifecycle state is read from the filesystem inside OpenCode. The CLI is bootstrap/maintenance only (`init`, `update`, `upgrade`).

Methodology:

- Rewrote `SPIRE.md` as the single authoritative workflow: Gates 0–5, the code-production loop (SC-1..SC-4), filesystem-inferred state, and the one-time/regular/as-needed cadence.
- Consolidated subagents: `spec-auditor` (Gate 1, writes `AUDIT.md`), `planner` (Gate 2, single `PLAN.md`), `verifier` (Gate 4, gap analysis + `VERIFICATION_REPORT.md`). Retired `featureplanner`, `review_against_spec`, and `specs_reviewer`.
- New skills `new-feature` and `implementation-loop`; renamed `grill_me` → `grill-me`; folded `CODE.md` and `ARCHITECTURE.md` into the relevant skills.
- Single `PLAN.md` with an embedded task list (no `TASKS.md` or `PROPOSAL.md`); dropped the spec-header `Status:` field in favour of filesystem state.
- Updated `opencode.json`, `manifest.json`, the README, and `docs/specs/PRODUCT.md` to match.

## 0.5.0

- Add `spire update --force` to overwrite protected project-root projections such as `opencode.json`.
- Show a `--force` hint when upstream protected files change but local files are kept.

## 0.1.0

- Initial Go CLI scaffold for `spire`.
- `spire init` now resolves methodology source automatically from canonical GitHub distribution.
- `spire update` uses `.methodology/.spire-source.json` metadata for deterministic refresh (with canonical fallback).
- No required runtime `SPIRE_METHODOLOGY_SOURCE` environment variable.
