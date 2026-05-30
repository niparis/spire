# Spec: Skill Consistency and Update Warning Clarity
Version: 0.1 | Status: DRAFT | Author: spire | Date: 2026-05-30

## 1. Goal
Restructure flat-file skills to match the `skill-name/SKILL.md` + frontmatter convention, and make the `spire update` dirty-files warning explain the consequence and resolution clearly.

## 2. Actors
- **Developer** — runs `spire update` and inspects `.methodology/skills/`.
- **OpenCode agent** — discovers skills under `.opencode/skills/`.
- **spire CLI** — ships the methodology payload.

## 3. User Journeys

### Journey 1 — Skill consistency
Given `methodology/skills/product-definition.md` exists as a flat file without frontmatter  
When a developer inspects the skills directory  
Then every skill follows the same `skill-name/SKILL.md` structure with `name` and `description` YAML frontmatter

### Journey 2 — Clear update warning
Given `.methodology/project_root/manifest.json` has local edits  
When the developer runs `spire update`  
Then the warning explains that continuing will overwrite the listed files with upstream changes, and prompts for confirmation

## 4. Acceptance Criteria

1. `methodology/skills/product-definition.md` is moved to `methodology/skills/product-definition/SKILL.md` with proper YAML frontmatter (`name: product-definition`, `description: ...`).
2. `methodology/skills/architecture-definition.md` is moved to `methodology/skills/architecture-definition/SKILL.md` with proper YAML frontmatter (`name: architecture-definition`, `description: ...`).
3. The `HumanInvokedSkills` mapping in the Go CLI is updated to reference the new `SKILL.md` paths.
4. Test helpers that create fake methodology sources write skills to the new subfolder paths.
5. The `DetectDirty` warning message in `spire update` explains that the listed files differ from the last synced state and will be overwritten if the user continues.
6. The interactive mode prompt remains a `Continue? [y/N]` but the warning line preceding it now includes the consequence.
7. No external Go dependencies are added.

## 5. Non-Functional Requirements
- `go build ./...`, `go vet ./...`, and `go test ./...` must pass after the change.
- The change is backward-compatible for end-users: old `.opencode/skills/` copies are tracked or untracked correctly by the projection system.

## 6. Out of Scope
- Changing the content of skills beyond adding frontmatter.
- Changing skill behavior or functionality.
- Adding new skills.
- Modifying the `--force` flag behavior.

## 7. Open Questions
None.
