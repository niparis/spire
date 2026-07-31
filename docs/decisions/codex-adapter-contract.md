# Codex adapter contract

**Status:** successful run and usage-limit failure captured; resume still required
**Observed CLI:** `codex-cli 0.146.0-alpha.3.1`; success captured 2026-07-31
with model `gpt-5.6-luna`

The adapter invokes `codex exec --json` with an explicit model, permission
profile, output schema, workspace, and deadline. It must persist the run
identifier before treating a mutating run as recoverable, and it may resume only
the same harness continuation.

Captured evidence lives in `tests/fixtures/harness/codex/`. The capture procedure
is [`../runbooks/harness-fixture-capture.md`](../runbooks/harness-fixture-capture.md).

## Where the structured result lives

Codex does not place the structured result on its terminal event. `turn.completed`
carries only `{type, usage}`. The result appears as the `text` of an
`agent_message` item, encoded as a **JSON string inside the event**, and it is
also written verbatim to the `--output-last-message` file.

**Read the `--output-last-message` file.** It contained exactly the final result
and nothing else, which makes it the cheapest reliable source.

**A schema-valid result is emitted more than once, and the first one is a plan.**
The capture contains two conforming `agent_message` items:

| Item | `outcome` | `evidence_reference` |
|---|---|---|
| `item_0` | `pr_ready` | "I'll append the requested line to NOTES.md and verify…" |
| `item_4` | `pr_ready` | "NOTES.md now has the appended line…; git diff shows no other file changes." |

`item_0` was emitted before any file was touched and already claimed
`pr_ready`. An adapter that accepts the first schema-valid message records a
declared intention as a completed outcome. Take the last conforming message, or
the `--output-last-message` file, and never the first.

## `parse_jsonl_result` does not match either provider

`parse_jsonl_result` in `crates/spire-adapters/src/harness.rs` parses each raw
line as a `StructuredRunResult`. No captured line from either harness carries
`schema_version` at the top level: Claude Code nests the result at
`.structured_output` on its terminal event, and Codex nests it inside
`.item.text` as an encoded JSON string. Against real captures the helper
therefore returns `InvalidContract` for both providers.

It was written before any capture existed and describes a format no provider
emits. Sprint 05 must replace it with per-provider extraction that yields the
shared `StructuredRunResult`, rather than treat it as a working shared parser.

## Event shape

A successful run brackets `item.started` / `item.completed` pairs between
`turn.started` and `turn.completed`:

```text
thread.started                       -> carries thread_id
turn.started
item.completed  agent_message        -> plan; already schema-conforming
item.started    command_execution
item.completed  command_execution    -> carries command, exit_code
item.started    file_change
item.completed  file_change          -> carries changes[].path and kind
item.started    command_execution
item.completed  command_execution
item.completed  agent_message        -> final result
turn.completed                       -> carries usage only
```

The failure path stops before any item:

```text
thread.started   -> carries thread_id
turn.started
error            -> carries message
turn.failed      -> carries error.message
```

Not every item is bracketed: the two `agent_message` items appeared as
`item.completed` with no preceding `item.started`. An adapter must not assume
every item id is opened before it is closed.

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

## Required captures still outstanding

- `exec resume` against the recorded thread;
- invalid model, invalid profile, malformed output, cancellation, timeout, and
  unavailable-network events.

Sprint 05 must still reject unknown Codex event variants rather than treating
them as success.

## Unknown / Unverified

- Whether `thread_id` is stable across `exec resume`.
- Whether a failing run also writes an `--output-last-message` file; the
  usage-limit capture produced none.
- Whether the usage-limit message format is localized or stable enough to
  detect by prefix, which the opaque-wait recommendation avoids relying on.
- Exact installed model IDs beyond the configured `gpt-5.6-luna`.
