# TASKS.md — Skill Consistency and Update Warning Clarity
Feature: 003-skill-consistency
Date: 2026-05-30

---

## Task 1: Restructure product-definition skill
**Goal:** Move `methodology/skills/product-definition.md` to `methodology/skills/product-definition/SKILL.md` and add YAML frontmatter.
**Files:**
- `methodology/skills/product-definition.md` (delete)
- `methodology/skills/product-definition/SKILL.md` (create)
**Tests:** Update `internal/scaffold/skills_test.go` `TestApplySkillProjections` helper and assertions.
**Verification:** `go test ./internal/scaffold/...` passes.
**Satisfies:** AC-1.

## Task 2: Restructure architecture-definition skill
**Goal:** Move `methodology/skills/architecture-definition.md` to `methodology/skills/architecture-definition/SKILL.md` and add YAML frontmatter.
**Files:**
- `methodology/skills/architecture-definition.md` (delete)
- `methodology/skills/architecture-definition/SKILL.md` (create)
**Tests:** Update `internal/scaffold/skills_test.go` assertions.
**Verification:** `go test ./internal/scaffold/...` passes.
**Satisfies:** AC-2.

## Task 3: Update Go skill mappings
**Goal:** Update `HumanInvokedSkills` source paths to reference the new `SKILL.md` locations.
**Files:** `internal/scaffold/skills.go`
**Tests:** Existing unit tests cover this indirectly; verify `TestHumanInvokedSkillsCount` still passes.
**Verification:** `go test ./internal/scaffold/...` passes.
**Satisfies:** AC-3.

## Task 4: Update test helper for new skill paths
**Goal:** Update `createMethodologySource` to write skills into subfolders.
**Files:** `internal/commands/init_test.go`
**Tests:** `go test ./internal/commands/...` passes.
**Verification:** `go test ./internal/commands/...` passes.
**Satisfies:** AC-4.

## Task 5: Reword update dirty-files warning
**Goal:** Make the warning explain that continuing will overwrite local changes.
**Files:** `internal/commands/update.go`
**Tests:** Update `internal/commands/update_test.go` assertions for `TestRunUpdateDirtyPromptsAndAbortsOnNo` and `TestRunUpdateDirtyNonInteractiveAborts`.
**Verification:** `go test ./internal/commands/...` passes.
**Satisfies:** AC-5, AC-6.

## Task 6: Run full test suite and fix regressions
**Goal:** Ensure no existing tests are broken by path or wording changes.
**Files:** Any files needing regression fixes.
**Tests:** `go test ./...`
**Verification:** `go test ./...` passes with 0 failures.
**Satisfies:** All ACs (regression guard).

## Task 7: Verification handoff
**Goal:** Produce verification report and hand off to Gate 4.
**Files:** `docs/changes/003-skill-consistency/VERIFICATION_REPORT.md`
**Tests:** None (reporting task).
**Verification:** Traceability matrix covers AC1–AC7.
**Satisfies:** Gate 4 exit criteria.
