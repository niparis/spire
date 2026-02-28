# PLAN.md — sdd CLI
Feature: 001-sdd-cli
Status: APPROVED
Date: 2026-02-28

---

## Context

`sdd` is a bash CLI that manages the SDD methodology lifecycle across projects.
It has two responsibilities: distributing the global methodology files (install/update),
and scaffolding local project artefacts (init, new, status).

It is intentionally not an OpenCode wrapper. It manages files. OpenCode manages agents.

---

## Chosen Approach

**Pure bash, single-file, GitHub-hosted.**

Two alternatives were considered:

- **Node.js CLI (e.g. via npm)** — richer UX (colours, prompts, arg parsing),
  but requires Node on the machine. Not guaranteed, adds friction for a tool
  that should work everywhere a developer works.
  Rejected: dependency risk outweighs UX gain.

- **Python script** — also widely available, better string handling than bash.
  But Python 2/3 ambiguity, venv issues, and the operations here (git clone,
  file copy, string interpolation) are trivially expressible in bash.
  Rejected: not meaningfully better for this use case.

- **Pure bash** ← chosen. Zero runtime dependencies. Every operation is a git
  command or a file copy. Installs to PATH in one curl command.
  Maintained as two files: `install.sh` (one-time) and `sdd` (the CLI itself).

---

## Repository Structure (methodology repo)

```
opencode-sdd/                        ← the methodology repo (GitHub)
│
├── install.sh                       ← curl | bash target
├── sdd                              ← the CLI script itself
│
├── methodology/                     ← everything that gets distributed
│   ├── skills/
│   │   ├── spec-auditor.md
│   │   ├── verification.md
│   │   ├── testing.md
│   │   ├── backend.md
│   │   ├── frontend.md
│   │   ├── security.md
│   │   ├── product-definition.md
│   │   └── architecture-definition.md
│   │
│   ├── agents/
│   │   ├── architecture-agent.md
│   │   ├── plan-agent.md
│   │   └── code-agent.md
│   │
│   └── templates/
│       ├── AGENTS.md                ← base template
│       ├── opencode.json            ← starter config
│       ├── spec-template.md
│       └── session-template.md
│
├── CHANGELOG.md
└── README.md
```

The `sdd` script and `install.sh` live at repo root.
The `methodology/` folder is what gets cloned into `.methodology/` in each project.

---

## What Each Command Does

### `install.sh` (run once, globally)
- Detects OS (macOS / Linux)
- Determines install path: `~/.local/bin` on Linux, `/usr/local/bin` on macOS
  (falls back to `~/bin` if neither is writable without sudo)
- Downloads `sdd` script from GitHub raw URL
- Makes it executable
- Checks PATH and warns if install dir is not in PATH
- Prints version and confirms install

### `sdd init` (run once per project)
- Checks not already initialised (`.methodology/` exists → abort with message)
- Clones `methodology/` folder from GitHub into `.methodology/`
  (uses sparse checkout to clone only the `methodology/` subfolder, not the full repo)
- Adds `.methodology/` to `.gitignore`
- Scaffolds local files FROM templates if they don't already exist:
  - `agents/AGENTS.md` (from `templates/AGENTS.md`, with project name placeholder)
  - `opencode.json` (from `templates/opencode.json`)
  - `specs/_template.md` (copied directly)
  - `specs/product.md` (empty stub with header)
  - `architecture/` (empty dir with `.gitkeep`)
  - `changes/` (empty dir with `.gitkeep`)
- Does NOT overwrite files that already exist
- Prints next step instructions

### `sdd update`
- Checks `.methodology/` exists (abort if not — run init first)
- Detects if any files inside `.methodology/` have been manually edited
  (git status inside the cloned dir) → warns and asks confirmation before overwriting
- Pulls latest from GitHub into `.methodology/`
- Diffs what changed: prints list of modified skill/agent/template files
- Does NOT touch any local project files
- Suggests reviewing CHANGELOG.md for notes on changes

### `sdd new`
- Reads existing specs to determine next feature number (scans `specs/feature-*.md`)
- Prompts: "Feature name (kebab-case):"
- Creates `specs/feature-<N>-<name>.md` from `specs/_template.md`
  with number and name pre-filled in header
- Creates `changes/<N>-<name>/` directory
- Creates `changes/<N>-<name>/SESSION.md` from `templates/session-template.md`
  with feature name and date pre-filled
- Prints: "Spec created. Fill it in, then run the Plan Agent."

### `sdd status`
- Scans `specs/feature-*.md` for all features
- For each, infers status by checking what files exist:
  - SPEC ONLY → "Awaiting audit"
  - + AUDIT.md → "Awaiting planning"
  - + PLAN.md → "Awaiting implementation"
  - + SESSION.md (in changes/) → reads SESSION.md for "Status:" line → shows it
  - + VERIFICATION_REPORT.md → "Awaiting PR"
  - (no changes/ dir, spec archived) → "Complete"
- Prints a simple table:
  ```
  #   Feature               Status
  001 user-authentication   In progress (task 3/7)
  002 payment-flow          Awaiting planning
  003 notifications         Spec only
  ```

---

## File-by-File Change List

```
opencode-sdd/
  install.sh              CREATE  — global installer
  sdd                     CREATE  — CLI script (~300 lines bash)
  methodology/            CREATE  — all methodology files (from previous work)
  CHANGELOG.md            CREATE  — empty, version 0.1.0
  README.md               CREATE  — install instruction + command reference
```

All methodology content already exists (written in prior sessions).
This plan is about the delivery mechanism only.

---

## Test Strategy

**Unit (bash):** Use `bats` (Bash Automated Testing System) — the standard
bash testing framework. Tests live in `tests/`.

Each command gets its own test file:
- `tests/init.bats` — tests init in a temp directory
- `tests/update.bats` — tests update detects changes, pulls correctly
- `tests/new.bats` — tests numbering, file creation, template substitution
- `tests/status.bats` — tests status inference from file presence

Key test cases per command:
- `init`: happy path, already-initialised abort, .gitignore written,
  existing files not overwritten
- `update`: no .methodology abort, dirty detection, changelog shown
- `new`: correct numbering (gap handling, first feature), template substitution,
  duplicate name handling
- `status`: all five status states, empty project (no features yet)

**Integration:** A single `tests/e2e.bats` that runs the full flow in a
temp git repo: install → init → new → (simulate plan/implement files) → status.

**Manual smoke test checklist** (run before any release tag):
- Fresh macOS (no existing .methodology)
- Fresh Ubuntu
- Project with existing AGENTS.md (confirm not overwritten on init)
- Dirty .methodology/ (confirm update warns)

---

## Rollback Plan

`sdd` only reads and writes files. It never modifies project source code.
Rollback for any command is `git checkout .` or deleting the files it created.
The `.methodology/` directory is in `.gitignore` and not tracked — removing it
has no git consequences.

---

## CI/CD

GitHub Actions on the `opencode-sdd` repo:

```yaml
on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install bats
        run: sudo apt-get install -y bats
      - name: Run tests
        run: bats tests/

  test-macos:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install bats
        run: brew install bats-core
      - name: Run tests
        run: bats tests/

  release:
    if: startsWith(github.ref, 'refs/tags/')
    needs: [test, test-macos]
    steps:
      - name: Update install.sh raw URL to point to tag
        # install.sh downloads `sdd` from a pinned tag URL
        # on release, this is updated automatically
```

The `install.sh` always installs the latest tagged release, not main.
`sdd update` for the methodology pulls from main (methodology files
change more frequently and don't need semver pinning).

---

## Open Questions

None. All resolved in planning session.
