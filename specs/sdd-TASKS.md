# TASKS.md — sdd CLI
Feature: 001-sdd-cli
Depends on: PLAN.md approved

Each task: 5–10 minutes. Ordered by dependency.
Format per task: Goal / Files / Tests to add / Verification step.

---

## Phase 1 — Repository Skeleton

### Task 001 — Create repo structure and README
**Goal:** Establish the repository skeleton so all subsequent tasks have a home.
**Files:**
- `README.md` — install instruction, command reference table, contributing note
- `CHANGELOG.md` — version 0.1.0 header, empty entries
- `methodology/` — empty dirs: `skills/`, `agents/`, `templates/`
- `.github/workflows/ci.yaml` — stub (just the on: trigger for now)
**Tests:** None yet (nothing to test).
**Verification:** `ls -R` confirms structure matches PLAN.md repo layout.

---

### Task 002 — Populate methodology/ with all content files
**Goal:** Copy all previously written methodology content into the repo.
**Files:**
- `methodology/skills/spec-auditor.md`
- `methodology/skills/verification.md`
- `methodology/skills/testing.md`
- `methodology/skills/backend.md`
- `methodology/skills/frontend.md`
- `methodology/skills/security.md`
- `methodology/skills/product-definition.md`
- `methodology/skills/architecture-definition.md`
- `methodology/agents/architecture-agent.md`
- `methodology/agents/plan-agent.md`
- `methodology/agents/code-agent.md`
- `methodology/templates/AGENTS.md`
- `methodology/templates/opencode.json`
- `methodology/templates/spec-template.md`
- `methodology/templates/session-template.md`
**Tests:** None (content files, not logic).
**Verification:** Every file exists and is non-empty. Word count sanity check:
`wc -l methodology/**/*.md` — no file should be 0 lines.

---

## Phase 2 — Core CLI Script

### Task 003 — sdd script skeleton with help and version
**Goal:** Create the `sdd` script with entry point, arg routing, and `--help` / `--version`.
**Files:**
- `sdd` (new, executable)
**Implementation notes:**
```bash
#!/usr/bin/env bash
set -euo pipefail

SDD_VERSION="0.1.0"
SDD_REPO="https://github.com/YOU/opencode-sdd"
SDD_METHODOLOGY_URL="${SDD_REPO}/archive/refs/heads/main.tar.gz"
METHODOLOGY_DIR=".methodology"

main() {
  case "${1:-help}" in
    init)    cmd_init ;;
    update)  cmd_update ;;
    new)     cmd_new ;;
    status)  cmd_status ;;
    --version|-v) echo "sdd ${SDD_VERSION}" ;;
    *)       cmd_help ;;
  esac
}
```
**Tests:** `tests/help.bats` — `sdd --version` outputs version string,
`sdd --help` exits 0 and mentions all four commands.
**Verification:** `bash -n sdd` (syntax check), `./sdd --version` outputs `sdd 0.1.0`.

---

### Task 004 — `sdd init` command
**Goal:** Clone methodology into `.methodology/`, scaffold local files.
**Files:** `sdd` (add `cmd_init` function)
**Implementation notes:**
- Check `.methodology/` already exists → print "Already initialised." and exit 1
- Use `git clone --depth=1 --filter=blob:none --sparse $SDD_REPO .methodology`
  then `git -C .methodology sparse-checkout set methodology/`
  to get only the methodology subfolder (avoids downloading the full repo)
- Add `.methodology/` to `.gitignore` (append if file exists, create if not,
  skip if entry already present)
- For each local file: copy from template only if destination does not exist
- Print each action taken (created / skipped existing)
- Final message: list of next steps
**Tests:** `tests/init.bats`
- Happy path: all files created in temp git repo
- Already initialised: exits 1, prints message, no changes
- Existing AGENTS.md not overwritten: create it first, run init, confirm unchanged
- .gitignore entry written correctly
- .gitignore not duplicated if run twice (test guards against re-running init)
**Verification:** Run in a fresh temp dir. Confirm all expected files present.
Confirm `.methodology/` in `.gitignore`. Confirm existing file untouched.

---

### Task 005 — `sdd update` command
**Goal:** Pull latest methodology, warn on dirty local edits, report changes.
**Files:** `sdd` (add `cmd_update` function)
**Implementation notes:**
- Check `.methodology/` exists → if not, "Run sdd init first." exit 1
- Check for local modifications: `git -C .methodology status --porcelain`
  → if non-empty, print warning listing modified files, prompt "Continue? [y/N]"
  → if N (or non-interactive), exit 1 with message "Stash or remove local edits first."
- Pull: `git -C .methodology pull origin main`
- Report changed files: `git -C .methodology diff --name-only HEAD@{1} HEAD`
  → print each changed file (strip `methodology/` prefix for readability)
- Print: "Review CHANGELOG.md for notes on what changed."
**Tests:** `tests/update.bats`
- No .methodology: exits 1 with helpful message
- Clean .methodology: pulls and reports changed files
- Dirty .methodology: prompts, aborts on N, continues on Y
- Non-interactive (piped input): aborts safely
**Verification:** Run in temp repo with a pre-cloned `.methodology/`.
Manually dirty a file, confirm warning. Confirm changed files listed after pull.

---

### Task 006 — `sdd new` command
**Goal:** Scaffold numbered feature spec and SESSION.md.
**Files:** `sdd` (add `cmd_new` function)
**Implementation notes:**
```bash
cmd_new() {
  # 1. Find next feature number
  local next_num
  next_num=$(find specs/ -name "feature-*.md" 2>/dev/null \
    | grep -oP 'feature-\K[0-9]+' \
    | sort -n | tail -1)
  next_num=$(printf "%03d" $(( ${next_num:-0} + 1 )))

  # 2. Prompt for name
  read -rp "Feature name (kebab-case): " name
  name=$(echo "$name" | tr ' ' '-' | tr '[:upper:]' '[:lower:]')
  [[ -z "$name" ]] && echo "Name required." && exit 1

  local slug="${next_num}-${name}"

  # 3. Create spec from template
  local spec="specs/feature-${slug}.md"
  [[ -f "$spec" ]] && echo "Spec already exists: $spec" && exit 1
  mkdir -p specs
  sed "s/\[Feature Name\]/${name}/g; s/\[NUMBER\]/${next_num}/g; \
       s/YYYY-MM-DD/$(date +%F)/g" \
    .methodology/methodology/templates/spec-template.md > "$spec"

  # 4. Create changes dir and SESSION.md
  local changes_dir="changes/${slug}"
  mkdir -p "$changes_dir"
  sed "s/\[Feature Name\]/${name}/g; s/YYYY-MM-DD/$(date +%F)/g" \
    .methodology/methodology/templates/session-template.md \
    > "${changes_dir}/SESSION.md"

  echo "Created: $spec"
  echo "Created: ${changes_dir}/SESSION.md"
  echo ""
  echo "Next: fill in the spec, then run the Plan Agent."
  echo "Prompt: 'Audit specs/feature-${slug}.md using the spec-auditor skill.'"
}
```
**Tests:** `tests/new.bats`
- First feature: gets number 001
- Numbering: existing 001, 002 → new gets 003
- Gap handling: existing 001, 003 → new gets 004 (uses max+1, not gap-fill)
- Name sanitisation: spaces → hyphens, uppercase → lowercase
- Empty name: exits 1
- Duplicate spec file: exits 1 with message
- Template substitution: feature name and date appear correctly in output files
**Verification:** Run in temp project with existing specs. Confirm correct number,
correct filenames, correct template substitution. Confirm changes/ dir created.

---

### Task 007 — `sdd status` command
**Goal:** Print a table of all features and their inferred status.
**Files:** `sdd` (add `cmd_status` function)
**Implementation notes:**

Status inference rules (checked in order):
1. No `changes/<slug>/` dir AND no `specs/feature-<N>-<n>-AUDIT.md` → **Spec only**
2. `specs/feature-<N>-<n>-AUDIT.md` exists but no `changes/<slug>/PLAN.md` → **Awaiting planning**
3. `changes/<slug>/PLAN.md` exists but `changes/<slug>/SESSION.md` has no completed tasks → **Awaiting implementation**
4. `changes/<slug>/SESSION.md` exists and has content → parse "Status:" line → **In progress (X)**
5. `changes/<slug>/VERIFICATION_REPORT.md` exists → **Awaiting PR**
6. `archive/<slug>/` exists → **Complete**

```bash
cmd_status() {
  local specs
  specs=$(find specs/ -name "feature-*.md" ! -name "*-AUDIT.md" \
          ! -name "_template.md" 2>/dev/null | sort)
  
  if [[ -z "$specs" ]]; then
    echo "No features yet. Run: sdd new"
    return
  fi

  printf "%-5s %-35s %s\n" "#" "Feature" "Status"
  printf "%-5s %-35s %s\n" "---" "-----------------------------------" "------"

  while IFS= read -r spec; do
    local slug
    slug=$(basename "$spec" .md | sed 's/feature-//')
    local num="${slug%%-*}"
    local name="${slug#*-}"
    local status
    status=$(infer_status "$slug")
    printf "%-5s %-35s %s\n" "$num" "$name" "$status"
  done <<< "$specs"
}
```

**Tests:** `tests/status.bats`
- Empty project (no specs): prints helpful message
- Each of the 6 status states (set up file fixtures for each)
- SESSION.md status line parsing: correct extraction
- Feature in archive/: shows Complete
**Verification:** Set up a temp project with specs in various states.
Confirm table alignment and correct status for each.

---

## Phase 3 — Installer

### Task 008 — install.sh
**Goal:** One-command global install of `sdd` to PATH.
**Files:** `install.sh` (new)
**Implementation notes:**
```bash
#!/usr/bin/env bash
set -euo pipefail

SDD_VERSION="0.1.0"
RAW_URL="https://raw.githubusercontent.com/YOU/opencode-sdd/${SDD_VERSION}/sdd"

# Determine install dir
if [[ "$OSTYPE" == "darwin"* ]]; then
  INSTALL_DIR="/usr/local/bin"
else
  INSTALL_DIR="${HOME}/.local/bin"
fi

# Fall back to ~/bin if preferred dir not writable
if [[ ! -w "$INSTALL_DIR" ]]; then
  INSTALL_DIR="${HOME}/bin"
  mkdir -p "$INSTALL_DIR"
fi

echo "Installing sdd ${SDD_VERSION} to ${INSTALL_DIR}..."
curl -fsSL "$RAW_URL" -o "${INSTALL_DIR}/sdd"
chmod +x "${INSTALL_DIR}/sdd"

# PATH check
if ! echo "$PATH" | grep -q "$INSTALL_DIR"; then
  echo ""
  echo "⚠ ${INSTALL_DIR} is not in your PATH."
  echo "  Add this to your shell rc file:"
  echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
fi

echo "✓ sdd ${SDD_VERSION} installed. Run: sdd --help"
```
**Tests:** `tests/install.bats` — mock curl, test path detection logic,
test PATH warning when dir not in PATH.
**Verification:** Run on macOS and Linux (CI matrix). Confirm `sdd --version` works after install.

---

## Phase 4 — CI and Release

### Task 009 — Complete CI workflow
**Goal:** Green CI on both Ubuntu and macOS for all bats tests.
**Files:** `.github/workflows/ci.yaml` (fill in from stub in Task 001)
**Tests:** This task IS the test — CI must pass.
**Verification:** Push to GitHub. Both matrix jobs pass.

### Task 010 — Release tag and raw URL validation
**Goal:** Tag v0.1.0 and confirm the install URL works end-to-end.
**Files:** `install.sh` (update `SDD_VERSION` constant), `CHANGELOG.md` (fill in 0.1.0 entry)
**Verification:** 
- `curl -fsSL <raw-url> | bash` installs successfully on a clean machine
- `sdd --version` outputs `sdd 0.1.0`
- `sdd init` in a temp project creates expected files
- `sdd new` creates a correctly numbered spec

---

## Task Summary

| # | Task | Phase | Estimated time |
|---|------|-------|----------------|
| 001 | Repo skeleton and README | Skeleton | 5 min |
| 002 | Populate methodology/ files | Skeleton | 10 min |
| 003 | sdd script skeleton + help | CLI | 5 min |
| 004 | sdd init | CLI | 10 min |
| 005 | sdd update | CLI | 10 min |
| 006 | sdd new | CLI | 10 min |
| 007 | sdd status | CLI | 10 min |
| 008 | install.sh | Installer | 5 min |
| 009 | CI workflow | CI | 5 min |
| 010 | Release tag | Release | 5 min |

Total: ~75 minutes of implementation time.

---

## AC Traceability (for Verification Report)

| AC | Description | Implemented in | Tested by |
|----|-------------|---------------|-----------|
| AC-1 | curl install works | install.sh | tests/install.bats |
| AC-2 | sdd init scaffolds correct files | sdd:cmd_init | tests/init.bats |
| AC-3 | sdd init does not overwrite existing | sdd:cmd_init | tests/init.bats |
| AC-4 | sdd update warns on dirty .methodology | sdd:cmd_update | tests/update.bats |
| AC-5 | sdd new numbers correctly | sdd:cmd_new | tests/new.bats |
| AC-6 | sdd new substitutes template vars | sdd:cmd_new | tests/new.bats |
| AC-7 | sdd status shows correct state | sdd:cmd_status | tests/status.bats |
| AC-8 | Works on macOS and Linux | all | CI matrix |
| AC-9 | No runtime dependencies beyond bash+git | all | manual + CI |
