# PLAN.md — spire CLI
Feature: 001-spire-cli
Status: APPROVED
Date: 2026-02-28

---

## Context

`spire` is a Go CLI that manages the SDD methodology lifecycle across projects.
It has two responsibilities: distributing the global methodology files (install/update),
and scaffolding local project artefacts (init, new, status).

It is intentionally not an OpenCode wrapper. It manages files. OpenCode manages agents.

---

## Chosen Approach

**Pure Go CLI, single static binary, GitHub-hosted releases.**

Alternatives considered:

- **Pure bash** — zero compile step, but weaker maintainability and testability
  for parsing, templating, and cross-platform behavior.
  Rejected: long-term reliability and DX are better in Go.

- **Node.js CLI (e.g. via npm)** — rich UX, but requires Node/npm runtime.
  Rejected: runtime dependency not guaranteed everywhere.

- **Python script** — stronger ergonomics than bash, but interpreter/version/env
  ambiguity on target machines.
  Rejected: still runtime-dependent.

- **Pure Go** (chosen) — single compiled binary, strong stdlib for files/templates,
  native testing, straightforward cross-compilation, no runtime dependency.

---

## Repository Structure (methodology repo)

```
opencode-spire/
│
├── cmd/
│   └── spire/
│       └── main.go                  ← CLI entrypoint
│
├── internal/
│   ├── cli/                         ← command routing + help/version
│   ├── commands/                    ← init/update/new/status handlers
│   ├── methodology/                 ← clone/fetch/update behaviors
│   ├── scaffold/                    ← template rendering + file creation
│   └── status/                      ← status inference + table output
│
├── methodology/                     ← distributed methodology content
│   ├── skills/
│   ├── agents/
│   ├── templates/
│   └── project_root/               ← files projected to repo root
│       ├── local_agents.md
│       └── manifest.json
│
├── scripts/
│   └── install.sh                   ← curl | bash installer for binaries
│
├── tests/
│   └── e2e/                         ← optional integration tests
│
├── go.mod
├── go.sum
├── CHANGELOG.md
└── README.md
```

---

## Command Behavior

### `spire init` (run once per project)
- Fails if already initialised (`.methodology/` exists)
- Fetches methodology content into `.methodology/` from the canonical GitHub source
  with zero user configuration
- Treats `.methodology/` as a recursive payload sync target (no hardcoded file list)
- Adds `.methodology/` to `.gitignore` (no duplicate entry)
- Applies `project_root/manifest.json` mappings from `.methodology/project_root/`
  to project root (for example, `local_agents.md` -> `AGENTS.md`)
- Uses per-file policy from manifest (`if_missing`, `never_overwrite`, etc.)
- Never overwrites existing local files
- Prints next steps

### `spire update`
- Fails if `.methodology/` missing (`Run spire init first.`)
- Detects local edits inside `.methodology/` and warns
- In interactive mode: asks confirmation to continue
- In non-interactive mode: aborts safely when dirty
- Refreshes methodology content from the same canonical GitHub source model as init
- Prints changed methodology files
- Re-evaluates `project_root/manifest.json` mappings after sync
- Does not overwrite user-modified root files; prints notice when upstream template
  changed but policy blocks overwrite

### `spire new`
- Scans `specs/feature-*.md` to compute next numeric id (`max+1`, padded 3 digits)
- Prompts for feature name (kebab-case normalization)
- Creates `specs/feature-<N>-<name>.md` from `specs/_template.md` with substitutions
- Creates `changes/<N>-<name>/SESSION.md` from methodology session template
- Prints next-step guidance

### `spire status`
- Scans all `specs/feature-*.md` (excluding template/audit files)
- Infers state from associated files:
  - Spec only
  - Awaiting planning
  - Awaiting implementation
  - In progress (from `SESSION.md` Status line)
  - Awaiting PR
  - Complete
- Prints aligned table (`#`, `Feature`, `Status`)

### Methodology Source Strategy (revision)
- `SPIRE_METHODOLOGY_SOURCE` is removed as a required runtime input for users.
- `spire init` and `spire update` resolve source automatically from the official
  GitHub repository; users are not asked to provide local source paths.
- Download mechanism: fetch repository tarball for a chosen ref, extract only
  the `methodology/` subtree, and sync it into project `.methodology/`.
- Avoid shelling out to `git` for runtime fetch to reduce host dependencies.
- Persist source metadata in `.methodology/.spire-source.json`:
  - repository
  - ref/tag
  - fetched_at timestamp
  - optional integrity hash
- `spire update` uses persisted metadata to keep update behavior deterministic.

### Agent Mode and Verification Gate (revision)
- Plan mode is allowed to edit/write Markdown planning artifacts only.
  - Default deny for edits and writes.
  - Allowlist: `**/*.md` (and optionally `**/*.mdx`).
  - This enables plan-mode output to `changes/[feature]/PLAN.md` and
    `changes/[feature]/TASKS.md` without enabling code changes.
- Build mode continues to own implementation changes.
- Verification is elevated to a dedicated Gate 4 role:
  - Add a verification agent profile and verification skill guidance.
  - Required output: `changes/[feature]/VERIFICATION_REPORT.md` with
    traceability matrix, commands run, coverage summary, self-review, and verdict.
  - PR opening is blocked when verdict is `NEEDS WORK`.
- Independence policy:
  - Preferred: run Gate 4 verification in a separate OpenCode session from build.
  - Minimum rule: verifier must not be the same active implementation run.

---

## Delivery / Installation

- Build release binaries for:
  - `darwin/arm64` (Apple Silicon)
  - `darwin/amd64`
  - `linux/amd64`
  - `linux/arm64`
- Publish binaries as GitHub Release assets
- `scripts/install.sh` detects OS/arch, downloads correct binary, installs to:
  - macOS: `/usr/local/bin` (fallback `~/bin`)
  - Linux: `~/.local/bin` (fallback `~/bin`)
- Installer checks PATH and prints guidance if missing

---

## File-by-File Change List

```
opencode-spire/
  cmd/spire/main.go               CREATE
  internal/cli/*.go               CREATE
  internal/commands/*.go          CREATE
  internal/methodology/*.go       CREATE
  internal/scaffold/*.go          CREATE
  internal/status/*.go            CREATE
  methodology/project_root/manifest.json  CREATE
  methodology/agents/VERIFICATION.md      CREATE
  methodology/skills/verification.md      UPDATE (gate-4 schema and verdict rules)
  methodology/project_root/opencode.json  UPDATE (project-root opencode projection)
  scripts/install.sh              CREATE
  methodology/                    CREATE (existing content copied)
  CHANGELOG.md                    CREATE
  README.md                       CREATE
  .github/workflows/ci.yaml       CREATE
  .github/workflows/release.yaml  CREATE (single publish job after matrix build artifacts)
```

---

## Test Strategy

**Unit + command tests (Go):**
- `go test ./...`
- Table-driven tests for:
  - command routing/help/version
  - numbering and slug normalization
  - status inference
  - gitignore idempotency
  - template substitution
  - manifest parsing and mapping policy behavior
  - verification report generator schema validation

**Integration tests:**
- Temp-dir workflow tests:
  - init happy path + no-overwrite behavior
  - init applies project_root mappings (`local_agents.md` -> `AGENTS.md`)
  - update dirty warning + prompt behavior
  - update reports project_root template changes without overwriting protected files
  - new creates expected files
  - status outputs expected states/table
  - verification gate emits required `VERIFICATION_REPORT.md` sections

**Manual smoke test checklist** (run before any release tag):
- Fresh macOS Apple Silicon machine
- Fresh Ubuntu machine
- Project with existing AGENTS.md (confirm not overwritten on init)
- Dirty `.methodology/` (confirm update warns)

---

## Rollback Plan

`spire` only creates/updates project metadata files and `.methodology/`.
Rollback is deleting generated files or reverting via git.
No project source code changes are performed by design.

---

## CI/CD

GitHub Actions on the `opencode-spire` repo:

- `test` job (ubuntu-latest): `go test ./...`
- `test-macos` job (macos-latest): `go test ./...`
- `build-release` job (tags only): cross-compile and upload release assets

Installer installs latest tagged release binary, not `main`.

---

## Open Questions

None.
