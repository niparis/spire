# Project: Spire

A single-node orchestrator that turns ready Linear tickets into bounded Codex or
Claude Code implementation, CI, independent review, and human-merge workflows.

Target stack: Rust, Tokio, Axum/Tower, SQLx with SQLite, `lineark-sdk`, systemd,
Cloudflare Tunnel, Linear, and GitHub. Application code is not scaffolded yet.

## North Star Preferences

- Build the orchestrator, not a custom planner, supervisor, or AI runtime. Treat
  Codex and Claude Code as complete Code Harnesses.
- Keep dependencies inward: domain has no framework or IO imports; application
  defines ports; adapters implement Linear, GitHub, SQLite, filesystem, systemd,
  Codex, and Claude boundaries.
- Use SQLite as the single-node source of truth. Keep write transactions short,
  use the inbox/outbox pattern, and never hold a transaction across external IO.
- Use webhooks for responsiveness and reconciliation for correctness. All external
  events and writes must be idempotent.
- Linear supplies complexity, not harness/model settings. Versioned dispatch rules
  select and persist the `(harness, model, effort)` decision.
- Preserve the configured total and AI-initiated concurrency controls. Capacity
  failures are not engineering failures and must not create retry loops.
- Review current CI-green SHA with fresh context and a different harness from the
  sticky maker. Harnesses never merge.

## Documentation & Roadmap

- Read [`docs/ai_harness_architecture.md`](docs/ai_harness_architecture.md) for
  invariants and system boundaries.
- Read [`docs/ai_harness_implementation.md`](docs/ai_harness_implementation.md) for
  contracts, state machines, persistence, and failure policy.
- The ordered roadmap is [`docs/sprints/README.md`](docs/sprints/README.md). Work
  from the current sprint document and satisfy its entry and exit criteria.
- If implementation disproves an assumption, update the architecture or
  implementation document and every affected sprint document in the same PR.
- Preserve sprint work-package IDs. Record unresolved behavior under
  **Unknown / Unverified** rather than guessing.
- Never read, search, modify, summarize, or use anything under `LEGACY/`.

## Git Workflow

- Work on a focused `codex/<short-description>` branch; never push directly to
  `main`.
- Use the `git-master` skill for all Git operations.
- Prefer small Conventional Commits and reviewable PRs aligned to the current
  sprint's suggested PR slices.
- Do not merge. A human owns final merge authority.

## Verification

- Use deterministic fixtures for Linear, GitHub, Codex, and Claude adapter tests.
- Test crash windows, duplicate/out-of-order events, stale SHAs, capacity
  exhaustion, and recovery—not only the happy path.
- Once Sprint 01 creates the Cargo workspace, use its canonical gates:
  `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and
  `cargo test --workspace`.
- Never log credentials, raw authorization headers, or unredacted provider events.

## Communication

- Be concise and evidence-led. State assumptions and blockers explicitly.
- Ask before expanding authority, weakening an invariant, or enabling external
  writes earlier than the sprint plan allows.
