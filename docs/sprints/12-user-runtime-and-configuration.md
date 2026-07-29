# Sprint 12 — User Runtime and Configuration Foundation

**Last Verified:** 2026-07-29
**Depends on:** Sprint 11 exit criteria
**Unlocks:** Sprint 13

## Outcome

A released Spire binary can establish and operate a user-scoped installation
without a dedicated service account or explicit `--config` on every command.
Configuration paths follow XDG conventions, maker and reviewer selections include
provider/model/effort, and service lifecycle commands target the invoking user's
systemd instance. Automation remains disabled.

This sprint establishes local installation mechanics only. It does not authenticate
providers, create Linear projects, register repositories, or migrate repository
routing.

## Entry criteria

- Sprint 11 publishes a checksum-verified Linux x86_64 release binary.
- [`../decisions/first-run-onboarding-and-project-mapping.md`](../decisions/first-run-onboarding-and-project-mapping.md)
  is accepted.
- The supported Linux distribution, systemd version, XDG behavior, and binary
  install location have been verified on the target VM.
- The maker/reviewer fallback requirement in
  [`../decisions/dispatch-policy-v1.md`](../decisions/dispatch-policy-v1.md) is
  resolved without weakening different-harness review.

## Target command surface

```text
spire paths
spire config path
spire config show
spire config validate
spire config migrate
spire service install
spire start
spire stop
spire status
```

Final spelling may change before implementation, but every operation above must
have an explicit, scriptable equivalent.

## Work packages

### S12.1 Reconcile configuration and identity contracts

Update before changing code:

- `docs/decisions/external-identities.md`;
- `docs/decisions/security-and-authority.md`;
- `docs/decisions/dispatch-policy-v1.md`;
- configuration sections in `docs/ai_harness_implementation.md`;
- Sprint 01, Sprint 05, Sprint 09, and Sprint 11 assumptions affected by the
  login-user default.

Binding decisions to record:

1. User-scoped execution is the default.
2. A dedicated system identity is an explicit advanced profile.
3. Provider-native Codex and Claude Code authentication replaces harness
   `credential_ref`.
4. The simplified maker/reviewer selection compiles into a versioned dispatch
   policy.
5. Existing active runs keep their persisted dispatch decision across configuration
   migration.

Verification:

- No active decision document simultaneously requires user-native harness auth and
  per-harness systemd credential injection.
- The dispatch ADR states whether fallback candidates are mandatory at first run.
- The architecture and implementation documents agree on the default runtime user.

### S12.2 Implement installation profile and path resolution

Add an application-owned `InstallationProfile` and provider-neutral path-resolution
DTO. Filesystem and environment access remain in the binary/adapters layer.

User profile defaults:

```text
config:  $XDG_CONFIG_HOME/spire/config.yaml  or $HOME/.config/spire/config.yaml
data:    $XDG_DATA_HOME/spire               or $HOME/.local/share/spire
state:   $XDG_STATE_HOME/spire              or $HOME/.local/state/spire
cache:   $XDG_CACHE_HOME/spire              or $HOME/.cache/spire
```

When an XDG variable is absent, use its specification fallback below the invoking
user's home. Do not reuse `HOME`, `CODEX_HOME`, or provider-specific variables as
Spire overrides.

Configuration resolution precedence:

1. `--config`;
2. `SPIRE_CONFIG`;
3. user XDG configuration;
4. `/etc/spire/spire.yaml`.

Implementation:

1. Return resolved paths from one shared resolver used by every subcommand.
2. Add `spire paths --format text|json`.
3. Canonicalize existing parents without requiring not-yet-created leaf paths.
4. Reject files, symlink escapes, conflicting roots, and paths owned by another
   user.
5. Create directories with user-only permissions and atomic error recovery.
6. Keep system-profile paths compatible with the existing `/etc/spire` and
   `/var/lib/spire` layout.

Verification:

- Table-driven tests cover every precedence combination.
- Missing XDG variables produce deterministic fallbacks.
- A relative override, malicious symlink, and foreign-owned directory fail closed.
- Commands resolve the same paths before and after logout/reboot.

### S12.3 Introduce the user-facing harness-role configuration

Replace the primary user interface of a capability registry plus hand-authored
dispatch matrix with role-oriented selections:

```yaml
harnesses:
  maker:
    provider: codex
    model: MODEL_ID
    effort: high
  reviewer:
    provider: claude
    model: MODEL_ID
    effort: high
```

Implementation:

1. Add typed maker/reviewer configuration values.
2. Validate provider, model, and effort identifiers using domain value objects.
3. Reject the same provider for maker and reviewer.
4. Compile the simplified selection into exact `(role, complexity)` coverage.
5. Preserve a generated policy version and deterministic rule IDs.
6. Permit an advanced representation for complexity-specific selections and
   approved fallback candidates.
7. Keep Linear incapable of supplying provider, model, or effort.

Verification:

- A minimal pair covers all supported complexity classes exactly once.
- Same-provider maker/reviewer fails with an actionable path.
- Reordering YAML does not change generated rule IDs or policy output.
- Every run still persists the complete dispatch evaluation and selected triplet.

### S12.4 Version and migrate configuration safely

Implementation:

1. Introduce configuration schema version 4; do not reinterpret schema 3.
2. Add `spire config migrate --from PATH [--write]`.
3. Default migration to a redacted preview with no filesystem mutation.
4. On `--write`, create an adjacent timestamped backup and use atomic replacement.
5. Convert harness capability/dispatch data only when it has one unambiguous
   maker/reviewer result for every complexity.
6. Report credential-reference and repository-mapping fields as deferred to
   Sprints 13 and 14 rather than silently discarding them.
7. Keep rollout disabled in every generated or migrated configuration.

Verification:

- Schema 3 fixtures migrate deterministically or stop with exact unresolved fields.
- Interrupted replacement leaves either the original or complete new file.
- Secret values never enter preview, backup names, logs, or generated config.
- Re-running migration is idempotent.

### S12.5 Add configuration management commands

Implementation:

1. Add `spire config path`.
2. Add `spire config show --effective --redacted`.
3. Make `spire config validate` use implicit path resolution when `--config` is
   absent.
4. Add narrow setters for supported operator choices; do not create a generic YAML
   mutation language.
5. Serialize edits through atomic file replacement and retain comments only when
   the chosen serializer can prove preservation.
6. Refuse changes that would invalidate active persisted dispatch plans.

Verification:

- Text and JSON output contain no secrets.
- Concurrent edits produce one winner and one actionable conflict.
- Unknown keys continue to fail validation.
- Every existing command works with both explicit and implicit config paths.

### S12.6 Package a user-level systemd service

Create a user service template separate from the hardened system service.

Implementation:

1. Run as the invoking user; do not set `User=` or `Group=`.
2. Install the unit below the resolved
   `$XDG_CONFIG_HOME/systemd/user/spire.service` path.
3. Resolve the released binary and config through stable installed paths.
4. Preserve graceful `SIGINT`, restart policy, and stop timeout.
5. Restrict write access to resolved Spire roots without hiding the user's
   provider-native auth or SSH configuration.
6. Add `spire service install` with a preview and explicit confirmation.
7. Add `spire start`, `spire stop`, and `spire status` wrappers around
   `systemctl --user`.
8. Detect whether user lingering is required for logout/reboot persistence and
   report the exact operator action; do not silently invoke privileged
   `loginctl`.

Verification:

- Unit rendering is deterministic and contains no secrets.
- A path containing spaces is handled without shell interpolation.
- Install is idempotent and does not overwrite a modified unit without approval.
- Start, stop, restart, logout, and reboot behavior are demonstrated on the target
  VM.

### S12.7 Preserve the system installation profile

Implementation:

1. Keep `--system` an explicit installation profile.
2. Continue to support `/etc/spire`, `/var/lib/spire`, and the existing hardened
   system unit.
3. Require explicit privilege before any machine-wide write.
4. Keep user and system installations distinguishable in status output.
5. Reject an accidental mixed installation unless the caller selects one profile.

Verification:

- User installation requires no root-owned writes.
- System installation never falls back to the invoking user's config.
- Mixed state produces remediation instructions rather than nondeterministic
  selection.

## Suggested pull-request slices

1. Decision updates, installation profile, and path resolver.
2. Harness-role configuration and deterministic dispatch compilation.
3. Schema migration and config management commands.
4. User systemd unit, lifecycle commands, and target-VM evidence.

## Sprint demo

Download the Sprint 11 binary on a clean Linux VM, create a user-scoped
configuration through documented commands, show the generated maker/reviewer
dispatch coverage, install the user service, log out, reboot, and show that
`spire status` resolves the same configuration and durable paths. Automation
remains disabled and no provider API is called.

## Exit criteria

- Every command resolves configuration and state through the shared precedence
  rules.
- A schema 3 configuration has a deterministic, non-destructive migration path.
- Maker and reviewer selections include provider, model, and effort and preserve
  dispatch invariants.
- The user service survives logout and reboot on the target VM.
- The existing system profile remains explicit and functional.
- No provider credential or repository mapping remains necessary to demonstrate
  the sprint.

## Evidence Sources

- [`../decisions/first-run-onboarding-and-project-mapping.md`](../decisions/first-run-onboarding-and-project-mapping.md)
- [`../decisions/dispatch-policy-v1.md`](../decisions/dispatch-policy-v1.md)
- [`11-release-artifacts-versioning-and-installation.md`](11-release-artifacts-versioning-and-installation.md)
- XDG Base Directory and target-VM systemd evidence captured during the sprint.

## Unknown / Unverified

- Exact supported distribution and user-systemd version.
- `spire start` requires a prior explicit `spire service install --yes`; it does
  not create a unit implicitly.
- A minimal primary maker/reviewer pair is accepted at first run. Ordered
  fallbacks remain optional advanced configuration.
- User-systemd start, stop, logout, and reboot persistence still require target
  VM evidence; installation reports the required `loginctl enable-linger` action
  but never invokes it.
