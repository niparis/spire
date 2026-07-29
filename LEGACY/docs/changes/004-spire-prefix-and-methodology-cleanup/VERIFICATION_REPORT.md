# Verification Report: 004-spire-prefix-and-methodology-cleanup

Feature: 004-spire-prefix-and-methodology-cleanup  
Date: 2026-05-30  
Agent: verifier (kimi-k2.6)  

---

## 1. Traceability Matrix

| AC | Implemented in file:line | Tested by test:name | Verdict |
|----|--------------------------|---------------------|---------|
| AC-1 (frontmatter `spire-product-definition`) | `methodology/skills/product-definition/SKILL.md:2` | `TestRunUpdateSkillFrontmatterHasSpirePrefix` | PASS |
| AC-2 (frontmatter `spire-new-feature`) | `methodology/skills/new-feature/SKILL.md:2` | `TestRunUpdateSkillFrontmatterHasSpirePrefix` | PASS |
| AC-3 (frontmatter `spire-grill-me`) | `methodology/skills/grill-me/SKILL.md:2` | `TestRunUpdateSkillFrontmatterHasSpirePrefix` | PASS |
| AC-4 (frontmatter `spire-architecture-definition`) | `methodology/skills/architecture-definition/SKILL.md:2` | `TestRunUpdateSkillFrontmatterHasSpirePrefix` | PASS |
| AC-5 (remove stale files) | `internal/methodology/update.go:68-79` | `TestRunUpdateRemovesStaleMethodologyFile` | PASS |
| AC-6 (remove empty dirs) | `internal/methodology/update.go:81-83`, `update.go:248-276` | `TestRunUpdateRemovesEmptyMethodologyDirs` | PASS |
| AC-7 (never delete state files) | `internal/methodology/update.go:70-71` | `TestRunUpdatePreservesSyncState` | PASS |
| AC-8 (report with prefix) | `internal/commands/update.go:86-91` | `TestRunUpdateRemovesStaleMethodologyFile` | PASS |
| AC-9 (stdlib only) | `go.mod` (no external deps) | `go mod` inspection | PASS |
| AC-10 (`go vet ./...` and `go test ./...` pass) | Entire change set | `go vet ./...` + `go test ./... -v` | PASS |

---

## 2. Commands Run with Evidence

### 2.1 `go vet ./...`
```
(no output — exits 0)
```

### 2.2 `go test ./... -v`
```
?   	opencode-spire/cmd/spire	[no test files]
?   	opencode-spire/internal/cli	[no test files]
=== RUN   TestRunInitHappyPathCreatesMethodologyAndRootProjection
--- PASS: TestRunInitHappyPathCreatesMethodologyAndRootProjection (0.02s)
=== RUN   TestRunInitAlreadyInitializedAborts
--- PASS: TestRunInitAlreadyInitializedAborts (0.00s)
=== RUN   TestRunInitDoesNotOverwriteExistingProjectedFiles
--- PASS: TestRunInitDoesNotOverwriteExistingProjectedFiles (0.02s)
=== RUN   TestRunInitGitignoreEntryAddedOnlyOnce
--- PASS: TestRunInitGitignoreEntryAddedOnlyOnce (0.02s)
=== RUN   TestRunInitFailsWhenSourceDownloadFails
--- PASS: TestRunInitFailsWhenSourceDownloadFails (0.00s)
=== RUN   TestRunUpdateWithoutMethodologyAborts
--- PASS: TestRunUpdateWithoutMethodologyAborts (0.00s)
=== RUN   TestRunUpdateCleanReportsChangedFiles
--- PASS: TestRunUpdateCleanReportsChangedFiles (0.03s)
=== RUN   TestRunUpdateDirtyPromptsAndAbortsOnNo
--- PASS: TestRunUpdateDirtyPromptsAndAbortsOnNo (0.02s)
=== RUN   TestRunUpdateDirtyContinuesOnYes
--- PASS: TestRunUpdateDirtyContinuesOnYes (0.03s)
=== RUN   TestRunUpdateDirtyNonInteractiveAborts
--- PASS: TestRunUpdateDirtyNonInteractiveAborts (0.03s)
=== RUN   TestRunUpdateUnknownFlagAborts
--- PASS: TestRunUpdateUnknownFlagAborts (0.00s)
=== RUN   TestRunUpdateRootMappingNoticeWithoutOverwrite
--- PASS: TestRunUpdateRootMappingNoticeWithoutOverwrite (0.05s)
=== RUN   TestRunUpdateForceOverwritesProtectedRootMapping
--- PASS: TestRunUpdateForceOverwritesProtectedRootMapping (0.05s)
=== RUN   TestRunUpdateOpencodeMappingOverwritesWhenSourceChanged
--- PASS: TestRunUpdateOpencodeMappingOverwritesWhenSourceChanged (0.03s)
=== RUN   TestRunUpdateUsesStoredMetadataOverCurrentDefaults
--- PASS: TestRunUpdateUsesStoredMetadataOverCurrentDefaults (0.03s)
=== RUN   TestRunUpdateFallsBackWhenMetadataMissing
--- PASS: TestRunUpdateFallsBackWhenMetadataMissing (0.03s)
=== RUN   TestRunUpdateCreatesSkills
--- PASS: TestRunUpdateCreatesSkills (0.03s)
=== RUN   TestRunUpdateRemovesStaleAgent
--- PASS: TestRunUpdateRemovesStaleAgent (0.03s)
=== RUN   TestRunUpdatePreservesCustomFiles
--- PASS: TestRunUpdatePreservesCustomFiles (0.03s)
=== RUN   TestRunUpdateIdempotent
--- PASS: TestRunUpdateIdempotent (0.04s)
=== RUN   TestRunUpdateRemovesStaleMethodologyFile
--- PASS: TestRunUpdateRemovesStaleMethodologyFile (0.03s)
=== RUN   TestRunUpdateRemovesEmptyMethodologyDirs
--- PASS: TestRunUpdateRemovesEmptyMethodologyDirs (0.03s)
=== RUN   TestRunUpdatePreservesSyncState
--- PASS: TestRunUpdatePreservesSyncState (0.03s)
=== RUN   TestRunUpdateSkillFrontmatterHasSpirePrefix
--- PASS: TestRunUpdateSkillFrontmatterHasSpirePrefix (0.03s)
=== RUN   TestRunUpgradeRejectsUnexpectedArgs
--- PASS: TestRunUpgradeRejectsUnexpectedArgs (0.00s)
=== RUN   TestRunUpgradeNoopWhenLatestIsNotNewer
--- PASS: TestRunUpgradeNoopWhenLatestIsNotNewer (0.00s)
=== RUN   TestRunUpgradeWhenNewerReleaseAvailable
--- PASS: TestRunUpgradeWhenNewerReleaseAvailable (0.00s)
=== RUN   TestRunUpgradeFailsWhenAssetMissing
--- PASS: TestRunUpgradeFailsWhenAssetMissing (0.00s)
=== RUN   TestRunUpgradeFromDevBuild
--- PASS: TestRunUpgradeFromDevBuild (0.00s)
=== RUN   TestRunUpgradeFetchFailure
--- PASS: TestRunUpgradeFetchFailure (0.00s)
PASS
ok  	opencode-spire/internal/commands	1.222s
?   	opencode-spire/internal/methodology	[no test files]
=== RUN   TestLoadProjectRootManifest_Valid
--- PASS: TestLoadProjectRootManifest_Valid (0.00s)
=== RUN   TestLoadProjectRootManifest_InvalidSchemaReturnsTypedError
--- PASS: TestLoadProjectRootManifest_InvalidSchemaReturnsTypedError (0.00s)
=== RUN   TestLoadProjectRootManifest_PathTraversalRejected
--- PASS: TestLoadProjectRootManifest_PathTraversalRejected (0.00s)
=== RUN   TestLoadProjectRootManifest_UnknownPolicyRejected
--- PASS: TestLoadProjectRootManifest_UnknownPolicyRejected (0.00s)
=== RUN   TestCleanupOpencodeRemovesStaleTrackedFiles
--- PASS: TestCleanupOpencodeRemovesStaleTrackedFiles (0.00s)
=== RUN   TestCleanupOpencodePreservesUntrackedFiles
--- PASS: TestCleanupOpencodePreservesUntrackedFiles (0.00s)
=== RUN   TestCleanupOpencodeRemovesStaleSkillDirectories
--- PASS: TestCleanupOpencodeRemovesStaleSkillDirectories (0.00s)
=== RUN   TestGetExpectedAgentProjections
--- PASS: TestGetExpectedAgentProjections (0.00s)
=== RUN   TestHumanInvokedSkillsCount
--- PASS: TestHumanInvokedSkillsCount (0.00s)
=== RUN   TestHumanInvokedSkillsHaveSpirePrefix
--- PASS: TestHumanInvokedSkillsHaveSpirePrefix (0.00s)
=== RUN   TestGetExpectedSkillProjections
--- PASS: TestGetExpectedSkillProjections (0.00s)
=== RUN   TestApplySkillProjections
--- PASS: TestApplySkillProjections (0.01s)
=== RUN   TestApplySkillProjectionsSkipsMissingSkills
--- PASS: TestApplySkillProjectionsSkipsMissingSkills (0.00s)
=== RUN   TestBuildExpectedProjections
--- PASS: TestBuildExpectedProjections (0.00s)
PASS
ok  	opencode-spire/internal/scaffold	(cached)
```

### 2.3 `go test -cover ./internal/...`
```
ok  	opencode-spire/internal/cli		(cached)	coverage: 0.0% of statements
ok  	opencode-spire/internal/commands	1.959s	coverage: 54.8% of statements
ok  	opencode-spire/internal/methodology	(cached)	coverage: 0.0% of statements
ok  	opencode-spire/internal/scaffold	0.997s	coverage: 36.6% of statements
```

---

## 3. Coverage Summary

| Package | Coverage | Notes |
|---------|----------|-------|
| `internal/commands` | 54.8% | All update-related tests pass; new AC-specific tests added. |
| `internal/methodology` | 0.0% | No test files exist in this package. The `removeEmptyDirs` helper and `SyncAndReportChanges` logic are exercised only through `internal/commands` integration tests. |
| `internal/scaffold` | 36.6% | Existing coverage; skill-prefix assertions added via `TestHumanInvokedSkillsHaveSpirePrefix`. |

The new feature code in `internal/methodology/update.go` is exercised entirely through the `internal/commands` integration tests (`TestRunUpdateRemovesStaleMethodologyFile`, `TestRunUpdateRemovesEmptyMethodologyDirs`, `TestRunUpdatePreservesSyncState`, `TestRunUpdateSkillFrontmatterHasSpirePrefix`). While functionally covered, the methodology package itself has no direct unit tests.

---

## 4. Self-Review against Spec Intent

### 4.1 What the spec intended
- Human-invoked skills should have a `spire-` prefix in their frontmatter `name:` so OpenCode discovery matches the destination directory name.
- `spire update` should keep `.methodology/` as an exact mirror of the source tarball, removing stale files and empty directories while preserving sync-state files.
- The update should remain idempotent.

### 4.2 What the implementation delivers
- ✅ All four human-invoked skill sources now have the correct `spire-` prefix in frontmatter.
- ✅ Stale files in `.methodology/` are removed after `copyDir`.
- ✅ Empty directories under `.methodology/` are cleaned up bottom-up.
- ✅ `.spire-sync-state.json` and `.spire-source.json` are explicitly guarded from deletion.
- ✅ Removed files are reported with the mandated `removed stale methodology:` prefix.
- ✅ No external dependencies added (`go.mod` is unchanged except for `go` directive).
- ✅ `go vet ./...` and `go test ./...` pass cleanly.

### 4.3 Gaps and deviations
1. **Missing unit-test file for `internal/methodology`** — `PLAN.md` specified `internal/methodology/update_test.go` with `TestRemoveEmptyDirs`. This file was not created. The functionality is covered by integration tests (`TestRunUpdateRemovesEmptyMethodologyDirs`), so ACs are not violated, but the plan deviation is noted.

2. **Stale files appear in both `changed` and `removed`** — In `SyncAndReportChanges` (`internal/methodology/update.go:98-101`), any file removed because it is stale is also appended to the `changed` slice. Consequently, `commands/update.go` may print the same stale file under both `changed files:` and `removed stale methodology:`. This is a UX inconsistency; the `changed` slice should exclude paths already tracked in `removed`. The existing tests do not assert absence of stale files from the changed list, so the issue is not caught by the suite.

3. **Weak idempotency assertion** — `TestRunUpdateIdempotent` asserts no spurious removals on the second run but does not explicitly assert no changed files, contrary to the plan's specification that it should assert "no changed files and no `removed stale methodology:` lines."

4. **No direct test for `.spire-source.json` preservation** — `TestRunUpdatePreservesSyncState` verifies `.spire-sync-state.json` survives an update, but does not explicitly test `.spire-source.json`. Both files are guarded by the same code path (`update.go:70-71`), so the risk is low.

### 4.4 Risk assessment
- **Functional risk: LOW.** All ACs are satisfied by the integration tests. The double-reporting issue is cosmetic and does not break downstream consumers because manifest mappings for removed files no longer exist in the synced source.
- **Regression risk: LOW.** The change is additive/corrective. Existing init and update flows are preserved.
- **Test coverage risk: MEDIUM.** The `internal/methodology` package has 0% direct test coverage. If `removeEmptyDirs` or `SyncAndReportChanges` are refactored in the future, there are no fast unit tests to catch regressions.

---

## 5. Verdict

**READY FOR PR**

All 10 acceptance criteria are functionally satisfied, commands pass, and the feature behaves as specified. The identified gaps (missing unit-test file, stale-file double-reporting in stdout, and weak idempotency assertion) are plan deviations and UX polish items that should be addressed in a follow-up but do not block merge.

---

## 6. Recommendations (non-blocking)

1. Add `internal/methodology/update_test.go` with `TestRemoveEmptyDirs` as originally planned.
2. Strengthen `TestRunUpdateIdempotent` to assert that the second run prints `no methodology file changes detected`.
3. In `SyncAndReportChanges`, exclude paths already present in `removed` from the `changed` slice so stale files are reported only once.
4. Consider adding an explicit assertion for `.spire-source.json` preservation in `TestRunUpdatePreservesSyncState` or a dedicated test.
