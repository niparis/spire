# Spec: Spire Prefix for Skill Names and Methodology Cleanup
Version: 0.1 | Author: spire | Date: 2026-05-30

## 1. Goal
Fix two bugs in the `spire update` sync system: human-invoked skills must expose a `spire-` prefixed `name` in their YAML frontmatter for OpenCode discovery, and stale files must be removed from `.methodology/` (not just `.opencode/`) so the synced directory stays an exact mirror of the source tarball.

## 2. Actors
- **Developer** — runs `spire update` and expects `.methodology/` and `.opencode/skills/` to stay clean and consistent.
- **OpenCode agent** — discovers skills by their frontmatter `name:` field; relies on the `spire-` prefix for disambiguation.
- **spire CLI** — copies skills and syncs the methodology payload.

## 3. User Journeys

### Journey 1: Skill discoverability by name
Given a project where `spire update` has copied skills to `.opencode/skills/`  
When the developer loads a skill by name in OpenCode  
Then the skill's frontmatter `name:` field is `spire-product-definition` (not `product-definition`), matching the directory name and making discovery unambiguous.

### Journey 2: Methodology directory stays clean
Given a project where the previous `spire update` synced `.methodology/skills/backend.md`  
When the upstream source removes `skills/backend.md` and the developer runs `spire update`  
Then `.methodology/skills/backend.md` is deleted, and the directory no longer contains stale files.

### Journey 3: Empty directories cleaned up
Given a project where removing a stale file leaves an empty directory under `.methodology/`  
When the developer runs `spire update`  
Then the empty directory is removed, keeping `.methodology/` tidy.

### Journey 4: Sync-state preserved
Given `.methodology/.spire-sync-state.json` exists  
When `spire update` runs and cleans stale files  
Then `.spire-sync-state.json` and `.spire-source.json` are never deleted.

### Journey 5: Idempotent update
Given a clean sync state  
When the developer runs `spire update` twice in a row  
Then the second run reports no changes and no removals.

## 4. Acceptance Criteria

1. The YAML frontmatter `name:` field in `methodology/skills/product-definition/SKILL.md` is `spire-product-definition`.
2. The YAML frontmatter `name:` field in `methodology/skills/new-feature/SKILL.md` is `spire-new-feature`.
3. The YAML frontmatter `name:` field in `methodology/skills/grill-me/SKILL.md` is `spire-grill-me`.
4. The YAML frontmatter `name:` field in `methodology/skills/architecture-definition/SKILL.md` is `spire-architecture-definition`.
5. `spire update` removes files from `.methodology/` that were present in the previous sync but no longer exist in the source tarball.
6. `spire update` removes empty directories from `.methodology/` after stale file removal (excluding the `.methodology/` root itself).
7. Files named exactly `.spire-sync-state.json` or `.spire-source.json` are never deleted during `.methodology/` cleanup.
8. `spire update` stdout reports removed stale files in `.methodology/` with the prefix "removed stale methodology:".
9. The feature adds no external Go dependencies (stdlib only).
10. `go vet ./...` and `go test ./...` pass after the change.

## 5. Non-Functional Requirements
- Skill copying preserves file permissions.
- The update remains idempotent: running twice produces the same state with no spurious change or removal messages.
- No user-created files outside `.methodology/` are affected.

## 6. Out of Scope
- Renaming the auto-loaded skills (`implementation-loop`, `spec-auditor`) — only the 4 human-invoked skills get the prefix.
- Modifying skill behavior or content beyond the frontmatter `name:` field.
- Adding new skills.
- Changing the `--force` flag behavior.
- Cleaning files outside `.methodology/` and `.opencode/`.

## 7. Open Questions
None.
