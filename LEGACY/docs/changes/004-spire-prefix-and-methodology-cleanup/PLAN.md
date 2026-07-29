# Plan: Spire Prefix for Skill Names and Methodology Cleanup

Feature: 004-spire-prefix-and-methodology-cleanup  
Audit Verdict: PASS (49/50)

---

## 1. Chosen Approach + Rationale

### Approach

1. **Skill frontmatter prefix** — Edit the YAML frontmatter `name:` field in the four human-invoked skill sources under `methodology/skills/` to include the `spire-` prefix. `ApplySkillProjections` already copies these files verbatim to `.opencode/skills/spire-<name>/SKILL.md`; the only missing piece is making the frontmatter `name:` match the destination directory for OpenCode discovery.

2. **Stale methodology cleanup** — Enhance `SyncAndReportChanges` (`internal/methodology/update.go`) so that after `copyDir` copies the source tarball forward, it:
   - computes a `sourceHashes` map (what the upstream tarball contains),
   - deletes files present in the previous sync (`beforeHashes`) but absent from `sourceHashes`,
   - explicitly skips `.spire-sync-state.json` and `.spire-source.json` regardless of hash state,
   - walks the tree bottom-up and removes any empty directories (excluding the `.methodology/` root),
   - returns removed paths separately from changed paths so `commands/update.go` can print them with the mandated prefix.

### Rationale

- `SyncAndReportChanges` is the natural place because it already owns the atomic "compute before/after, copy, write sync state" sequence. Adding deletion inside that sequence keeps the operation atomic and reuses the existing `dirFileHashes` helper.
- `copyDir` is intentionally left untouched; it is a generic forward-only copier used elsewhere. Giving it deletion semantics would be surprising.
- Returning `removed` as a separate slice (rather than printing deep inside the sync layer) keeps the CLI responsible for user-facing output formatting, consistent with how `CleanupOpencode` works.
- An explicit guard for the two state files (in addition to their exclusion from `dirFileHashes`) provides defense-in-depth.

### Rejected Alternatives

- **Delegate cleanup to `commands/update.go`** — Would require exposing `beforeHashes` / `sourceHashes` internals or a separate walk, splitting the atomic sync into two phases and increasing the risk of partial failure.
- **Make `copyDir` destructive** — `copyDir` is a low-level utility in `fetch.go`; turning it into a destructive sync would break its contract and any future callers that expect simple copying.

---

## 2. File-by-File Change List

| File | Change |
|------|--------|
| `methodology/skills/product-definition/SKILL.md` | Frontmatter `name:` → `spire-product-definition` |
| `methodology/skills/new-feature/SKILL.md` | Frontmatter `name:` → `spire-new-feature` |
| `methodology/skills/grill-me/SKILL.md` | Frontmatter `name:` → `spire-grill-me` |
| `methodology/skills/architecture-definition/SKILL.md` | Frontmatter `name:` → `spire-architecture-definition` |
| `internal/methodology/update.go` | Add `sourceHashes` computation; stale-file deletion loop; `removeEmptyDirs` helper; update `SyncAndReportChanges` signature to return `(changed, removed, error)` |
| `internal/methodology/fetch.go` | Thread new `removed` return value through `SyncAndReportChangesFromMetadata` |
| `internal/commands/update.go` | Accept `removed` slice from `SyncAndReportChangesFromMetadata`; print each as `removed stale methodology: <path>` |
| `internal/commands/update_test.go` | Add tests for stale methodology removal, empty-directory cleanup, state-file preservation, and idempotency |

---

## 3. Test Strategy

### Unit / targeted tests
- **`internal/methodology/update_test.go`** (new file):
  - `TestRemoveEmptyDirs` — create a nested tree under a temp dir, call `removeEmptyDirs`, assert only truly empty directories are removed and non-empty ones remain.

### Integration tests
- **`internal/commands/update_test.go`**:
  - `TestRunUpdateRemovesStaleMethodologyFile` — seed a file in `.methodology/` that does not exist in the upstream source; assert the file is deleted and stdout contains `removed stale methodology:`.
  - `TestRunUpdateRemovesEmptyMethodologyDirs` — seed a stale file in a nested subdir, remove it, assert the now-empty subdir is also removed.
  - `TestRunUpdatePreservesSyncState` — assert `.spire-sync-state.json` and `.spire-source.json` survive an update even if the source tarball is missing them.
  - `TestRunUpdateIdempotent` (extend existing) — run update twice; assert second run reports zero changed files and zero removals.

### No new external dependencies
All changes use the Go standard library (`os`, `path/filepath`, `sort`, `strings`, `fmt`). AC-9 is verified by `go mod tidy` / inspection.

---

## 4. Rollback Plan

- All changes are additive or corrective; no schema or API migrations are involved.
- If a bug is discovered post-merge, revert the commit. The `.spire-sync-state.json` will still be valid because the hash map only tracks file contents — reverting to an older binary simply repopulates the directory from the source tarball on the next `spire update`.
- The skill frontmatter changes are content-only; reverting restores the old `name:` values with no side effects.

---

## 5. Ordered Task List

### Task 1 — Prefix skill frontmatter names
- **Goal:** Update the `name:` field in all four human-invoked skill sources so OpenCode discovery matches the destination directory name.
- **Files to touch:**
  - `methodology/skills/product-definition/SKILL.md`
  - `methodology/skills/new-feature/SKILL.md`
  - `methodology/skills/grill-me/SKILL.md`
  - `methodology/skills/architecture-definition/SKILL.md`
- **Tests to add:** None (content change; existing `TestRunUpdateCreatesSkills` continues to assert body text, which is unchanged).
- **Verification:** `grep '^name: spire-' methodology/skills/*/SKILL.md` yields 4 matches.
- **ACs satisfied:** AC-1, AC-2, AC-3, AC-4

### Task 2 — Implement stale methodology file/directory removal and reporting
- **Goal:** After `copyDir`, compute source hashes, delete files present in the previous sync but absent from source, skip state files, remove empty directories, and report removals.
- **Files to touch:**
  - `internal/methodology/update.go` — add `sourceHashes` computation, stale-deletion loop, `removeEmptyDirs` helper, update `SyncAndReportChanges` signature
  - `internal/methodology/fetch.go` — thread new `removed` return value through `SyncAndReportChangesFromMetadata`
  - `internal/commands/update.go` — print `removed stale methodology: <path>` for each removed file
- **Tests to add:**
  - `internal/methodology/update_test.go` — `TestRemoveEmptyDirs`
  - `internal/commands/update_test.go` — `TestRunUpdateRemovesStaleMethodologyFile`, `TestRunUpdateRemovesEmptyMethodologyDirs`
- **Verification:** `go test ./internal/methodology/... ./internal/commands/...` passes; new tests confirm files and empty dirs are removed and reported correctly.
- **ACs satisfied:** AC-5, AC-6, AC-7, AC-8

### Task 3 — Verify idempotency and state preservation
- **Goal:** Ensure a second consecutive `spire update` on a clean state reports no changes and no removals, and that sync-state files are never deleted.
- **Files to touch:** `internal/commands/update_test.go`
- **Tests to add:**
  - Extend `TestRunUpdateIdempotent` to assert the second run reports no changed files and no `removed stale methodology:` lines.
  - `TestRunUpdatePreservesSyncState` — assert `.spire-sync-state.json` and `.spire-source.json` remain after update.
- **Verification:** `go test ./internal/commands/...` passes.
- **ACs satisfied:** AC-5, AC-10

### Task 4 — Final verification
- **Goal:** Confirm the entire change compiles cleanly, uses only stdlib, and all tests pass.
- **Files to touch:** None (read-only verification).
- **Tests to add:** None.
- **Verification:** `go vet ./... && go test ./...` exits 0 with no failures. Inspect `go.mod` to confirm no new external dependencies.
- **ACs satisfied:** AC-9, AC-10

---

## 6. AC → Task Traceability

| AC | Task(s) |
|----|---------|
| AC-1 (frontmatter `spire-product-definition`) | Task 1 |
| AC-2 (frontmatter `spire-new-feature`) | Task 1 |
| AC-3 (frontmatter `spire-grill-me`) | Task 1 |
| AC-4 (frontmatter `spire-architecture-definition`) | Task 1 |
| AC-5 (remove stale files) | Task 2 |
| AC-6 (remove empty dirs) | Task 2 |
| AC-7 (never delete state files) | Task 2, Task 3 |
| AC-8 (report with prefix) | Task 2 |
| AC-9 (stdlib only) | Task 4 |
| AC-10 (vet + test pass) | Task 3, Task 4 |
