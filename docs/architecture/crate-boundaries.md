# Crate boundaries

`spire-domain` owns value objects, lifecycle aggregates, provider-neutral policy,
and invariants. It has no framework, database, network, filesystem, or provider
imports.

`spire-application` owns use-case DTOs, configuration validation, dispatch
evaluation orchestration, and ports. Ports accept domain/application values only.

`spire-adapters` will own implementations of those ports for SQLite, Linear,
GitHub, worktrees, systemd, Codex, and Claude Code. Sprint 01 intentionally has
no live adapter implementation.

`spire` is the composition root. It owns CLI translation, Tokio, Axum, tracing,
and process lifecycle; HTTP handlers delegate to application services.

`scripts/check-architecture.sh` checks that `spire-domain` and
`spire-application` do not gain forbidden infrastructure dependencies.
