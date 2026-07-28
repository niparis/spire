# Harness normalized outcome taxonomy v1

**Status:** approved for fail-closed fixture classification
**Last checked:** 2026-07-29

Provider events are evidence, not lifecycle authority. A parser may emit exactly
one outcome below for an invocation; unrecognized or malformed input maps to
`unknown_provider_failure` and must retain redacted evidence.

| Class | Normalized outcome | Run handling |
|---|---|---|
| implementation result | `pr_ready`, `specs_needed`, `blocked`, `no_change`, `task_failed` | require schema-valid result |
| review result | `approved`, `changes_required`, `blocked`, `task_failed` | require schema-valid result |
| capacity | `rate_limited`, `quota_exhausted`, `context_exhausted`, `output_limit` | terminal run; WorkItem may wait for provider |
| integration | `auth_failed`, `model_unavailable`, `runner_unhealthy`, `contract_invalid`, `unknown_provider_failure` | stop and alert; do not infer success |

The taxonomy deliberately distinguishes a future-start capacity refusal from an
active-run exhaustion. A refusal may open a circuit and try the next candidate; an
active run is never cancelled simply because future-start capacity is exhausted.

## Fixture contract

Every captured JSONL fixture must have a sidecar record containing:

- fixture name, harness, CLI version, and capture date;
- one normalized outcome from this table;
- observed terminal event (or explicit absence);
- exit status/session identifier presence; and
- redaction confirmation for tokens, paths, usernames, remotes, prompts, and
  authorization material.

`scripts/sprint00/verify-fixture-manifest.sh` enforces this metadata offline.
