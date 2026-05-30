# Spec: Update Sync Cleanup & Skill Projection
Version: 0.1 | Status: DRAFT | Author: spire | Date: 2026-05-29

## 1. Goal
Improve `spire update` so that `.opencode/` stays in sync with the shipped methodology: stale projections are removed, and human-invoked skills become discoverable by OpenCode through a `spire-` prefixed copy into `.opencode/skills/`.

## 2. Actors
- **Developer** — runs `spire update` in a project that already uses the SDD methodology.
- **OpenCode agent** — discovers skills under `.opencode/skills/` when a human invokes them by name.
- **spire CLI** — orchestrates the sync and cleanup.

## 3. User Journeys

### Journey 1: Happy-path update with cleanup
Given a project where `.methodology/` exists and `.opencode/agents/` contains an old `planner.md` that is no longer in the manifest  
When the developer runs `spire update`  
Then spire refreshes `.methodology/` from the canonical source, applies manifest mappings, copies human-invoked skills to `.opencode/skills/`, and removes the stale `planner.md` from `.opencode/agents/`

### Journey 2: Skill discoverability
Given a project where `spire init` was run with an older spire version that did not copy skills  
When the developer runs `spire update`  
Then `.opencode/skills/spire-product-definition/SKILL.md` (and the other three human-invoked skills) are created, allowing the human to load them by name in OpenCode

### Journey 3: No unintended deletion
Given a project where `.opencode/` contains a file `custom-agent.md` that was created manually by the developer  
When the developer runs `spire update`  
Then `custom-agent.md` is preserved because it was never copied by spire

## 4. Acceptance Criteria

1. `spire update` detects files in `.opencode/agents/` whose source mapping no longer exists in the current manifest and deletes them.
2. `spire update` detects directories in `.opencode/skills/` whose corresponding source skill no longer exists in the human-invoked skills list and deletes them.
3. The four human-invoked skills (`product-definition`, `new-feature`, `grill-me`, `architecture-definition`) are copied from `.methodology/skills/` to `.opencode/skills/` with a `spire-` prefixed subdirectory name and `SKILL.md` as the entrypoint file.
4. Auto-loaded skills (`implementation-loop`, `spec-auditor`) are NOT copied to `.opencode/skills/`.
5. Stale-file cleanup operates only inside `.opencode/agents/` and `.opencode/skills/`; project-root mapped files (e.g. `AGENTS.md`, `opencode.json`) are never deleted by spire.
6. Files in `.opencode/` that were never created by spire (no matching source in manifest or skill list) are left untouched.
7. All changes are reported in stdout: created skills, removed stale files, and notices for skipped manual files.
8. The update command exits with code 0 when sync succeeds, and code 1 on any filesystem or manifest error.

## 5. Non-Functional Requirements
- Update remains idempotent: running `spire update` twice in a row produces the same state with no spurious changes reported on the second run.
- Skill copying preserves file permissions and uses atomic write-then-rename to avoid partial files.
- The feature adds no external Go dependencies (stdlib only).

## 6. Out of Scope
- Renaming or restructuring the source files inside `.methodology/skills/`.
- Supporting custom skill lists or user-configurable prefixes.
- Cleaning stale files outside `.opencode/` (e.g. deleting old `docs/` templates).
- Backwards migration for pre-existing skills that may have been manually copied by users.

## 7. Open Questions
None.
