# Worktree-first workspace ownership

**Status:** accepted; implemented in Sprint 14
**Decision owner:** product owner
**Last checked:** 2026-07-30

## Context

Spire must isolate concurrent harness changes, preserve maker state across
continuations and corrections, review an exact CI-green SHA from fresh local state,
and clean up only resources it owns.

A branch and a worktree solve different problems:

- a branch is the durable Git/PR lineage for maker changes;
- a worktree is the local filesystem isolation boundary in which a harness runs.

Linear ticket content is untrusted and may change while work is active. Linear
therefore cannot choose branch names, filesystem paths, source repositories, or
workspace lifecycle. Those are Orchestrator resource decisions.

The directory-only `WorkspaceAllocator` that motivated this decision was replaced
in Sprint 14 by the Git-aware adapter described below.

## Decision

### 1. Worktrees are the default execution isolation

Every implementation or review harness runs in a Spire-allocated Git worktree.
Spire does not run autonomous changes in the repository checkout passed to
`spire new`.

The registered local repository is the default worktree source. Spire resolves and
persists:

```text
canonical repository path
Git common directory
canonical GitHub repository
remote URL
default/base branch
```

Spire may fetch and maintain Git refs and worktree metadata through Git-aware
commands, but it does not edit files, switch branches, reset, or clean the
registered source checkout. A future managed-clone source is an adapter option,
not the initial default.

### 2. Spire owns branch and worktree allocation

Linear supplies work identity and the mapped project. SQLite supplies the enabled
project-to-repository mapping. GitHub supplies canonical repository/base-branch
facts. Spire alone chooses and persists the maker branch and worktree.

The initial maker branch convention is:

```text
spire/<sanitized-linear-identifier>-<root-run-short-id>
```

The readable Linear identifier is diagnostic. The validated root Run ID suffix
provides uniqueness. Spire records the exact branch at allocation and never
recomputes it from later ticket text or identifiers.

Branch and path components are value objects with bounded length and a strict
ASCII-safe alphabet. Ticket titles, descriptions, labels, project names, and
repository instructions never enter a branch or path.

### 3. A maker workspace belongs to the root attempt

Spire creates one maker branch and worktree per root implementation attempt. The
workspace belongs to `root_run_id`, not to each child Run.

The same worktree and branch are reused by:

- same-harness context continuations;
- CI correction runs;
- review correction runs; and
- recovery after an Orchestrator restart.

A new root retry creates a new branch/worktree unless an explicit future adoption
workflow selects an existing one. Child runs never allocate a second maker branch
implicitly.

### 4. A reviewer receives a separate detached worktree

After required CI succeeds, Spire creates a fresh reviewer worktree detached at
the exact current PR head SHA:

```text
git worktree add --detach <review-path> <head-sha>
```

The reviewer does not receive the maker worktree and does not own a branch.
Spire records the review cycle, reviewed SHA, path, and ownership marker. It
compares the worktree before and after review; any modification fails the review
contract and cannot be published as maker code.

A new head SHA requires a new review cycle and a new detached reviewer worktree.

### 5. Allocation is durable and recoverable

Workspace allocation uses a durable intent:

1. Persist a workspace record in `allocating` state with repository, root/review
   identity, base/head SHA, branch when applicable, and intended path.
2. Commit the SQLite transaction.
3. Execute explicit Git commands through `WorkspacePort`; never hold a transaction
   across Git or filesystem IO.
4. Write an exact ownership marker.
5. Inspect the resulting Git worktree and transition the record to `ready`.

After a crash, reconciliation inspects SQLite, `git worktree list --porcelain`,
the filesystem, and the ownership marker. It adopts only an exact match. Missing,
foreign, ambiguous, or mismatched state is quarantined; Spire never deletes or
resets it speculatively.

An existing local or remote branch with the intended name is not automatically
adopted. Spire verifies its recorded ownership and expected ancestry or blocks for
an explicit audited operator decision.

### 6. Local worktree and remote branch lifecycles are separate

Spire may remove an owned local worktree only after:

- no live Run, lease, process, or review cycle references it;
- terminal retention has expired;
- the path is below the configured worktree root;
- the ownership marker and Git common directory match SQLite; and
- Git recognizes it as the expected worktree.

Cleanup uses `git worktree remove` followed by bounded `git worktree prune`.
Broad recursive deletion is not the primary cleanup mechanism.

Removing a local worktree does not imply deleting its branch. A local or remote
branch needed by an open PR is retained. Branch deletion requires a separate,
explicit policy after canonical PR terminal state is recorded.

### 7. Publication authority is orthogonal

Worktree-first execution does not decide whether the maker pushes directly or
Spire uses a mechanical publisher. That authority remains governed by
`security-and-authority.md`.

In either model:

- the harness does not choose or rename the branch;
- the reviewer cannot publish worktree changes;
- neither harness may merge; and
- branch protection remains an independent enforcement boundary.

## Application and adapter boundaries

The application-owned workspace contract distinguishes maker and reviewer
allocation:

```text
prepare_repository(mapping) -> repository_status
allocate_maker(work_item_id, root_run_id, base_sha) -> workspace
allocate_review(review_cycle_id, head_sha) -> workspace
inspect(workspace_id) -> workspace_status
quarantine(workspace_id, reason)
cleanup(workspace_id)
```

Application DTOs contain repository identity, workspace kind, root/review identity,
base/head SHA, branch when applicable, intended path, and lifecycle state.

The Git/worktree adapter owns command invocation and parsing. It uses explicit
executables and argument vectors, bounded output, timeouts, and validated paths.
The domain and application layers import no Git library, process API, or filesystem
implementation.

## Consequences

### Positive

- Concurrent tickets never share mutable checkout files.
- Maker continuations and corrections retain their exact local state.
- Review starts from a fresh checkout of the exact CI-green SHA.
- The operator's registered checkout remains outside Spire cleanup authority.
- Branch/worktree identity survives restart and is auditable in SQLite.
- Linear content cannot escape into Git names or filesystem paths.

### Costs and trade-offs

- Worktree allocation requires Git command/recovery logic rather than directory
  creation alone.
- The registered source repository and Git common directory must remain available.
- Worktree metadata can drift after manual Git operations and needs reconciliation.
- Reviewer worktrees consume additional disk during review.
- Existing workspace rows and the directory-only allocator require migration.

## Rejected alternatives

- **Run directly in the registered checkout:** prevents safe concurrency and makes
  cleanup/ownership ambiguous.
- **Let Linear choose branch or worktree:** gives mutable untrusted tracker content
  local resource authority.
- **Create one worktree per child Run:** fragments maker state and makes correction
  lineage harder to recover.
- **Let reviewer inspect the maker worktree:** leaks mutable/uncommitted maker state
  and does not prove review of the CI-green SHA.
- **Use directories without Git worktree registration:** cannot prove branch,
  common-directory, or cleanup ownership.
- **Delete branch when deleting worktree:** conflates local resource cleanup with PR
  lifecycle.

## Follow-up

- The Git-aware adapter and forward-only workspace schema migration are implemented
  by [`../sprints/14-durable-project-routing.md`](../sprints/14-durable-project-routing.md).
- Preserve Sprints 05, 08, and 09 as historical records of the behavior and
  evidence delivered at their completion; do not rewrite them to imply this later
  decision was already implemented.
- Prove the implemented worktree lifecycle on the target VM before enabling the
  runtime contract.
- Resolve maker direct-push versus mechanical publisher separately.

## Unknown / unverified

- Exact Git version and worktree behavior on the target VM.
- Whether a registered source that is itself a linked worktree should be accepted
  by resolving its common Git directory or rejected in the first release.
- Branch-name maximum length after combining provider and Git hosting constraints.
- Push/publication ownership until `security-and-authority.md` is completed.
