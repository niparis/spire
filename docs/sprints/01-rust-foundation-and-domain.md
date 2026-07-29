# Sprint 01 — Rust Foundation and Domain

**Last Verified:** 2026-07-28  
**Depends on:** Sprint 00 exit criteria  
**Unlocks:** Sprint 02

## Outcome

A compiling Rust modular monolith with validated configuration, pure domain types,
application ports, deterministic dispatch-policy evaluation, and health endpoints.
No database or external API is required to run its unit tests.

## Entry criteria

- Rust toolchain and minimum supported version are selected.
- Dispatch policy version 1 and complexity mapping are known.
- Harness capability matrix and normalized capacity taxonomy are approved.

## Target structure

```text
Cargo.toml
crates/
├── spire-domain/
├── spire-application/
├── spire-adapters/
└── spire/
config/
└── spire.example.yaml
```

Dependency direction:

```text
spire-domain <- spire-application <- spire-adapters <- spire binary
```

## Work packages

### S01.1 Bootstrap the Cargo workspace

Implementation:

1. Create the four crates and workspace dependency declarations.
2. Pin Rust edition and minimum toolchain.
3. Add formatting, linting, testing, and type/documentation commands.
4. Deny unsafe code unless an ADR explicitly permits it.
5. Add CI locally executable commands:
   `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
   and `cargo test --workspace`.

Verification:

- Empty workspace builds and tests from a clean checkout.
- Domain crate dependency tree contains no infrastructure crate.

### S01.2 Implement domain identifiers and value objects

Create validated types for:

- `WorkItemId`, `RunId`, `ReviewCycleId`, and `WorkspaceId`.
- `LinearIssueId`, `LinearIdentifier`, `RepositoryName`, and `CommitSha`.
- `ComplexityEstimate` and `ComplexityClass`.
- `HarnessId`, `ModelId`, `Effort`, and `CredentialProfile`.
- `DispatchPolicyVersion`, `DispatchRuleId`, and `CandidateIndex`.
- `Initiator`, `TriggerKind`, `RunRole`, and `ProviderCapacity`.

Rules:

- Reject empty/unbounded strings at construction.
- Keep provider-specific aliases outside the domain.
- Serialize stable wire names explicitly; never rely on Rust variant debug names.

Verification:

- Round-trip serialization tests exist for every persisted enum.
- Invalid identifiers fail before reaching adapters.

### S01.3 Implement the dispatch-policy aggregate

Implementation:

1. Parse a provider-neutral `DispatchPolicy`.
2. Validate unique rule IDs and exact coverage for every supported
   `(role, complexity)` pair.
3. Validate candidates against a `HarnessCapabilityRegistry`.
4. Evaluate rules as a pure function.
5. Filter candidates using immutable provider-health input.
6. Return a `DispatchEvaluation` containing every candidate and selection/skip
   reason.
7. Reject a review selection equal to the sticky maker harness.

Required skip reasons:

```text
unsupported_capability
circuit_open
auth_disabled
model_unavailable
same_as_maker
selected
```

Verification:

- Golden table from Sprint 00 produces exact candidates and selections.
- Rule order cannot hide overlap.
- No healthy candidate returns a wait result, not an exception.

### S01.4 Implement lifecycle aggregates and invariants

Implement domain models for:

- `WorkItem`.
- `Run`.
- `ReviewCycle`.
- `Lease`.
- `DispatchDecision`.
- `ProviderHealth`.

Required invariants:

- One active harness run per ticket.
- One mutating run per repository under the pilot policy.
- Terminal work requires explicit reopen/retry.
- Review requires successful CI for the exact SHA.
- Approval is invalidated by a new head SHA.
- Maker becomes sticky only after successful mutating-run launch.
- Capacity terminal states do not consume correction cycles.

Verification:

- State transition tables are unit-tested exhaustively.
- Invalid transitions return typed domain errors with no side effects.

### S01.5 Define application ports

Define interfaces in the core for:

- `LinearPort`.
- `GitHubPort`.
- `HarnessRunnerPort`.
- `WorkspacePort`.
- `ClockPort`.
- `NotifierPort`.
- `UnitOfWork`.

Rules:

- Ports use domain/application DTOs, never SDK types.
- Methods communicate idempotency keys and expected versions explicitly.
- External result ambiguity is represented, not collapsed to `bool`.

Verification:

- In-memory fake implementations support application tests.
- The application crate compiles without Axum, SQLx, Reqwest, or provider CLIs.

### S01.6 Implement configuration loading and validation

Implementation:

1. Define one typed root configuration.
2. Support a committed redacted example and an operator-owned real file.
3. Resolve managed-provider secrets from environment/systemd credential
   references; Codex and Claude Code use provider-native runtime-user auth.
4. Validate complexity mapping, harness registry, dispatch policy, concurrency,
   paths, status IDs, and timeouts as one operation.
5. Reject unknown fields to catch spelling mistakes.
6. Add `spire config validate` and `spire dispatch dry-run`.
7. Do not implement hot reload.

Verification:

- Missing and unknown keys fail with precise paths.
- Invalid policy never reaches service readiness.
- Dry-run output includes policy version, rule, candidates, and skip reasons.

### S01.7 Add process entrypoint and health API

Implementation:

1. Start a Tokio runtime and Axum server.
2. Add request IDs and structured tracing.
3. Implement `GET /health/live`.
4. Implement `GET /health/ready` using injected readiness checks.
5. Bind admin routes to a separately configurable loopback listener.
6. Implement graceful shutdown cancellation tokens without starting workers yet.

Verification:

- Liveness does not call external systems.
- Invalid configuration prevents ready status.
- Shutdown completes without abandoned tasks.

### S01.8 Enforce architecture boundaries

Implementation:

1. Add dependency checks or CI scripts that detect forbidden crate edges.
2. Document which crate owns each port and aggregate.
3. Add compile-time tests where practical.

Verification:

- Introducing SQLx into `spire-domain` fails the architecture check.
- Controllers contain translation and delegation only.

## Suggested pull-request slices

1. Workspace, CI commands, and dependency rules.
2. Domain value objects and aggregates.
3. Dispatch policy and capability registry.
4. Ports, configuration CLI, and health API.

## Sprint demo

Run a dispatch dry run over every complexity class, show rejected invalid
configuration, and exercise domain transitions entirely in memory.

## Exit criteria

- All workspace checks pass.
- Dispatch policy is deterministic and fully unit-tested.
- Core crates contain no infrastructure dependencies.
- Configuration and health entrypoints work.
- Ports are sufficient for Sprint 02 and Sprint 03 adapters.

## Evidence Sources

- [`../ai_harness_architecture.md`](../ai_harness_architecture.md)
- [`../ai_harness_implementation.md`](../ai_harness_implementation.md)
- Sprint 00 decisions and fixtures, when completed.

## Unknown / Unverified

- Exact crate versions remain unverified until Sprint 00 pins them.
- Target paths are proposed because the Rust workspace does not yet exist.
