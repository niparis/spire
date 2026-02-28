# TASKS.md — spire CLI
Feature: 001-spire-cli
Depends on: PLAN.md approved

Each task: 5-20 minutes. Ordered by dependency.
Format per task: Goal / Files / Tests to add / Verification step.

---

## Phase 1 — Repo & Go Bootstrap

### Task 001 — Create repo skeleton and Go module
**Goal:** Establish project layout for a Go CLI implementation.
**Files:**
- `go.mod`
- `cmd/spire/main.go`
- `internal/cli/`
- `internal/commands/`
- `internal/methodology/`
- `internal/scaffold/`
- `internal/status/`
- `README.md`
- `CHANGELOG.md`
- `.github/workflows/ci.yaml` (stub)
**Tests:** None yet.
**Verification:** `go test ./...` runs (even with minimal placeholders).

---

### Task 002 — Populate methodology payload and root projection files
**Goal:** Store full methodology content and define root projection rules.
**Files:**
- `methodology/skills/**`
- `methodology/agents/**`
- `methodology/templates/**`
- `methodology/project_root/local_agents.md`
- `methodology/project_root/manifest.json`
**Tests:** None (content files).
**Verification:**
- All files exist and are non-empty.
- `manifest.json` validates and includes `local_agents.md -> AGENTS.md` mapping.

---

## Phase 2 — Core CLI Entry

### Task 003 — CLI skeleton with help and version
**Goal:** Implement root command routing and `--help` / `--version`.
**Files:**
- `cmd/spire/main.go`
- `internal/cli/root.go`
**Tests:**
- `internal/cli/root_test.go` for help and version output.
**Verification:** `go run ./cmd/spire --version` outputs `spire 0.1.0`.

---

## Phase 3 — Command & Sync Implementations

### Task 004 — Implement project_root manifest parser and mapper
**Goal:** Load `project_root/manifest.json` and convert entries into safe copy operations.
**Files:**
- `internal/scaffold/manifest.go`
- `internal/scaffold/project_root.go`
**Tests:**
- `internal/scaffold/manifest_test.go`:
  - valid manifest parses
  - invalid schema returns typed error
  - path traversal in destination is rejected
  - unknown policy is rejected
**Verification:** `go test ./internal/scaffold -run Manifest` passes.

---

### Task 005 — Implement `spire init`
**Goal:** Initialize `.methodology/` and scaffold local files idempotently.
**Files:**
- `internal/commands/init.go`
- `internal/methodology/fetch.go`
- `internal/scaffold/init_files.go`
**Behavior notes:**
- Sync `.methodology/` recursively from methodology payload.
- Apply project-root mappings from manifest (default policy: create-if-missing).
- Never overwrite existing local files.
**Tests:**
- `internal/commands/init_test.go`:
  - happy path
  - already-initialized abort
  - existing `AGENTS.md` not overwritten
  - `.gitignore` entry added once only
  - manifest mapping creates `AGENTS.md` from `local_agents.md`
**Verification:** Temp repo run confirms recursive payload copy and root projection behavior.

---

### Task 006 — Implement `spire update`
**Goal:** Refresh methodology safely, with dirty-state warning and mapping notices.
**Files:**
- `internal/commands/update.go`
- `internal/methodology/update.go`
**Behavior notes:**
- After methodology sync, evaluate manifest mappings.
- Never overwrite protected root files on update.
- Print notice when source template changed but overwrite policy blocks write.
**Tests:**
- `internal/commands/update_test.go`:
  - abort when `.methodology/` missing
  - clean update reports changed files
  - dirty mode prompts and aborts on default/no
  - dirty mode continues on explicit yes
  - non-interactive dirty mode aborts safely
  - upstream `local_agents.md` change triggers notice, not overwrite
**Verification:** Temp repo with dirty `.methodology/` and edited root `AGENTS.md` confirms safe behavior.

---

### Task 007 — Implement `spire new`
**Goal:** Create numbered feature spec and SESSION.md from templates.
**Files:**
- `internal/commands/new.go`
- `internal/scaffold/new_feature.go`
**Tests:**
- `internal/commands/new_test.go`:
  - first feature is `001`
  - numbering uses `max+1`
  - gaps are not backfilled
  - slug normalization (space/uppercase)
  - empty name aborts
  - duplicate target aborts
  - template substitution for name/date/number
**Verification:** Temp project with seeded specs; confirm generated files and substitutions.

---

### Task 008 — Implement `spire status`
**Goal:** Print a table of all features and their inferred status.
**Files:**
- `internal/commands/status.go`
- `internal/status/infer.go`
- `internal/status/table.go`
**Tests:**
- `internal/status/infer_test.go` for all states
- `internal/commands/status_test.go` for output formatting and empty project message
**Verification:** Fixture project with multiple feature states; confirm table correctness.

---

## Phase 4 — Installer & Distribution

### Task 009 — Installer script for release binaries
**Goal:** One-command global install of `spire` binary to PATH.
**Files:** `scripts/install.sh`
**Tests:** Install script tests (shell) for:
- OS/arch detection mapping
- install path fallback behavior
- PATH warning output
**Verification:** On macOS ARM64, install then run `spire --version`.

---

## Phase 5 — CI, Quality, Release

### Task 010 — CI + release build automation
**Goal:** Green CI on macOS + Ubuntu, and build/publish binaries on tags.
**Files:** `.github/workflows/ci.yaml` and/or `.github/workflows/release.yaml`
**Tests:** CI itself.
**Verification:** Tag build produces release assets for all target OS/arch pairs.

### Task 011 — Add e2e workflow test
**Goal:** Validate full lifecycle: init -> new -> status -> update.
**Files:** `tests/e2e/e2e_test.go` (or shell harness invoking binary)
**Tests:** Included in CI (optionally gated).
**Verification:** Green in CI on Ubuntu and macOS.

### Task 012 — Final docs and changelog
**Goal:** Document install/use/release flow for Go binary CLI.
**Files:**
- `README.md`
- `CHANGELOG.md`
**Tests:** None.
**Verification:** New user can install and run first commands from README only.

### Task 013 — Tag v0.1.0 and validate installer end-to-end
**Goal:** Confirm release + installer works on clean machine.
**Files:**
- `CHANGELOG.md` (final 0.1.0 notes)
- release tag and assets
**Verification:**
- `curl -fsSL <install-url> | bash` works
- `spire --version` outputs `spire 0.1.0`
- `spire init` and `spire new` function in a temp project

---

## Task Summary

| # | Task | Phase | Estimated time |
|---|------|-------|----------------|
| 001 | Repo skeleton + go module | Bootstrap | 10 min |
| 002 | Populate methodology + manifest | Bootstrap | 10 min |
| 003 | CLI root + help/version | Core | 10 min |
| 004 | Manifest parser + mapper | Commands | 20 min |
| 005 | spire init | Commands | 20 min |
| 006 | spire update | Commands | 20 min |
| 007 | spire new | Commands | 15 min |
| 008 | spire status | Commands | 15 min |
| 009 | Installer for binaries | Installer | 10 min |
| 010 | CI + release build | CI/CD | 15 min |
| 011 | e2e flow test | Quality | 15 min |
| 012 | docs + changelog | Release prep | 10 min |
| 013 | release tag validation | Release | 10 min |

Total: ~3.0-3.5 hours of implementation time.

---

## AC Traceability (for Verification Report)

| AC | Description | Implemented in | Tested by |
|----|-------------|---------------|-----------|
| AC-1 | curl install works | scripts/install.sh | install tests + manual |
| AC-2 | spire init syncs methodology payload recursively | internal/methodology/fetch.go | init tests |
| AC-3 | spire init projects root files via manifest without overwrite | internal/scaffold/project_root.go | init + scaffold tests |
| AC-4 | spire update warns on dirty .methodology | internal/commands/update.go | update tests |
| AC-5 | spire update does not overwrite protected root files | internal/commands/update.go | update tests |
| AC-6 | spire new numbers correctly | internal/commands/new.go | new tests |
| AC-7 | spire new substitutes template vars | internal/commands/new.go | new tests |
| AC-8 | spire status shows correct state | internal/status/infer.go | status tests |
| AC-9 | Works on macOS and Linux | build + runtime | CI matrix |
| AC-10 | No runtime dependency beyond binary (+ git for init/update strategy) | command layer | integration + docs |
