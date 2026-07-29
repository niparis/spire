# PLAN.md — Update Sync Cleanup & Skill Projection
Feature: 002-update-sync-cleanup
Status: APPROVED
Date: 2026-05-29

---

## Context

`spire update` currently syncs `.methodology/` from the canonical GitHub source and applies `project_root/manifest.json` mappings to the project root. However, it has two gaps:

1. **Stale projections persist**: when a subagent is removed from the manifest, its old copy in `.opencode/agents/` remains indefinitely.
2. **Skills are invisible to OpenCode**: human-invoked skills live only in `.methodology/skills/`, which OpenCode does not scan for discoverable skills.

This feature closes both gaps while preserving user-created files in `.opencode/`.

---

## Chosen Approach

**Extend `spire update` with two new phases: stale-cleanup and skill-projection, backed by extended sync-state tracking.**

### Rationale

- **Why extend sync state?** We must distinguish spire-managed files from user-created files in `.opencode/` to avoid deleting user work (AC6). Tracking projections in `.spire-sync-state.json` is the simplest durable mechanism.
- **Why hardcode the skill list?** The 4 human-invoked skills are a fixed methodology contract. Making them configurable adds complexity with no current use-case.
- **Why prefix with `spire-`?** OpenCode discovers skills by subdirectory name under `.opencode/skills/`. Prefixing prevents collisions with project-local skills and makes intent explicit.

### Alternatives considered

- **Scan `.opencode/` and delete anything not in manifest** — rejected: would delete user-created custom agents/skills.
- **Use a separate tracking file** — rejected: adds file complexity; extending existing sync state is simpler and atomic.
- **Copy ALL skills including auto-loaded** — rejected: auto-loaded skills (`implementation-loop`, `spec-auditor`) are consumed by the agent runtime directly from `.methodology/` via `opencode.json` instructions. Copying them would create duplication and confusion.

---

## File-by-File Change List

```
internal/methodology/update.go
  - Extend syncState struct with Projections map
  - Update read/write helpers to handle new field (backward-compatible)

internal/scaffold/project_root.go
  - Add CleanupOpencode(projectRoot, expectedAgents, expectedSkills, trackedProjections) function
  - Add buildExpectedAgentSet(manifest) helper

internal/scaffold/skills.go  (NEW)
  - Define HumanInvokedSkills []SkillMapping
  - Add ApplySkillProjections(methodologyDir, projectRoot, out) function
  - Add buildExpectedSkillSet() helper
  - Handle both flat-file skills (*.md) and directory skills (*/SKILL.md)

internal/commands/update.go
  - After ApplyProjectRootUpdateMappings, call ApplySkillProjections
  - After skill projection, call CleanupOpencode with tracked + expected sets
  - Report created/removed skills and stale files in stdout

internal/scaffold/skills_test.go  (NEW)
  - Unit tests for skill mapping logic, flat vs directory handling, prefixing

internal/commands/update_test.go
  - Integration tests for update cleanup behavior and skill projection

internal/scaffold/project_root_test.go
  - Tests for CleanupOpencode with tracked vs untracked files
```

---

## Detailed Design

### Sync State Extension

Current `.spire-sync-state.json`:
```json
{
  "hashes": { "agents/SPIRE.md": "abc..." }
}
```

Extended:
```json
{
  "hashes": { "agents/SPIRE.md": "abc..." },
  "projections": {
    ".opencode/agents/spec-auditor.md": "def...",
    ".opencode/skills/spire-product-definition/SKILL.md": "ghi..."
  }
}
```

The `projections` field is optional for backward compatibility. Old states without it are treated as "no tracked projections", so cleanup is a no-op until the first update with this feature.

### Stale Cleanup Algorithm

1. Build `expectedAgents` = set of destinations from manifest where dest starts with `.opencode/agents/`
2. Build `expectedSkills` = set of directories under `.opencode/skills/` derived from HumanInvokedSkills
3. Build `expectedProjections` = union of expectedAgents + expectedSkills
4. Read `trackedProjections` from sync state
5. For each path in `trackedProjections`:
   - If not in `expectedProjections`, delete the file/directory
   - Log "removed stale: <path>"
6. Write new `projections` = current files actually present after sync (only spire-managed ones)

### Skill Projection Mapping

| Source | Destination |
|--------|-------------|
| `.methodology/skills/product-definition.md` | `.opencode/skills/spire-product-definition/SKILL.md` |
| `.methodology/skills/new-feature/SKILL.md` | `.opencode/skills/spire-new-feature/SKILL.md` |
| `.methodology/skills/grill-me/SKILL.md` | `.opencode/skills/spire-grill-me/SKILL.md` |
| `.methodology/skills/architecture-definition.md` | `.opencode/skills/spire-architecture-definition/SKILL.md` |

Copy uses `copyFile` with atomic semantics (write temp + rename) and preserves permissions.

---

## Test Strategy

**Unit tests:**
- `TestHumanInvokedSkills` — verifies the 4 skill mappings are defined and paths are correct
- `TestApplySkillProjections` — creates temp `.methodology/skills/`, runs projection, asserts destinations exist
- `TestApplySkillProjectionsFlatFile` — handles `product-definition.md` → `spire-product-definition/SKILL.md`
- `TestApplySkillProjectionsDirectory` — handles `new-feature/SKILL.md` → `spire-new-feature/SKILL.md`
- `TestCleanupOpencode` — tracked stale file removed, untracked file preserved
- `TestCleanupOpencodeSkills` — tracked stale skill dir removed, untracked skill dir preserved
- `TestSyncStateBackwardCompatibility` — old sync state without `projections` parses correctly

**Integration tests:**
- `TestUpdate_RemovesStaleAgent` — full update flow: old agent in `.opencode/agents/`, removed from manifest, assert deletion
- `TestUpdate_CreatesSkills` — full update flow: assert 4 skill directories created
- `TestUpdate_LeavesCustomFiles` — full update flow: custom file in `.opencode/agents/`, assert preserved
- `TestUpdate_Idempotent` — run twice, assert no changes on second run

---

## Rollback Plan

- Stale cleanup is reversible by re-running `spire update` from a version that includes the removed mapping/skill.
- Skill projections are deletable manually if they cause issues.
- No project source code is touched.
- Worst case: delete `.opencode/` and re-run `spire update` to reconstruct.

---

## CI/CD Impact

- No CI workflow changes required.
- New test files must pass in existing `go test ./...` job.

---

## Gate 4 Handoff Criteria

- Verification agent runs `go test ./...` and confirms all new tests pass.
- Verification agent produces `docs/changes/002-update-sync-cleanup/VERIFICATION_REPORT.md` with traceability matrix mapping each AC to test coverage.
- PR is blocked if verification verdict is `NEEDS WORK`.

---

## Open Questions
None.
