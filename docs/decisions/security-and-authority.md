# Security and authority decisions

**Status:** boundaries approved; credential implementation choices blocked on operator approval
**Last checked:** 2026-07-29

## Non-negotiable boundaries

- Harness credentials cannot merge a pull request.
- Reviewer credentials cannot push branches or tags.
- Webhook credentials only authenticate ingress; they cannot invoke arbitrary
  local commands.
- All external mutations originate from an idempotent outbox action.
- A review waiver requires an explicit human authority, reason, timestamp, and
  immutable audit record.

## Decisions requiring an operator selection

| Decision | Allowed choices | Current state |
|---|---|---|
| GitHub identity | GitHub App or scoped bot token | blocked |
| Push model | maker direct push or mechanical publisher | blocked |
| Admin endpoints | loopback-only or Cloudflare Access | blocked |
| Human-status conflict policy | human wins / documented guarded projection | blocked |
| Webhook-secret storage | systemd credential reference | required |

The GitHub identity must have only the minimum read/write scope needed for the
chosen publisher model. Branch protection and token/app permissions must make a
merge impossible independently of application logic.
