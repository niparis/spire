# PLAN.md — Skill Consistency and Update Warning Clarity
Feature: 003-skill-consistency
Status: APPROVED
Date: 2026-05-30

---

## Context

The shipped methodology skills have an inconsistent directory layout:
- `new-feature/SKILL.md`, `grill-me/SKILL.md`, `implementation-loop/SKILL.md` follow the proper structure (subfolder + SKILL.md + YAML frontmatter).
- `product-definition.md` and `architecture-definition.md` are flat files without frontmatter.

Additionally, the `spire update` dirty-files warning is cryptic. It lists files but does not explain that continuing will overwrite them with upstream changes.

This feature fixes both issues with minimal, safe changes.

---

## Chosen Approach

**Two independent mechanical changes: rename skills in the shipped payload, and reword the CLI warning.**

### Rationale

- **Why rename rather than duplicate?** The user explicitly wants consistency. Flat files are an accidental layout from an earlier iteration. Renaming aligns all skills with the `skill-name/SKILL.md` convention that OpenCode expects for skill discovery.
- **Why reword the warning?** The current message (`warning: local edits detected in .methodology:`) is factual but lacks consequence. Adding "continuing will overwrite your local changes with upstream versions" makes the risk explicit.

### Alternatives considered

- **Keep flat files and add a compatibility shim** — rejected: adds permanent technical debt for no benefit; the user wants consistency.
- **Add frontmatter to flat files in place** — rejected: still leaves an inconsistent directory structure (`skill.md` vs `skill/SKILL.md`).

---

## File-by-File Change List

```
methodology/skills/product-definition.md
  -> methodology/skills/product-definition/SKILL.md
  (move + add YAML frontmatter)

methodology/skills/architecture-definition.md
  -> methodology/skills/architecture-definition/SKILL.md
  (move + add YAML frontmatter)

internal/scaffold/skills.go
  - Update HumanInvokedSkills Source paths for the two renamed skills

internal/commands/init_test.go
  - Update createMethodologySource helper to write skills to new subfolder paths

internal/commands/update.go
  - Reword dirty-files warning to include consequence
  - Reword non-interactive abort message to include consequence
```

---

## Detailed Design

### Skill Restructure

**product-definition**
```yaml
---
name: product-definition
description: Use this skill in plan mode to create or update docs/specs/PRODUCT.md — the product north star.
---

# Product Definition
...
```

**architecture-definition**
```yaml
---
name: architecture-definition
description: Use this skill in plan mode to create or update docs/architecture/ARCHITECTURE.md and Architecture Decision Records.
---

# Architecture Definition
...
```

The existing body text is preserved unchanged.

### CLI Warning Reword

Current:
```
warning: local edits detected in .methodology:
- project_root/manifest.json
```

New:
```
warning: local edits detected in .methodology (continuing will overwrite these files with upstream versions):
- project_root/manifest.json
```

Non-interactive abort current:
```
non-interactive mode: stash or remove local edits first.
```

New:
```
non-interactive mode: stash, remove, or back up local edits first. These files will be overwritten if you continue.
```

---

## Test Strategy

**Unit + integration tests:**
- `TestApplySkillProjections` — update to assert correct content from new paths.
- `TestHumanInvokedSkillsCount` — count remains 4.
- `TestRunUpdateDirtyPromptsAndAbortsOnNo` — assert new warning text in stderr.
- `TestRunUpdateDirtyNonInteractiveAborts` — assert new abort message in stderr.

**Manual verification:**
- Run `go build ./...` and `go test ./...`.
- Run `go run ./cmd/spire init` in a temp dir and verify `.opencode/skills/spire-product-definition/SKILL.md` exists with frontmatter.

---

## Rollback Plan

- Skill restructure: revert the two file moves and update Go source to old paths.
- Warning reword: revert the two `fmt.Fprintln` lines.
- No project source code is touched.

---

## CI/CD Impact

- No workflow changes.
- Tests must pass in existing `go test ./...` job.

---

## Gate 4 Handoff Criteria

- Verification agent runs `go test ./...` and confirms all tests pass.
- Verification agent produces `docs/changes/003-skill-consistency/VERIFICATION_REPORT.md`.
- PR is blocked if verification verdict is `NEEDS WORK`.

---

## Open Questions
None.
