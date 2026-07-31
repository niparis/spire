# Codex adapter contract

**Status:** usage-limit failure captured; successful run blocked on account quota
**Observed CLI:** `codex-cli 0.146.0-alpha.3.1` on 2026-07-29; capture attempted
2026-07-30 with model `gpt-5.6-luna`

The adapter invokes `codex exec --json` with an explicit model, permission
profile, output schema, workspace, and deadline. It must persist the run
identifier before treating a mutating run as recoverable, and it may resume only
the same harness continuation.

Captured evidence lives in `tests/fixtures/harness/codex/`. The capture procedure
is [`../runbooks/harness-fixture-capture.md`](../runbooks/harness-fixture-capture.md).

## Event shape

The capture attempt produced a complete failure sequence before any work began:

```text
thread.started   -> carries thread_id
turn.started
error            -> carries message
turn.failed      -> carries error.message
```

The run identifier is `thread_id` on `thread.started`. It is **not** named
`session_id`; Claude Code uses `session_id`. The adapters must not share
identifier extraction.

No structured output is emitted on this path, and the process exited non-zero.
The adapter must treat a missing structured result as a failure to classify from
the terminal event, never as an absent-but-successful run.

## Capacity and reset timestamps

The usage-limit event carries its reset time only as English prose inside
`error.message`:

> You've hit your usage limit. Upgrade to Pro (…), visit … or try again at
> Aug 5th, 2026 12:09 PM.

There is no structured reset field. This is the opposite of Claude Code, which
emits a machine-readable `rate_limit_event.rate_limit_info.resetsAt` in epoch
seconds; see [`claude-adapter-contract.md`](claude-adapter-contract.md).

The recommended handling is to treat the Codex reset time as **opaque**: record
that a capacity wait is in effect, do not parse the prose into a timestamp, and
let reconciliation retry. Parsing localized date prose is a correctness risk for
no operational gain, and a capacity wait consumes no engineering correction
round in any case.

## Required captures still blocked

- successful no-op repository task and schema-valid structured result;
- `exec resume` against the recorded thread;
- invalid model, invalid profile, malformed output, cancellation, timeout, and
  unavailable-network events.

The account hit its usage limit on 2026-07-30 with a stated reset of
2026-08-05. Until a success is captured, Sprint 05 must reject unknown Codex
event variants rather than treating them as success, and Sprint 00 cannot
satisfy the two-harness exit criterion.

## Unknown / Unverified

- Whether a successful run reports its structured result on a terminal event or
  through `--output-last-message`, and whether both are populated.
- Whether `thread_id` is stable across `exec resume`.
- Whether the usage-limit message format is localized or stable enough to
  detect by prefix, which the opaque-wait recommendation avoids relying on.
- Exact installed model IDs beyond the configured `gpt-5.6-luna`.
