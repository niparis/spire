#!/usr/bin/env bash
# Redact one captured harness JSONL into a committable fixture.
#
# Redaction is mechanical so it is repeatable and reviewable. It is not a
# substitute for a human reading the result: `redaction_confirmed` in the
# manifest records a human decision, never this script's exit status.
#
# Preserved on purpose: event ordering, event and subtype names, terminal
# events, schema version fields, error codes and messages, HTTP status, usage
# counters, and the structured result. Those carry the provider contract.
set -euo pipefail

if [[ $# -ne 2 ]]; then
  printf 'usage: %s <captured.jsonl> <redacted.jsonl>\n' "$0" >&2
  exit 64
fi

source_file="$1"
destination="$2"

if [[ ! -r "${source_file}" ]]; then
  printf 'unreadable capture: %s\n' "${source_file}" >&2
  exit 66
fi

home_prefix="${HOME%/}"
user_name="$(id -un)"

# Structural redaction removes identifiers and host inventories; the string
# scrub then rewrites anything path-shaped or account-shaped that survives
# inside free text, including provider error messages.
jq -c \
  --arg home "${home_prefix}" \
  --arg user "${user_name}" '
  def scrub_string:
    gsub("\\u0000"; "")
    | gsub($home; "/redacted-home")
    | gsub($user; "redacted-user")
    | gsub("[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}"; "REDACTED_UUID")
    | gsub("[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\\.[A-Za-z]{2,}"; "REDACTED_EMAIL")
    | gsub("(?<k>(sk|pk)-)[A-Za-z0-9_-]{8,}"; "REDACTED_TOKEN")
    | gsub("(?<k>[Bb]earer )[A-Za-z0-9._-]{8,}"; "REDACTED_TOKEN");

  def scrub_strings:
    walk(if type == "string" then scrub_string else . end);

  def redact_key($key; $placeholder):
    if has($key) then .[$key] = $placeholder else . end;

  # Host inventories describe the operator workstation, not the provider
  # contract. Keep the keys so event shape survives; empty the values.
  (if has("mcp_servers") then .mcp_servers = [] else . end)
  | (if has("agents") then .agents = [] else . end)
  | (if has("slash_commands") then .slash_commands = [] else . end)
  | (if has("tools") then .tools = [] else . end)
  | (if has("skills") then .skills = [] else . end)
  | (if has("plugins") then .plugins = [] else . end)
  | (if has("memory_paths") then .memory_paths = {} else . end)
  | (if has("cwd") then .cwd = "/redacted-workspace" else . end)
  | redact_key("uuid"; "REDACTED_UUID")
  | redact_key("session_id"; "REDACTED_SESSION_ID")
  | walk(
      if type == "object" then
          (if has("signature") then .signature = "REDACTED_SIGNATURE" else . end)
        | (if has("thinking") then .thinking = "REDACTED_THINKING" else . end)
        | (if has("id") and (.type? == "tool_use") then .id = "REDACTED_TOOL_USE_ID" else . end)
        | (if has("tool_use_id") then .tool_use_id = "REDACTED_TOOL_USE_ID" else . end)
      else . end
    )
  | scrub_strings
  ' "${source_file}" > "${destination}"

# Every emitted line must be valid JSON; a partial redaction is a failed one.
line_number=0
while IFS= read -r line; do
  line_number=$((line_number + 1))
  if ! printf '%s' "${line}" | jq -e . > /dev/null 2>&1; then
    printf 'line %s of %s is not valid JSON after redaction\n' \
      "${line_number}" "${destination}" >&2
    exit 65
  fi
done < "${destination}"

source_lines="$(wc -l < "${source_file}" | tr -d ' ')"
destination_lines="$(wc -l < "${destination}" | tr -d ' ')"
if [[ "${source_lines}" != "${destination_lines}" ]]; then
  printf 'line count changed during redaction: %s -> %s\n' \
    "${source_lines}" "${destination_lines}" >&2
  exit 65
fi

printf 'redacted %s -> %s (%s lines)\n' \
  "${source_file}" "${destination}" "${destination_lines}"
