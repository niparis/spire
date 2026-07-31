# Harness fixture capture

**Status:** procedure verified against installed CLIs on macOS 2026-07-30;
captures not yet performed
**Satisfies:** S00.4 (Codex) and S00.5 (Claude Code) fixture artifacts

Sprint 05 cannot implement a runner until Sprint 00 has captured redacted
provider evidence. This runbook is the repeatable procedure for producing that
evidence. Fixtures are captures, never hand-written representations of a
provider; see [`../../tests/fixtures/README.md`](../../tests/fixtures/README.md).

## The gate

`./scripts/sprint00/verify-fixture-manifest.sh --require-captured` is the Sprint
00 exit gate. It requires every row in
[`../../tests/fixtures/harness/fixture-manifest.json`](../../tests/fixtures/harness/fixture-manifest.json)
to have `capture_state: "captured"` and `redaction_confirmed: true`. Each row
must also list its `required_fields`, currently `terminal_event`, `exit_status`,
`session_id`, and `structured_output`.

A row that cannot be captured keeps the gate closed. Record credit-gated and
environment-gated cases in the adapter decision records under **Unknown /
Unverified** instead of adding a manifest row for them.

## Preconditions

Confirm each of these before capturing. The first two were verified on
2026-07-30; the versions are recorded in the adapter decision records.

1. `codex login status` reports a logged-in account.
2. `claude auth status --json` reports `"loggedIn": true`.
3. `codex --version` and `claude --version` match the approved shapes the
   diagnostics probe accepts (`codex-cli <numeric>` and
   `<numeric> (Claude Code)`).
4. The capture shell is interactive and not inside an agent sandbox. The
   2026-07-29 attempt failed because a sandbox denied unrelated filesystem
   writes, which produced no execution contract.

## Prohibited during capture

- Never capture against the `spire` working tree. Harness runs mutate their
  working directory; use a disposable repository.
- Never log out, rotate, or overwrite the operator's real provider credentials.
  Induce an authentication failure with an empty `CODEX_HOME` instead.
- Never enable `--fallback-model`. Fallback is a dispatch-policy decision and a
  concealed model substitution invalidates the capture.
- Never commit an unredacted capture, even temporarily.

## Disposable capture environment

```sh
capture_root="$(mktemp -d)"
repo="${capture_root}/repo"
evidence="${capture_root}/evidence"
mkdir -p "${repo}" "${evidence}"
git -C "${repo}" init --quiet
printf 'fixture capture target\n' > "${repo}/NOTES.md"
git -C "${repo}" add NOTES.md
git -C "${repo}" -c user.email=capture@invalid -c user.name=Capture \
  commit --quiet -m "chore: seed capture repository"
```

The repository has no remote. That keeps a push impossible and keeps remote URLs
out of the captured events.

Write the shared result schema once. It mirrors `StructuredRunResult` in
[`../../crates/spire-application/src/execution.rs`](../../crates/spire-application/src/execution.rs),
so a capture proves the contract the parser already enforces:

```sh
cat > "${capture_root}/result-schema.json" <<'JSON'
{
  "type": "object",
  "additionalProperties": false,
  "required": ["schema_version", "outcome", "session_id", "evidence_reference"],
  "properties": {
    "schema_version": {"type": "integer", "enum": [1]},
    "outcome": {
      "type": "string",
      "enum": ["pr_ready", "specs_needed", "blocked", "no_change", "task_failed"]
    },
    "session_id": {"type": ["string", "null"]},
    "evidence_reference": {"type": "string"}
  }
}
JSON
```

The two harnesses do not accept the same JSON Schema. Claude Code accepts
ordinary JSON Schema; Codex forwards the schema to OpenAI structured outputs,
which rejects the request with `invalid_json_schema` (HTTP 400) unless every
property carries an explicit `type`, every property appears in `required`, and
`additionalProperties` is `false`. Validator keywords such as `const`,
`minLength`, `pattern`, and `minimum` are not accepted there.

The schema above is the intersection that both providers accept. Keep it that
way: a schema that only Claude tolerates fails Codex at request validation,
before any inference, which is cheap to discover but easy to miss because the
Claude capture succeeds first. Non-emptiness of `evidence_reference` is enforced
by `parse_structured_result`, not by the schema.

Use one no-op task for every success capture so outcomes stay comparable:

```text
Append the line 'fixture capture' to NOTES.md, then stop. Make no other change.
```

## Codex captures (S00.4)

| Case | Expected normalized outcome | Method |
|---|---|---|
| `codex-success` | `pr_ready` or `no_change` | no-op task with output schema |
| `codex-resume` | same as the resumed session | `codex exec resume <session-id>` |
| `codex-invalid-model` | `model_unavailable` | `-m not-a-real-model` |
| `codex-auth-failed` | `auth_failed` | empty `CODEX_HOME` |
| `codex-cancelled` | `task_failed` | `SIGINT` mid-run |
| `codex-network-unavailable` | `runner_unhealthy` | unroutable `HTTPS_PROXY` |

Success capture. Replace `<selected-model>` with an exact model ID from the
operator's account; installed model IDs are an installation-specific discovery
input and must not be guessed:

```sh
codex exec --json \
  --model <selected-model> \
  --sandbox workspace-write \
  --cd "${repo}" \
  --skip-git-repo-check \
  --output-schema "${capture_root}/result-schema.json" \
  --output-last-message "${evidence}/codex-success.last.json" \
  "Append the line 'fixture capture' to NOTES.md, then stop. Make no other change." \
  > "${evidence}/codex-success.jsonl" 2> "${evidence}/codex-success.stderr"
printf 'exit=%s\n' "$?" > "${evidence}/codex-success.exit"
```

Record the exit status in its own file for every case. It is a required field
and it is not recoverable from the JSONL after the fact.

Resume proves the same-harness continuation contract. Read the session ID from
the success JSONL, then:

```sh
codex exec resume "<session-id>" --json \
  --cd "${repo}" \
  "Report the last change you made. Change nothing." \
  > "${evidence}/codex-resume.jsonl" 2> "${evidence}/codex-resume.stderr"
```

Authentication failure, without touching real credentials:

```sh
CODEX_HOME="$(mktemp -d)" codex exec --json \
  --model <selected-model> --cd "${repo}" --skip-git-repo-check \
  "Do nothing." \
  > "${evidence}/codex-auth-failed.jsonl" 2>&1
```

Network unavailability, without changing host networking:

```sh
HTTPS_PROXY=http://127.0.0.1:1 HTTP_PROXY=http://127.0.0.1:1 \
  codex exec --json --model <selected-model> --cd "${repo}" --skip-git-repo-check \
  "Do nothing." \
  > "${evidence}/codex-network-unavailable.jsonl" 2>&1
```

Cancellation, sending one graceful signal and recording what the provider emits
before exit:

```sh
codex exec --json --model <selected-model> --sandbox workspace-write \
  --cd "${repo}" --skip-git-repo-check \
  "Append one line to NOTES.md every few seconds until told to stop." \
  > "${evidence}/codex-cancelled.jsonl" 2>&1 &
codex_pid=$!
sleep 20
kill -INT "${codex_pid}"
wait "${codex_pid}"; printf 'exit=%s\n' "$?" > "${evidence}/codex-cancelled.exit"
```

## Claude Code captures (S00.5)

| Case | Expected normalized outcome | Method |
|---|---|---|
| `claude-success` | `pr_ready` or `no_change` | no-op task with inline JSON schema |
| `claude-resume` | same as the resumed session | `claude -r <session-id>` |
| `claude-invalid-model` | `model_unavailable` | `--model not-a-real-model` |
| `claude-auth-failed` | `auth_failed` | isolated `HOME` |
| `claude-cancelled` | `task_failed` | `SIGINT` mid-run |
| `claude-budget-exceeded` | `quota_exhausted` or a distinct code | `--max-budget-usd 0.01` |

`--json-schema` takes inline JSON, not a file path. Effort accepts `low`,
`medium`, `high`, `xhigh`, and `max`:

```sh
claude -p "Append the line 'fixture capture' to NOTES.md, then stop. Make no other change." \
  --output-format stream-json \
  --verbose \
  --model <selected-model> \
  --effort high \
  --permission-mode acceptEdits \
  --max-budget-usd 1.00 \
  --json-schema "$(cat "${capture_root}/result-schema.json")" \
  > "${evidence}/claude-success.jsonl" 2> "${evidence}/claude-success.stderr"
printf 'exit=%s\n' "$?" > "${evidence}/claude-success.exit"
```

Run this from inside `${repo}`; Claude Code takes its working directory from the
process, not a flag. Confirm at capture time whether `--output-format
stream-json` still requires `--verbose` under `-p`; the flag is harmless when
redundant, so it is included above.

Record from the success capture: the `system/init` event, the session ID, the
result subtype, structured error fields, HTTP status, usage, and the terminal
process status. Then resume:

```sh
claude -r "<session-id>" -p "Report the last change you made. Change nothing." \
  --output-format stream-json --verbose \
  > "${evidence}/claude-resume.jsonl" 2>&1
```

A reviewer resume must never inherit maker context. Capture the resume so
Sprint 08 can prove that boundary from evidence rather than assertion.

`--max-budget-usd 0.01` is the safest available way to observe a spend-limit
refusal. Confirm whether it maps to `quota_exhausted` or a distinct code before
assigning its normalized outcome; a budget refusal is not necessarily an account
quota exhaustion.

## Redaction

Redact every capture before it leaves the temporary directory. Remove or
replace:

- API tokens, session cookies, and authorization headers;
- account email addresses, organization IDs, and subscription details, as
  emitted by `claude auth status --json`;
- absolute paths, the temporary directory name, and the host username;
- repository remotes and any hostname;
- prompt and completion text beyond what the outcome requires;
- machine identifiers and IP addresses.

Preserve event ordering, event type names, the terminal event, schema version
fields, error codes, HTTP status values, usage counters, and the structured
result. Those carry the contract. Replace a redacted value with a stable
placeholder such as `REDACTED_SESSION_ID` rather than deleting the key, so the
shape of the event survives.

Verify each redacted file parses and contains nothing sensitive:

```sh
while read -r line; do printf '%s' "${line}" | jq -e . > /dev/null; done \
  < tests/fixtures/harness/codex/codex-success.jsonl
grep -nEi 'ni\.paris|@gmail|/Users/|sk-|Bearer |orgId' \
  tests/fixtures/harness/codex/*.jsonl tests/fixtures/harness/claude/*.jsonl
```

The `grep` must find nothing. Set `redaction_confirmed: true` only after a human
has read the file, not because the `grep` was quiet.

## Install and verify

1. Copy redacted files to `tests/fixtures/harness/codex/<case>.jsonl` and
   `tests/fixtures/harness/claude/<case>.jsonl`.
2. Add or update the matching manifest row: `id`, `harness` (`codex` or
   `claude_code`), `capture_state: "captured"`, the single
   `normalized_outcome`, `required_fields`, and `redaction_confirmed: true`.
3. Each captured fixture maps to exactly one normalized outcome. If a capture
   plausibly maps to two, the taxonomy is wrong; resolve it in
   [`../decisions/harness-outcome-taxonomy-v1.md`](../decisions/harness-outcome-taxonomy-v1.md)
   before recording the row.
4. Run the structural check, then the exit gate:

```sh
./scripts/sprint00/verify-fixture-manifest.sh
./scripts/sprint00/verify-fixture-manifest.sh --require-captured
```

5. Update [`../decisions/codex-adapter-contract.md`](../decisions/codex-adapter-contract.md)
   and [`../decisions/claude-adapter-contract.md`](../decisions/claude-adapter-contract.md)
   with the observed contract and move anything still missing to **Unknown /
   Unverified**.
6. Remove the temporary capture directory, which still holds unredacted
   material:

```sh
rm -rf "${capture_root}"
```

## Cost control

Each success and resume capture is one no-op task. Keep prompts explicitly
bounded, pass `--max-budget-usd` on every Claude capture, and never capture a
real usage-limit event by exhausting an account.

## Unknown / Unverified

- `rate_limited`, `quota_exhausted`, and `context_exhausted` cannot be captured
  without hitting real provider limits. S00.4 item 5 permits retaining these as
  unverified fixture requirements. Sprint 05 must reject unknown variants rather
  than infer success, so the safety property does not depend on them.
- `output_limit` has no confirmed forcing mechanism on either CLI.
- `contract_invalid` describes a provider response the schema rejects and cannot
  be reliably induced. Exercise it in tests by mutating a captured fixture; do
  not create a hand-written fixture for it.
- S00.6 run recovery is no longer platform-blocked: execution is portable, so the
  spike runs on any development host. See
  [`../decisions/harness-process-execution.md`](../decisions/harness-process-execution.md).
  It is still unexercised against a live harness.
- Exact model IDs, effort mappings, and whether Claude usage-limit reset
  timestamps are machine-readable or message-only remain installation-specific
  discovery inputs.
