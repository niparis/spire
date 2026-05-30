# TASKS.md — Update Sync Cleanup & Skill Projection
Feature: 002-update-sync-cleanup
Date: 2026-05-29

---

## Task 1: Extend sync state with projections tracking
**Goal:** Add `projections` field to sync state for tracking spire-managed `.opencode/` files.
**Files:** `internal/methodology/update.go`
**Tests:** Update `internal/methodology/update_test.go` (or create if needed) — test parsing old sync state without projections, test round-trip write/read with projections.
**Verification:** `go test ./internal/methodology/...` passes.
**Satisfies:** Foundation for AC1, AC2, AC6.

## Task 2: Define human-invoked skill mappings
**Goal:** Create a hardcoded list of 4 human-invoked skills with source → destination mappings.
**Files:** `internal/scaffold/skills.go` (NEW)
**Tests:** `internal/scaffold/skills_test.go` — verify 4 mappings exist, verify correct source/destination pairs, verify `spire-` prefix.
**Verification:** `go test ./internal/scaffold/...` passes.
**Satisfies:** AC3, AC4.

## Task 3: Implement skill projection application
**Goal:** Copy human-invoked skills from `.methodology/skills/` to `.opencode/skills/` with `spire-` prefix.
**Files:** `internal/scaffold/skills.go`
**Tests:** `internal/scaffold/skills_test.go` — temp dir test: create fake `.methodology/skills/`, run ApplySkillProjections, assert all 4 destinations exist with correct content.
**Verification:** `go test ./internal/scaffold/...` passes.
**Satisfies:** AC3.

## Task 4: Implement stale projection cleanup
**Goal:** Remove tracked `.opencode/` files/directories that are no longer in the expected manifest or skill set.
**Files:** `internal/scaffold/project_root.go`
**Tests:** `internal/scaffold/project_root_test.go` — test tracked stale agent removed, test tracked stale skill dir removed, test untracked custom file preserved, test untracked custom skill preserved.
**Verification:** `go test ./internal/scaffold/...` passes.
**Satisfies:** AC1, AC2, AC5, AC6.

## Task 5: Wire cleanup and skill projection into update command
**Goal:** Integrate skill projection and stale cleanup into the `spire update` execution flow.
**Files:** `internal/commands/update.go`
**Tests:** `internal/commands/update_test.go` — integration tests for full update flow.
**Verification:** `go test ./internal/commands/...` passes.
**Satisfies:** AC1, AC2, AC3, AC7, AC8.

## Task 6: Integration tests for update behavior
**Goal:** Verify end-to-end update scenarios: stale removal, skill creation, custom file preservation, idempotence.
**Files:** `internal/commands/update_test.go`
**Tests:**
- `TestUpdate_RemovesStaleAgent`
- `TestUpdate_CreatesSkills`
- `TestUpdate_LeavesCustomFiles`
- `TestUpdate_Idempotent`
**Verification:** `go test ./internal/commands/...` passes.
**Satisfies:** AC1, AC3, AC6, AC7.

## Task 7: Run full test suite and fix regressions
**Goal:** Ensure no existing tests are broken by sync state extension or new behavior.
**Files:** Any files needing regression fixes.
**Tests:** `go test ./...`
**Verification:** `go test ./...` passes with 0 failures.
**Satisfies:** All ACs (regression guard).

## Task 8: Verification handoff
**Goal:** Produce verification report and hand off to Gate 4.
**Files:** `docs/changes/002-update-sync-cleanup/VERIFICATION_REPORT.md`
**Tests:** None (reporting task).
**Verification:** Traceability matrix covers AC1–AC8.
**Satisfies:** Gate 4 exit criteria.
