# CLI command surface

**Status:** accepted; hiding shipped in PR #39, follow-up work outstanding
**Decision owner:** product owner
**Last checked:** 2026-07-31

## Context

The `spire` binary exposes 45 leaf commands across 13 top-level subtrees. The
public documentation (README, Sprints 12–14) describes roughly 12 of them as the
intended user surface. The remainder accumulated organically as debug helpers,
dry-run harness entry points, one-shot fetch commands, low-level operator
utilities, and internal plumbing that other components shell out to.

Sprint 12 already acknowledged this: *"final spelling may change before
implementation, but every operation above must have an explicit, scriptable
equivalent."* In practice every operation got its own top-level command and the
"final spelling" step never happened. `spire --help` therefore advertised the
implementation surface, not the product surface.

## Decision

### 1. Ten commands are hidden from `--help` but remain fully invocable

Shipped in PR #39 via `#[command(hide = true)]`:

- Top-level hidden: `dispatch`, `db`, `ops`, `linear`, `github`, `scheduler`,
  `runs`.
- Hidden inside `projects`: `doctor`, `preflight`, `reconcile`.

Hidden commands still respond to direct invocation and to
`spire <cmd> --help`. Tests, scripts, and internal callers continue to work
unchanged.

### 2. The visible surface is the approved product surface

`spire --help` now lists exactly these eleven commands:

`init`, `paths`, `service`, `start`, `stop`, `status`, `config`, `auth`,
`doctor`, `projects` (with `list` / `map` / `show` / `disable` / `remove`),
`serve`.

`init` was added after PR #39 as the first-run entry point named by
`docs/decisions/first-run-onboarding-and-project-mapping.md`.

New user-facing capabilities go through this surface. Anything else must
either become part of it or stay hidden.

### 3. The hidden commands are on notice — they are unapproved and must be
removed, merged, or promoted

Hiding is a holding action, not the end state. Each hidden command must be
resolved into one of the following outcomes before Spire 2.0 ships:

- **Delete** — the command exists only because a test or a past debug session
  needed it; the same behavior belongs behind a test harness or a library
  entry point, not on the CLI.
- **Merge** — the behavior is one flag away from an existing command; collapse
  it (e.g. `db backup` + `db backup-daily`, `linear get` + `linear explain`,
  the three `scheduler` verbs).
- **Promote** — the behavior is genuinely user-facing and was miscategorised
  during hiding; write the docs, add it to the visible surface, and remove
  the `hide` attribute.
- **Move under `spire debug`** — the behavior is a legitimate internal
  operator/dev tool that should remain reachable but never top-level. Group
  the survivors under a single `debug` subtree so the "these are internals"
  contract is explicit rather than implicit.

Known candidates for each outcome (non-exhaustive, decided per command):

| Candidate | Likely outcome | Note |
|---|---|---|
| `linear get` vs `linear explain` | merge | one absorbs the other; the delta is the eligibility evaluation |
| `db backup` vs `db backup-daily` | merge | one command, `--schedule` flag |
| `scheduler once` / `explain` / `capacity-show` | merge or move | three verbs where one command with flags would do |
| `runs start-manual` | delete or move | pure dev fixture entry point |
| `dispatch dry-run` | move | policy introspection, useful but not user-facing |
| `github reconcile` | move | operator reconciliation, needs a real name |
| `ops status` | promote or merge into `doctor` | overlaps with `doctor` and `status` |
| `projects doctor` / `preflight` / `reconcile` | move | operator lifecycle utilities |
| `db check` / `restore-check` / `restore-latest` | move | backup drill machinery |

### 4. No new hidden commands

New commands must land on the visible surface with documentation, or they
must land under `spire debug` (once that subtree exists). Adding
`#[command(hide = true)]` to a fresh top-level command is not an acceptable
shortcut.

## Consequences

- `spire --help` reflects the product, not the implementation. The doc/CLI
  gap that motivated this decision is closed for now.
- Hidden commands remain a technical debt line item. Each release cycle
  should retire at least one via delete / merge / promote / move.
- Callers that rely on hidden commands (scripts, tests, other tooling) get
  advance warning: anything hidden today may be renamed, moved under
  `spire debug`, or removed without a deprecation window, because it was
  never a documented interface.
- Sprint 05, 09, and 14 target-command-surface sections still reference some
  now-hidden commands. A doc reconciliation pass is a follow-up.

## References

- PR #39 — hides the ten commands.
- `docs/decisions/first-run-onboarding-and-project-mapping.md` — makes `init`
  the primary first-run interface.
- `docs/sprints/12-user-runtime-and-configuration.md` — original target
  command surface for user runtime.
- `docs/sprints/13-authentication-and-diagnostics.md` — auth and doctor
  surface.
- `docs/sprints/14-durable-project-routing.md` — projects surface.
