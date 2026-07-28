#!/usr/bin/env bash
set -euo pipefail

manifest="tests/fixtures/harness/fixture-manifest.json"
require_captured=false

if [[ "${1:-}" == "--require-captured" ]]; then
  require_captured=true
elif [[ $# -ne 0 ]]; then
  printf 'usage: %s [--require-captured]\n' "$0" >&2
  exit 64
fi

jq -e '
  .schema_version == 1 and
  (.fixtures | type == "array" and length > 0) and
  all(.fixtures[];
    (.id | type == "string" and length > 0) and
    (.harness == "codex" or .harness == "claude_code") and
    (.capture_state == "captured" or .capture_state == "required" or .capture_state == "blocked") and
    (.normalized_outcome | IN(
      "pr_ready", "specs_needed", "blocked", "no_change", "task_failed",
      "approved", "changes_required", "rate_limited", "quota_exhausted",
      "context_exhausted", "output_limit", "auth_failed", "model_unavailable",
      "runner_unhealthy", "contract_invalid", "unknown_provider_failure"
    )) and
    (.required_fields | type == "array" and length > 0) and
    (.redaction_confirmed | type == "boolean")
  )
' "$manifest" >/dev/null

if "$require_captured"; then
  jq -e 'all(.fixtures[]; .capture_state == "captured" and .redaction_confirmed)' "$manifest" >/dev/null
fi

printf 'fixture manifest is structurally valid%s\n' \
  "$($require_captured && printf ' and fully captured' || printf '')"
