# Codex adapter contract

**Status:** partial local feasibility evidence; successful run fixture blocked
**Observed CLI:** `codex-cli 0.146.0-alpha.3.1` on 2026-07-29

The adapter will invoke `codex exec --json` with an explicit model, permission
profile, output schema, workspace, and deadline. It must persist the session/run
identifier before treating a mutating run as recoverable, and it may resume only
the same harness continuation.

## Local observation

`codex --version` returned the version above. The CLI also emitted a warning that
it could not create PATH aliases because the sandbox denied that unrelated
filesystem change. This does not establish an execution, JSONL, resume, or
capacity contract.

## Required captures still blocked

- successful no-op repository task and schema-valid structured result;
- `exec resume` against the recorded session;
- invalid model, invalid profile, malformed output, cancellation, timeout, and
  unavailable-network events;
- any safely observable rate/usage-limit event.

Each capture must be redacted and listed in
`tests/fixtures/harness/fixture-manifest.json`. Until then, Sprint 05 must reject
unknown Codex event variants rather than treating them as success.
