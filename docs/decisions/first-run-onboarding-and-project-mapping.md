# First-run onboarding and Linear project mapping

**Status:** accepted; revised 2026-07-31 after the first live `spire init` run
**Decision owner:** product owner
**Last checked:** 2026-07-31

The 2026-07-31 revision changes §1 (init is re-runnable and seeds from the
existing configuration), makes the model validation in §4 concrete, and extends
the §5 provisioning-write contract from projects to workflow states. Everything
else stands as originally accepted.

## Context

The current deployment contract exposes Spire's full runtime schema to the
operator. A first installation requires manually creating directories, installing
systemd units, copying provider identifiers, authoring dispatch rules, and naming
credential references. Repository routing is configured through Linear labels in
YAML.

That is an implementation-oriented interface rather than an acceptable first-run
experience. Codex and Claude Code users also normally authenticate their installed
CLI as their ordinary login user. Spire should follow that convention instead of
requiring a separate harness credential mechanism by default.

Spire still needs durable, unambiguous answers to two questions:

1. Which `(harness, model, effort)` executes implementation and review?
2. Which Git repository belongs to a Linear issue?

The onboarding interface may simplify those decisions, but the runtime must still
persist their resolved identities and fail closed when either is ambiguous.

## Decision

### 1. `spire init` is the primary first-run interface

After installing a released binary, the default onboarding flow is:

```text
spire init
spire doctor
spire start
```

`spire init`:

- uses the invoking login user as the default runtime identity;
- creates user-scoped configuration, data, and evidence locations;
- discovers Linear organizations, teams, workflow states, estimate scales, and
  existing webhooks through authenticated read APIs;
- detects Git, GitHub, Codex, and Claude Code executables and authentication state;
- collects maker and reviewer execution choices;
- writes a valid fail-closed configuration;
- initializes SQLite; and
- leaves production automation disabled.

Init does not install a service. Service installation is a separate,
platform-dependent step: `spire service install` where a supervisor exists, and
a foreground `spire serve` on macOS, per
[`harness-process-execution.md`](harness-process-execution.md).

A hardened machine-wide installation with a dedicated service identity may be
offered as an explicit advanced mode. It is not the default onboarding path.

`spire init` is re-runnable and is the supported way to change any answer it
collected. On a second run it loads the existing configuration, offers every
previous answer as the default, and lets the operator revisit earlier answers
before committing. It is a read-modify-write interview, not a write-once
installer.

Re-runnability is the editing interface promised by §2. Requiring an operator to
hand-edit YAML to correct a single answer is not acceptable, and neither is
refusing to run because a configuration already exists.

Two properties hold on every run, first or subsequent:

- **Nothing is written until the interview completes.** Init performs one atomic
  configuration write at the end. An interrupted run leaves the installation
  exactly as it found it, including on a re-run over an existing configuration.
- **Every collected answer is recorded.** Init emits a structured trace of the
  choices it made and the values it derived, so a later surprise can be traced
  to the answer that caused it. The resulting configuration records outcomes;
  the trace records decisions.

`spire doctor` verifies the effective runtime context, not merely the interactive
shell. It checks configuration, database access, provider authentication,
non-interactive Git fetch/push access, harness availability, and service health
without enabling automation.

### 2. User and system configuration locations are distinct

The default user installation follows XDG locations:

- configuration under `$XDG_CONFIG_HOME/spire`, falling back to
  `$HOME/.config/spire`;
- durable data under `$XDG_DATA_HOME/spire`, falling back to
  `$HOME/.local/share/spire`; and
- runtime/evidence paths derived from the initialized installation.

An explicit system installation uses `/etc/spire` for configuration and
`/var/lib/spire` for durable state.

Configuration resolution order is:

1. an explicit `--config` path;
2. `SPIRE_CONFIG`;
3. the user XDG path; and
4. the system path.

Generated configuration is inspectable and editable through `spire config`
commands. Operators do not need to author the complete runtime schema before the
first start.

### 3. Authentication is managed separately from ordinary configuration

Spire does not place secret values in its ordinary configuration. It exposes
credential lifecycle through an operator-facing authentication command, for
example:

```text
spire auth status
spire auth login linear
spire auth login github
spire auth rotate linear
```

The concrete secret store may vary between user and system installations. The CLI
owns its permissions and migrations; operators do not manually author
`credential_ref` values.

`spire auth login github` registers a GitHub App owned by the operator rather
than asking for a pasted credential. Spire submits a manifest, the operator
confirms one pre-filled page and installs the App, and Spire stores the returned
private key and webhook secret directly. Operators do not fill in the App form,
choose permissions, or handle the key. Every installation registers its own App;
see [`github-app-identity.md`](github-app-identity.md).

Codex and Claude Code authentication remains provider-native. Spire launches each
harness as the configured runtime user and reuses that user's existing provider
authentication. The initial harness configuration therefore has no Spire-managed
credential reference.

### 4. Maker and reviewer selections include model and effort

The user-facing configuration records a complete primary execution selection for
each role:

```yaml
harnesses:
  maker:
    provider: codex
    model: REPLACE_WITH_SUPPORTED_MODEL
    effort: high
  reviewer:
    provider: claude
    model: REPLACE_WITH_SUPPORTED_MODEL
    effort: high
```

The maker and reviewer providers must differ. `spire init` probes or asks for
supported provider, model, and effort values and validates the resulting pair.

Model is never collected as unvalidated free text. Neither Codex nor Claude Code
exposes a machine-readable model list, so Spire ships a **model catalog**: a
data file, editable without rebuilding, listing the known model identifiers for
each provider. Init offers that catalog as a selection.

The catalog is a convenience, not an authority. It goes stale the moment a
provider ships a model, so:

- an operator may supply a model outside the catalog, and Spire records it; and
- once the harness execution path can spawn a provider process, init
  **probes** the selected model and reports failure at setup time rather than
  at first dispatch.

The probe, not the catalog, is the real check. A catalog alone cannot detect its
own staleness, and a purely syntactic guard cannot tell a retired model from a
current one. Until the probe exists, an off-catalog model is accepted with a
recorded warning.

This simpler interface does not weaken the versioned dispatch contract. Spire
compiles the selections into a complete versioned dispatch policy, evaluates it
against normalized Linear complexity, and persists the effective
`(harness, model, effort)` decision for every run. Advanced configuration may
define complexity-specific choices and ordered fallback candidates.

Whether the first-run wizard requires fallback candidates or begins with one
primary candidate per role remains an implementation decision. It must be resolved
with `dispatch-policy-v1.md` before changing validation.

### 5. `spire new` registers a local repository with Linear

The onboarding command for an existing local Git repository is:

```text
spire new [PATH]
```

The command:

1. resolves and validates the canonical local path;
2. registers it as the worktree source and resolves its Git common directory;
3. inspects the Git remote, canonical GitHub repository, and default branch;
4. tests Git access using the runtime user's existing SSH configuration;
5. asks the operator to select a Linear organization and team;
6. offers to create a Linear project or select an existing project;
7. records the Linear-project-to-repository mapping in SQLite; and
8. reports the resulting mapping and automation state.

The registered checkout remains operator-owned. Harnesses run only in
Spire-allocated worktrees; Spire does not edit, switch, reset, clean, or delete the
registered checkout.

Linear exposes project creation through its GraphQL `projectCreate` mutation.
Creating a project is an explicit external write. Spire shows the intended change,
requires confirmation unless the caller supplied an explicit non-interactive
approval flag, and records a durable provisioning operation before delivery.

A retry must query existing projects before creating another. If Spire cannot
unambiguously reconcile a prior attempt, it stops for operator selection rather
than creating a possible duplicate.

The same contract governs **workflow states**. When a team has no state that can
carry a Spire lifecycle state, init offers to create one through
`workflowStateCreate` under the identical rules: show the intended change,
require confirmation, record a durable provisioning operation before delivery,
and query existing states before creating on any retry. Forcing the operator out
to the Linear UI mid-interview, then restarting init to pick the new state up, is
not an acceptable alternative.

Setup writes and runtime writes are distinct authorities and never share a gate:

| | Setup provisioning write | Runtime ticket write |
|---|---|---|
| Examples | create a project, create a workflow state | transition an issue, post a comment |
| Actor | the operator's own credential | the configured bot actor |
| Gate | explicit per-action confirmation during the interview | `rollout.linear_writes_enabled` plus the allowlists |
| Frequency | once, at setup | continuously, per ticket |

Confirming a setup write must never enable runtime automation, and enabling
runtime automation must never authorize schema changes to the workspace. An
implementation that satisfies one gate by consulting the other is wrong.

Because a setup write is performed with the operator's own credential, Linear
attributes it to that person rather than to the bot. That is acceptable for
one-time provisioning and is the reason the two authorities are separated rather
than merged into a single Linear write capability.

The same command supports existing projects, either interactively or through an
explicit option:

```text
spire new . --linear-project existing
```

Exact final command names and flags may be refined for consistency before
implementation, but the create-or-map behavior is binding.

As implemented, the mapping half of this command is spelled
`spire projects map --linear-project-id <ID> --repository-source <PATH>`. There
is no `spire new`; the local-repository registration and project-creation steps
above are not yet built. This paragraph exists because the unimplemented `spire
new` spelling has already been mistaken for a shipped command.

### 6. Project-to-repository mappings live in SQLite

Repository routing is durable application state, not static YAML. A mapping
contains at least:

```text
linear_organization_id
linear_team_id
linear_project_id
linear_project_name_snapshot
github_repository
repository_source_path
git_common_directory
git_remote_url
default_branch
enabled
created_at
updated_at
```

Stable provider IDs are authoritative; names and URLs are diagnostic snapshots.
The initial consistency rules are:

- one Linear project maps to exactly one Git repository;
- more than one Linear project may map to the same repository;
- only enabled mappings are eligible for admission;
- an issue without a mapped project is `repository_unmapped`;
- an ambiguous or stale mapping blocks admission and requires reconciliation; and
- multi-repository work remains unsupported until its workflow is designed
  explicitly.

The enabled mapping set is also Spire's repository allowlist. A Linear issue cannot
direct Spire to an arbitrary repository merely by containing a label, URL, or
prompt instruction.

Mapping changes are managed through Spire commands and retained in the audit
record. Expected operations include list, add/map, disable, remove, and doctor.

### 7. Existing SSH configuration is authoritative when it works

Spire does not require an SSH-key path when the runtime user's normal Git/SSH
configuration already provides non-interactive access.

`spire init`, `spire new`, and `spire doctor` test access from the actual runtime
context. They distinguish keys available on disk from an ephemeral or forwarded
agent that may disappear after logout or reboot. Git transport authentication is
separate from the GitHub API identity used for pull requests, checks, comments,
and webhooks.

## Architectural boundaries

- Provider discovery and interactive prompting belong to CLI and provider
  adapters.
- Repository routing and admission rules belong to the application core.
- SQLite adapters persist mappings and provisioning operations.
- Git/worktree adapters implement the accepted
  [`worktree-first workspace contract`](worktree-first-workspace-ownership.md).
- The domain and application layers do not import CLI, Linear SDK, GitHub SDK,
  filesystem, SSH, or systemd implementations.
- No SQLite transaction remains open across Linear, GitHub, Git, or harness IO.
- External provisioning writes are durable, auditable, and reconciled after
  ambiguous outcomes.

## Consequences

### Positive

- A released binary can reach a safe runnable state without a hand-authored
  deployment file.
- Existing Codex, Claude Code, Git, and SSH authentication works by default.
- Opaque Linear identifiers and repository metadata are discovered rather than
  copied manually.
- Project routing becomes durable, inspectable, and changeable without restarting
  Spire.
- The repository allowlist cannot be expanded by untrusted ticket content.

### Costs and trade-offs

- `spire init`, `spire auth`, `spire new`, and `spire doctor` add CLI and migration
  surface area.
- User-level service behavior must be tested across the supported Linux/systemd
  target, including logout and reboot.
- Provider-native harness authentication is only as isolated as the selected
  runtime user.
- Linear project and workflow-state creation introduce an external-write crash
  window that requires provisioning reconciliation.
- A re-runnable interview must read, present, and round-trip every value it once
  wrote, so each new configuration field costs more than it did under a
  write-once installer.
- The model catalog is a maintenance burden that goes stale on the provider's
  schedule rather than ours, and it remains wrong until the probe lands.
- Existing label-based repository mappings require a forward migration into
  SQLite.

## Rejected alternatives

- **Require operators to author the full YAML:** exposes internal schema and
  provider IDs as onboarding work.
- **Require a dedicated service user by default:** conflicts with the established
  Codex and Claude Code login-user convention.
- **Store Codex and Claude credentials in Spire:** duplicates provider-owned
  authentication and creates unnecessary secret-handling code.
- **Route only through Linear labels:** makes a mutable ticket field both routing
  input and repository authority.
- **Infer arbitrary repositories from ticket text or URLs:** permits untrusted
  content to expand Spire's authority.
- **Keep project mappings only in configuration:** requires restarts and makes
  discovered operational state awkward to reconcile.

## Required follow-up

- Update `external-identities.md` and `security-and-authority.md` to distinguish
  service authentication, provider-native harness authentication, and Git
  transport authentication.
- Refine `dispatch-policy-v1.md` so the simplified maker/reviewer configuration and
  required fallback behavior agree.
- Replace label-based `repository_mappings` configuration with a forward SQLite
  migration and application ports for project mapping.
- Implement the onboarding roadmap in Sprints 12–15:
  [`12-user-runtime-and-configuration.md`](../sprints/12-user-runtime-and-configuration.md),
  [`13-authentication-and-diagnostics.md`](../sprints/13-authentication-and-diagnostics.md),
  [`14-durable-project-routing.md`](../sprints/14-durable-project-routing.md), and
  [`15-guided-onboarding-and-project-provisioning.md`](../sprints/15-guided-onboarding-and-project-provisioning.md).
- Add deterministic Linear project create/list fixtures, including timeout after
  remote creation and ambiguous retry.
- Verify the exact Linear `ProjectCreateInput` schema and required permissions
  against the authenticated target workspace before enabling project creation.

## Evidence sources

- [Linear GraphQL API](https://linear.app/developers/graphql)
- [Linear SDK data modification](https://linear.app/developers/sdk-fetching-and-modifying-data)
- [Linear API changelog recording `projectCreate`](https://linear.app/changelog/page/17)
- [`worktree-first-workspace-ownership.md`](worktree-first-workspace-ownership.md)
- `docs/ai_harness_architecture.md`
- `docs/ai_harness_implementation.md`
- `docs/decisions/dispatch-policy-v1.md`
- `docs/decisions/security-and-authority.md`

## Unknown / unverified

- Exact CLI command names and non-interactive flags.
- Exact user-level systemd installation and lingering contract.
- Provider-native Codex and Claude Code authentication-status probes.
- Whether initial dispatch requires configured fallback candidates.
- The chosen GitHub API identity and token-refresh implementation.
- Target-workspace permissions and required fields for Linear project creation.
