# Sprint 15 — Guided Onboarding and Linear Project Provisioning

**Last Verified:** 2026-07-29
**Depends on:** Sprint 14 exit criteria
**Unlocks:** Operator-friendly VM pilot onboarding

## Outcome

An operator can download the released binary, run `spire init`, diagnose the
result, and run `spire new PATH` to either map an existing Linear project or create
one and map it to an existing local Git repository. The workflows are resumable,
fail closed, persist all resolved identities, and leave production automation
disabled until a separate rollout decision.

## Entry criteria

- User-scoped service/configuration and `spire doctor` pass on the target VM.
- Linear and GitHub service authentication is valid.
- Codex, Claude Code, Git, and SSH probes pass as the runtime user.
- Existing-project mappings are durable and authoritative in SQLite.
- The authenticated target workspace has verified the current
  `ProjectCreateInput`, project-list filters, permissions, and ambiguous-create
  behavior.

## Target command surface

```text
spire init
spire init --resume
spire init --non-interactive --answers PATH
spire new [PATH]
spire new [PATH] --linear-project existing
spire new [PATH] --linear-project create
```

No secret is accepted as a command-line argument or in an answers file.

## Work packages

### S15.1 Define resumable onboarding use cases

Create application-owned state machines for:

- installation initialization;
- provider discovery;
- harness selection;
- existing-project mapping;
- Linear project provisioning.

Each step is:

```text
pending
running
confirmed
needs_input
failed_retryable
failed_terminal
```

Implementation:

1. Persist non-secret progress and input hashes in SQLite.
2. Separate prompting through a `PromptPort`.
3. Separate external reads/writes through existing or new provider ports.
4. Resume from the last confirmed step after process interruption.
5. Reject changed answers that conflict with confirmed external effects.
6. Expose a redacted plan before any local or external mutation.

Verification:

- Pure state-machine tests cover interruption at every boundary.
- Resume never repeats a confirmed local or external effect.
- Unknown persisted step versions stop for migration.

### S15.2 Implement Linear onboarding discovery

Discover through read APIs:

- viewer and organization;
- available teams;
- workflow states for the selected team;
- estimate scale;
- existing Spire-compatible webhooks;
- existing projects available to the selected team.

Implementation:

1. Paginate every connection and respect rate limits.
2. Match common workflow-state names only as suggestions.
3. Require confirmation of every semantic state mapping.
4. Persist stable IDs and diagnostic names in generated configuration/state.
5. Hash or omit untrusted descriptions.
6. Refuse a team whose workflow/estimate contract cannot satisfy eligibility.

Verification:

- Fixtures cover multiple organizations/teams, renamed states, incomplete estimate
  scales, pagination, rate limits, and ambiguous names.
- Suggestions never become bindings without confirmation.
- Discovery performs no provider mutation.

### S15.3 Implement GitHub and local-repository discovery

Implementation:

1. Validate the approved GitHub API identity and installation/account metadata.
2. Inspect `PATH` as an existing local Git repository.
3. Resolve its canonical path and Git common directory as the registered worktree
   source.
4. Resolve its remote, GitHub repository, and default branch.
5. Reuse the runtime user's normal SSH configuration.
6. Query required checks and repository metadata through read-only APIs.
7. Verify Git worktree capability without changing the registered checkout.
8. Reject unsafe repository state that prevents isolated worktree allocation.

Verification:

- Fixtures cover SSH aliases, multiple remotes, detached HEAD, dirty worktree,
  inaccessible repository, renamed default branch, and path movement.
- GitHub API authority and Git transport authority remain separate results.
- Discovery cannot add repository authority before a mapping is committed.
- Discovery never switches, resets, cleans, or edits the registered checkout.

### S15.4 Implement harness selection in `spire init`

Implementation:

1. Present detected Codex and Claude Code installations/authentication state.
2. Collect maker and reviewer provider, model, and effort.
3. Enforce different providers.
4. Offer only model/effort values supported by captured capability evidence; allow
   explicit manual entry only with an unverified warning that blocks readiness
   until probed.
5. Generate the versioned dispatch policy and show complete complexity coverage.
6. Persist rollout as disabled.

Verification:

- Same-provider review cannot be confirmed.
- Unsupported model/effort cannot become ready.
- Re-running init preserves an unchanged policy version and increments it only when
  the effective rules change.

### S15.5 Assemble `spire init`

Implementation sequence:

1. Resolve installation profile and paths.
2. Preview local directories/config/service changes.
3. Establish or verify Linear and GitHub authentication.
4. Discover and confirm Linear workflow configuration.
5. Probe Codex, Claude Code, Git, and SSH.
6. Collect maker/reviewer choices.
7. initialize/migrate SQLite;
8. atomically write generated configuration;
9. install the user service definition;
10. run the equivalent of `spire doctor`;
11. print unresolved actions and leave rollout disabled.

Rules:

- Local writes are explicit in the preview.
- External provider mutations are not performed by the base init flow.
- Existing files are backed up or left unchanged; no blind overwrite.
- `--non-interactive` requires a versioned answer schema and stops on missing
  decisions.
- An answers file contains no secrets.

Verification:

- Clean, repeated, interrupted, partially configured, and migrated installations
  converge.
- Failure after any write leaves a resumable state and complete files.
- A successful init still cannot admit a ticket.

### S15.6 Add durable provisioning operations

Add `0009_onboarding_provisioning.sql` for provisioning operations. Persist:

```text
operation_id
operation_kind
desired_input_hash
status
provider
external_id
attempt_count
last_error_class
created_at
updated_at
```

Implementation:

1. Record intent before issuing a Linear mutation.
2. Commit the provisioning state and a `linear_project_create` outbox action
   atomically.
3. Lease and deliver that action through the outbox; do not call Linear directly
   from the transaction or prompt controller.
4. Do not hold a SQLite transaction across provider IO.
5. Store the returned Linear project ID before creating the mapping.
6. Use a deterministic operation identity for retries.
7. Reconcile ambiguous outcomes by listing projects in the selected organization,
   team, and expected name.
8. If zero candidates exist, retry within budget.
9. If one exact candidate exists, require operator confirmation before adoption
   unless immutable evidence proves it was created by this operation.
10. If multiple candidates exist, stop with `needs_input`.

Verification:

- Tests cover crash before request, timeout before response, success before local
  acknowledgement, duplicate-name candidate, rate limit, permission failure, and
  process restart.
- A retry cannot silently create a second project after an ambiguous outcome.
- Operation rows and mapping rows remain mutually consistent.

### S15.7 Implement Linear project creation

Extend `LinearPort` with provider-neutral project list/get/create operations.
The adapter uses the verified GraphQL `projectCreate` contract.

Implementation:

1. Require organization/team, name, and any verified mandatory input.
2. Show the exact proposed project and target team.
3. Require interactive confirmation or an explicit approved non-interactive flag.
4. Submit creation through the durable provisioning use case.
5. Normalize and persist the returned project identity.
6. Do not create workflow states, labels, initiatives, documents, or tickets as
   hidden side effects.
7. Log identifiers and operation ID, never credential or raw provider payload.

Verification:

- Deterministic fixtures validate request shape and partial GraphQL error handling.
- Permission failure makes no mapping.
- Project creation is bounded to the selected organization/team.

### S15.8 Implement `spire new`

Interactive flow:

1. Inspect the local repository.
2. Show canonical GitHub repository, remote, path, and default branch.
3. Select Linear organization/team.
4. Choose `existing` or `create`.
5. For `existing`, select one canonical project through read APIs.
6. For `create`, execute the durable provisioning operation.
7. Preview the resulting mapping.
8. Commit the mapping and history atomically.
9. Run `spire projects doctor`.
10. Report that rollout remains disabled.

Implementation:

1. Default `PATH` to the current directory.
2. Support stable JSON output and versioned non-interactive input.
3. Refuse to replace an existing project mapping implicitly.
4. Require revision and reason for reassignment.
5. Preserve old mapping history and active-work snapshots.

Verification:

- Existing and create paths converge on the same mapping use case.
- Re-running against the same repo/project is idempotent.
- Mapping a project already bound to another repository fails.
- Multiple Linear projects may intentionally map to one repository.

### S15.9 Add onboarding observability and support bundle

Implementation:

1. Add structured correlation IDs for onboarding/provisioning operations.
2. Expose redacted status through `spire status` and `spire doctor`.
3. Add `spire support bundle --redacted` containing versions, resolved non-secret
   paths, diagnostic codes, migration versions, and operation IDs.
4. Exclude credentials, raw provider events, issue text, repository content, SSH
   configuration content, and environment dumps.
5. Document recovery for interrupted init and ambiguous project creation.

Verification:

- Secret and untrusted-content sentinels never enter the bundle.
- A second operator can diagnose every onboarding failure fixture from the bundle
  and runbook.

### S15.10 Validate the clean-VM onboarding journey

On a clean supported VM:

1. Download and verify the Sprint 11 release.
2. Authenticate the login user's Codex, Claude Code, Git, and SSH normally.
3. Run `spire init`.
4. Run `spire doctor`.
5. Run `spire new` against a disposable local repository.
6. Exercise both existing-project mapping and new-project creation.
7. Log out and reboot.
8. Re-run doctor and mapping reconciliation.
9. Confirm rollout remains disabled and no ticket was claimed.

Retain timestamps, versions, operation IDs, redacted output, and rollback evidence.

## Suggested pull-request slices

1. Onboarding state machines, prompt abstraction, and Linear discovery.
2. GitHub/repository discovery, harness selection, and base `spire init`.
3. Provisioning migration/use case and Linear project creation adapter.
4. `spire new`, support bundle, runbook, and clean-VM evidence.

## Sprint demo

Starting only with a released binary and an already-authenticated login user, run
`spire init` to produce a healthy disabled installation. From an existing local
repository, first map an existing Linear project, then use a second disposable
repository to create and map a new Linear project. Interrupt and resume one
creation after an ambiguous response. Reboot and show that both mappings reconcile,
diagnostics pass, and no automation was enabled.

## Exit criteria

- A new operator can complete onboarding without hand-authoring the full YAML.
- `spire init` is resumable and leaves rollout disabled.
- `spire new` supports both existing and newly created Linear projects.
- Linear project creation is durable, auditable, and ambiguity-safe.
- Project mappings are committed only after canonical provider identities are
  confirmed.
- The login user's provider-native harness and SSH authentication works after
  logout/reboot.
- Clean-VM evidence and second-operator runbook verification are retained.

## Evidence Sources

- [`../decisions/first-run-onboarding-and-project-mapping.md`](../decisions/first-run-onboarding-and-project-mapping.md)
- [Linear GraphQL API](https://linear.app/developers/graphql)
- [Linear SDK data modification](https://linear.app/developers/sdk-fetching-and-modifying-data)
- Sprint 11 release artifact, Sprint 13 diagnostics, and Sprint 14 mapping fixtures.

## Unknown / Unverified

- Exact `ProjectCreateInput` fields and permissions until verified against the
  target workspace.
- Whether webhook creation belongs in `spire init` or a later explicit command.
- Target-VM behavior when the registered worktree source is itself a linked
  worktree.
- Final CLI spelling for project registration and reassignment.
