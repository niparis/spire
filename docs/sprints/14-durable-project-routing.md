# Sprint 14 — Durable Project Routing and Worktree Sources

**Last Verified:** 2026-07-30
**Depends on:** Sprint 13 exit criteria
**Unlocks:** Sprint 15

## Outcome

Linear-project-to-Git-repository mappings become durable SQLite state and the sole
repository-routing authority for new work. Each mapping registers the repository
source and Git common directory from which Spire allocates owned worktrees.
The Linear adapter includes stable project identity in its canonical issue
projection, eligibility resolves enabled mappings through an application port,
and mapping changes are auditable and do not reroute active work.

This sprint maps existing Linear projects only. It does not create a Linear project
or enable production automation. It also implements the accepted worktree-first
decision as a forward change from the directory-only allocator delivered earlier.

## Entry criteria

- Linear and GitHub authentication diagnostics pass.
- SQLite backup/restore is proven for the current schema.
- The target Linear fixture confirms how project identity appears on an issue.
- Existing schema 3 label mappings and active work have been inventoried without
  reading or migrating `LEGACY/`.

## Target command surface

```text
spire projects list
spire projects map
spire projects show
spire projects disable
spire projects remove
spire projects doctor
```

`remove` is a tombstone operation. Audit rows and mappings referenced by work
items are never physically deleted.

## Work packages

### S14.1 Define project mapping domain and application contracts

Add validated values and DTOs for:

- `LinearProjectId`;
- `ProjectRepositoryMappingId`;
- `ProjectRepositoryMapping`;
- `ProjectMappingRevision`;
- `ProjectMappingStatus`;
- `ProjectRoutingDecision`.

Define an application-owned `ProjectMappingPort` with operations to resolve,
list, create, revise, disable, and tombstone mappings.

Routing outcomes:

```text
mapped
repository_unmapped
mapping_disabled
mapping_stale
mapping_ambiguous
repository_unhealthy
```

Rules:

1. Stable Linear and GitHub identities are authoritative.
2. Display names, paths, and URLs are diagnostic snapshots.
3. One Linear project resolves to at most one enabled repository.
4. Multiple Linear projects may resolve to the same repository.
5. A ticket cannot supply an arbitrary repository through text, URL, or label.

Verification:

- Pure routing tests cover every outcome.
- Domain/application crates import no SQLx, Linear SDK, Git CLI, or filesystem
  packages.
- Mapping revisions are explicit and serializable.

### S14.2 Add forward-only SQLite mapping migrations

Add `0007_project_repository_mappings.sql` after
`0006_operations_cleanup.sql`. Create:

```text
project_repository_mappings
project_repository_mapping_history
```

The active table contains at least:

```text
id
linear_organization_id
linear_team_id
linear_project_id
linear_project_name_snapshot
github_repository
repository_source_path
git_common_directory
git_remote_url
default_branch
status
revision
created_at
updated_at
```

Constraints:

1. Unique `(linear_organization_id, linear_project_id)`.
2. Valid status check for `enabled`, `disabled`, and `removed`.
3. Positive monotonic revision.
4. Nonempty bounded identity values.
5. No cascade deletion of mapping history or referenced audit state.
6. History records the actor, operation, previous/new revision, reason, and
   timestamp.

Verification:

- Migration succeeds from an empty database and every existing migration state.
- Duplicate projects and invalid status/revision fail at the database boundary.
- Backup/restore preserves mappings and history exactly.
- Applying the migration never fabricates a project mapping from a label.

### S14.3 Implement the SQLite mapping adapter

Implementation:

1. Validate rows through domain constructors.
2. Use optimistic revision checks for update/disable/remove.
3. Commit the active-row change and history row atomically.
4. Resolve only enabled mappings.
5. Treat duplicate/corrupt rows as integrity failures.
6. Keep reads available during short mapping writes.
7. Expose mapping count and stale/disabled aggregates in operations diagnostics.

Verification:

- Repository contract tests use a temporary real SQLite database.
- Concurrent updates produce one winner.
- A crash cannot commit an active change without history.
- A removed mapping remains auditable and cannot silently reactivate.

### S14.4 Extend the canonical Linear projection with project identity

Implementation:

1. Query the stable project ID and diagnostic project name required for routing.
2. Preserve missing project as explicit `None`.
3. Include project identity in canonical revision/hash calculation.
4. Update deterministic Linear issue and webhook fixtures.
5. Do not treat project name as stable identity.
6. Bound additional GraphQL response size and pagination behavior.

Verification:

- Fixtures cover no project, mapped project, renamed project, archived project, and
  unknown/malformed project data.
- A project rename updates the snapshot without changing the mapping identity.
- SDK objects remain inside the adapter.

### S14.5 Replace label routing in eligibility

Implementation:

1. Remove repository selection from Linear labels for new observations.
2. Resolve canonical project ID through `ProjectMappingPort`.
3. Use the enabled mapping's repository as the only admission repository.
4. Persist mapping ID and revision with every newly claimed work item/dispatch
   decision.
5. Keep an active run bound to its snapshotted repository when a mapping changes.
6. Raise an operator action if an active issue moves to another project or its
   mapping is disabled.
7. Permit unclaimed observations to adopt the newest enabled mapping.

Verification:

- Ticket labels and description cannot change repository authority.
- Disabling a mapping stops new claims but not recovery/cleanup for active work.
- Changing a mapping during a run never moves its workspace or branch.
- Duplicate, missed, and reordered Linear events converge to one routing decision.

### S14.6 Define the schema 3 transition

Existing `linear.repository_mappings` labels do not contain stable Linear project
IDs and cannot be migrated automatically.

Implementation:

1. Add a preflight report listing every configured label mapping, matching
   unclaimed observation, and active work item.
2. Require explicit project selection before creating each SQLite mapping.
3. Preserve active work's snapshotted repository until terminal.
4. Mark new/unclaimed work `repository_unmapped` until an enabled mapping exists.
5. Remove the label-mapping field from the new effective schema only after the
   database transition is complete.
6. Retain a redacted migration evidence record; never retain ticket text.

Verification:

- No migration guesses from equal names or labels.
- Active work remains recoverable during the transition.
- Re-running preflight and mapping operations is idempotent.
- Rollback uses database backup plus the prior binary/config; no migration file is
  edited or reversed in place.

### S14.7 Implement project mapping commands

Implementation:

1. List and select existing Linear projects through a read-only adapter.
2. Resolve local Git path, remote, canonical GitHub repository, and default branch.
3. Resolve and persist its Git common directory as the worktree source.
4. `map` presents the complete proposed mapping before commit.
5. `disable` stops new admission immediately.
6. `remove` writes a tombstone and requires a reason.
7. `doctor` verifies Linear project existence, GitHub repository identity,
   repository source, Git common directory, remote, default branch, and Git
   worktree capability.
8. Support stable JSON output for automation.

Verification:

- Commands are deterministic with fake Linear/Git/GitHub adapters.
- Ambiguous remotes, duplicate projects, stale revisions, moved paths, and renamed
  default branches fail with remediation.
- Mapping commands make no provider mutation.
- CLI interruption before commit leaves no mapping/history row.

### S14.8 Reconcile mappings periodically

Implementation:

1. Add a bounded read-only reconciliation job for enabled mappings.
2. Detect archived/missing Linear projects, renamed projects, inaccessible GitHub
   repositories, changed default branches, and invalid local paths.
3. Update diagnostic snapshots only through revisioned history.
4. Never auto-remap to a different repository.
5. Block new admission on stale authority while recovery and cleanup continue.

Verification:

- Missed project webhook or API outage converges after reconciliation.
- Transient provider failure does not tombstone a mapping.
- Renames are distinguished from identity changes.

### S14.9 Migrate workspace ownership and allocate maker worktrees

Add `0008_worktree_ownership.sql` after
`0007_project_repository_mappings.sql`. Extend workspace persistence with at
least:

```text
kind
root_run_id
review_cycle_id
repository_source_path
git_common_directory
base_sha
head_sha
branch
allocation_state
```

`branch` is nullable for detached review workspaces. Existing rows retain their
historical identity and are not silently asserted to be Git-recognized
worktrees.

Implementation:

1. Replace the directory-only workspace allocator with a Git-aware adapter behind
   the application-owned workspace port.
2. Persist allocation intent in `allocating` state before Git or filesystem IO;
   never hold a SQLite transaction across either.
3. Derive the maker branch as
   `spire/<sanitized-linear-identifier>-<root-run-short-id>` from bounded,
   validated value objects.
4. Allocate one maker branch and worktree per root attempt from the mapped
   repository source and recorded base SHA.
5. Record the source path, Git common directory, base SHA, branch, path,
   ownership marker, and lifecycle state.
6. Reuse the root attempt's maker workspace for same-harness continuations and
   CI/review corrections.
7. Never switch, reset, clean, or edit the registered source checkout.
8. On recovery, adopt only when SQLite, the exact ownership marker, filesystem
   path, and `git worktree list --porcelain` agree; otherwise quarantine.

Verification:

- Migration succeeds from every supported existing schema and does not rewrite
  applied migrations.
- Concurrent allocation cannot reuse a branch or path.
- Linear titles, descriptions, labels, and project names cannot enter a branch or
  path.
- Allocation leaves dirty, detached, and linked registered source checkouts
  unchanged.
- Child runs reuse the root maker workspace; a new root attempt receives a new
  branch/worktree.
- Crash tests cover intent before Git, Git success before marker, marker before
  ready state, and exact-match recovery.
- Directory-only, foreign, symlink-escaped, and metadata-mismatched paths are
  rejected or quarantined.

### S14.10 Allocate reviewer worktrees and migrate cleanup

Implementation:

1. Allocate each review cycle a separate worktree detached at the exact current
   CI-green head SHA.
2. Do not expose or reuse the maker worktree and do not create a reviewer branch.
3. Bind the workspace record and ownership marker to the review cycle and reviewed
   SHA.
4. Detect tracked or untracked reviewer changes after execution and fail the
   review contract.
5. Before cleanup, prove terminal retention, no live Run/lease/unit/review cycle,
   an in-root path, an exact marker, and matching SQLite/Git metadata.
6. Remove an owned worktree with `git worktree remove` and perform only bounded
   metadata pruning.
7. Keep worktree removal separate from branch deletion; never delete a branch
   needed by an open PR.
8. Quarantine ambiguous or partially removed state for operator inspection.

Verification:

- Review allocation is detached at the requested SHA and cannot observe maker
  uncommitted state.
- A new head SHA receives a new review workspace; a stale SHA cannot be approved.
- Reviewer mutation, push attempts, and workspace reuse fail closed.
- Active, foreign, unowned, mismatched, source-checkout, Git-common-directory, and
  out-of-root paths are never removed.
- Cleanup and recovery converge after crashes before removal, after Git removal,
  and before terminal state persistence.

## Suggested pull-request slices

1. Domain/application contracts and SQLite migration/adapter.
2. Canonical Linear project projection and routing evaluation.
3. Schema 3 transition and mapping CLI.
4. Reconciliation, operations diagnostics, and failure coverage.
5. Workspace schema migration and Git-aware maker allocation.
6. Detached reviewer allocation, recovery, and cleanup migration.

## Sprint demo

Select an existing Linear project and local Git repository, preview and commit a
mapping, reconcile its canonical metadata, and show a matching Ready issue become
repository-eligible in dry-run mode. Then change the ticket label, disable the
mapping, and change the project during a simulated active run. Demonstrate that
labels never reroute work, new admission stops, and active work remains bound to
its persisted repository.

Using the mapped repository, allocate a maker worktree without changing the
registered checkout, reuse it for a child correction, allocate a detached reviewer
worktree at an exact fixture SHA, simulate each allocation crash window, and
remove only the owned terminal worktrees through Git-aware cleanup.

## Exit criteria

- SQLite is the only repository-routing source of truth for new work.
- Every mapping mutation is revisioned and auditable.
- Canonical Linear issues carry stable project identity.
- Active work cannot be rerouted by mapping or ticket changes.
- Schema 3 label mappings have an explicit non-guessing transition path.
- Mapping commands and reconciliation make no external mutation.
- Backup/restore and deterministic failure tests cover the new schema.
- Maker workspaces are Git-recognized, owned by the root attempt, and recoverable.
- Review workspaces are separate, detached, exact-SHA, and mutation-detecting.
- Registered source checkouts remain unchanged and outside cleanup authority.
- Worktree cleanup is Git-aware, fail-closed, and independent of branch deletion.

## Implementation evidence

- Forward-only migrations `0007_project_repository_mappings.sql` and
  `0008_worktree_ownership.sql` define revisioned routing authority and owned
  Git-worktree identity.
- `ProjectMappingPort`, `LinearProjectReadPort`, and `WorkspacePort` keep provider,
  SQLite, Git, and filesystem behavior outside the domain/application core.
- `spire projects list --linear` discovers selectable existing Linear projects;
  mapping commands remain read-only at providers and revision every SQLite change.
- Periodic bounded reconciliation heals transient stale health, refreshes diagnostic
  snapshots through history, and never remaps repository identity.
- Temporary real-Git fixtures cover maker reuse, detached reviewer isolation,
  reviewer mutation, allocation crash windows, live-lease cleanup refusal, cleanup
  restart convergence, source-checkout preservation, and branch retention.
- Canonical verification is `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, and `scripts/check-architecture.sh`.

## Evidence Sources

- [`../decisions/first-run-onboarding-and-project-mapping.md`](../decisions/first-run-onboarding-and-project-mapping.md)
- [`../decisions/worktree-first-workspace-ownership.md`](../decisions/worktree-first-workspace-ownership.md)
- [`../ai_harness_implementation.md`](../ai_harness_implementation.md)
- Sprint 03 Linear fixtures and Sprint 02 SQLite operating contract.

## Unknown / Unverified

- Whether archived Linear projects remain queryable with the selected credentials.
- Exact GitHub repository rename behavior under the approved API identity.
- Target-VM behavior for a moved or unmounted selected repository source.
- Exact Git version and linked-worktree behavior on the target VM.
- Branch-name maximum length after provider and Git hosting constraints.
