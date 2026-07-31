# Claude Code adapter contract

**Status:** success and authentication-failure runs captured; resume still required
**Observed environment:** macOS 26.5.1; Node.js v25.8.0; Claude Code 2.1.148;
model `claude-opus-4-7`; `authMethod: claude.ai`, `apiKeySource: none`; 2026-07-30

The adapter invokes `claude -p --output-format stream-json` with an explicit
model, effort, permission mode, and inline `--json-schema`. `--fallback-model`
stays disabled: fallback is a dispatch-policy decision, and a concealed model
substitution would invalidate the persisted `(harness, model, effort)` record.

Captured evidence lives in `tests/fixtures/harness/claude/`. The capture
procedure is [`../runbooks/harness-fixture-capture.md`](../runbooks/harness-fixture-capture.md).

## Result classification

`subtype` does not indicate success. Both captures terminate with
`subtype: "success"` and differ only in `is_error`:

| Fixture | `subtype` | `is_error` | `api_error_status` | `stop_reason` | `terminal_reason` |
|---|---|---|---|---|---|
| `claude-success` | `success` | `false` | `null` | `end_turn` | `completed` |
| `claude-auth-failed` | `success` | `true` | `401` | `stop_sequence` | `completed` |

`terminal_reason` is equally unreliable: it reports `completed` for the
authentication failure. The adapter must classify on `is_error` first, then read
`api_error_status` for the integration taxonomy. An adapter keyed on `subtype`
or `terminal_reason` books authentication failures as completed runs.

## Structured result

The schema-validated result arrives as `structured_output` on the terminal
`result` event. The `result` field holds prose only — `"Done."` in the success
capture — and carries no contract.

The captured `structured_output` is accepted by `parse_structured_result` in
`crates/spire-application/src/execution.rs` and normalizes to `pr_ready`, so the
provider contract and the existing parser agree without translation.

`structured_output.session_id` was `null` while the event-level `session_id`
carried the real identifier. The adapter must persist the session from the event
and never from the structured result. On the authentication failure,
`structured_output` was absent entirely.

## Capacity and reset timestamps

Usage limits arrive as a distinct `rate_limit_event` with machine-readable
fields, observed here at `status: "allowed"`:

```json
{"status": "allowed", "resetsAt": 1785426600, "rateLimitType": "five_hour",
 "overageStatus": "rejected", "overageDisabledReason": "org_level_disabled",
 "isUsingOverage": false}
```

`resetsAt` is epoch seconds. This resolves the Sprint 00 question for Claude:
reset timestamps are structured, not message-only. Codex is the opposite; see
[`codex-adapter-contract.md`](codex-adapter-contract.md). The two adapters must
not share reset-time handling.

Because the event shape is known from an allowed run, a `rate_limited` fixture
requires only the same event at a different `status`.

## Model selection

`--model opus` resolved to `claude-opus-4-7` at runtime. Aliases are prohibited
in dispatch and in capture: an alias that silently re-points would invalidate
both the fixture and the persisted policy decision.

One run reported usage for two models: the selected `claude-opus-4-7` and
`claude-haiku-4-5-20251001`. The harness makes internal model choices beyond the
dispatched selection, so "disable implicit model selection" governs only the
top-level choice. A dispatch record must not claim a run used exactly one model.

## Execution is not hermetic

Runs inherit the login user's Claude Code configuration. Two observed effects:

1. A user-level stop hook errored inside the capture, emitting
   `system/notification` with `key: "stop-hook-error"`.
2. Structured output is enforced through a stop-hook loop. A captured user turn
   contains `Stop hook feedback: You MUST call the StructuredOutput tool`, and
   `num_turns` was 6 for a single-line edit.

A user hook can therefore interfere with the structured-output contract that
Sprint 05 depends on. Maker and reviewer isolation requires a decision about
which user-level configuration a harness run may inherit. That decision is not
settled here.

## Cost reporting

`total_cost_usd` was `0.31271` under subscription authentication with
`apiKeySource: "none"`. The figure is notional and does not indicate an API
account charge, so it cannot be used to infer the billing path.

## Unknown / Unverified

- Session resume is not captured; `claude-resume` remains `required`.
- `rate_limited`, `quota_exhausted`, `context_exhausted`, and `output_limit`
  have no confirmed forcing mechanism that does not exhaust a real account.
- `contract_invalid` cannot be reliably induced. Exercise it by mutating a
  captured fixture rather than creating a hand-written one.
- Whether `--max-budget-usd` applies under subscription authentication.
- Which user-level settings, hooks, and plugins a harness run must be isolated
  from.
