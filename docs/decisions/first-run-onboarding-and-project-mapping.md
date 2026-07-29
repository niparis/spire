# First-run onboarding and Linear project mapping

**Status:** accepted; implementation planned in Sprints 12–15
**Decision owner:** product owner
**Last checked:** 2026-07-29

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
- installs or generates the supported user-level service definition;
- discovers Linear organizations, teams, workflow states, estimate scales, and
  existing webhooks through authenticated read APIs;
- detects Git, GitHub, Codex, and Claude Code executables and authentication state;
- collects maker and reviewer execution choices;
- writes a valid fail-closed configuration;
- initializes SQLite; and
- leaves production automation disabled.

A hardened machine-wide installation with a dedicated service identity may be
offered as an explicit advanced mode. It is not the default onboarding path.

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
2. inspects the Git remote, canonical GitHub repository, and default branch;
3. tests Git access using the runtime user's existing SSH configuration;
4. asks the operator to select a Linear organization and team;
5. offers to create a Linear project or select an existing project;
6. records the Linear-project-to-repository mapping in SQLite; and
7. reports the resulting mapping and automation state.

Linear exposes project creation through its GraphQL `projectCreate` mutation.
Creating a project is an explicit external write. Spire shows the intended change,
requires confirmation unless the caller supplied an explicit non-interactive
approval flag, and records a durable provisioning operation before delivery.

A retry must query existing projects before creating another. If Spire cannot
unambiguously reconcile a prior attempt, it stops for operator selection rather
than creating a possible duplicate.

The same command supports existing projects, either interactively or through an
explicit option:

```text
spire new . --linear-project existing
```

Exact final command names and flags may be refined for consistency before
implementation, but the create-or-map behavior is binding.

### 6. Project-to-repository mappings live in SQLite

Repository routing is durable application state, not static YAML. A mapping
contains at least:

```text
linear_organization_id
linear_team_id
linear_project_id
linear_project_name_snapshot
github_repository
local_repository_path
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
- Linear project creation introduces an external-write crash window that requires
  provisioning reconciliation.
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
