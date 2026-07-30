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
- Default user services do not inject Codex or Claude Code credentials. They may
  read the login user's native provider and SSH configuration without granting
  merge authority.
- Machine-wide writes require explicit `--system` selection and the required
  privilege; a user installation must never silently fall back to `/etc/spire`.

## Decisions requiring an operator selection

| Decision | Allowed choices | Current state |
|---|---|---|
| GitHub identity | GitHub App or scoped bot token | GitHub App; see [`github-app-identity.md`](github-app-identity.md) |
| Push model | maker direct push or mechanical publisher | blocked |
| Admin endpoints | loopback-only or Cloudflare Access | blocked |
| Human-status conflict policy | human wins / documented guarded projection | blocked |
| Webhook-secret storage | systemd credential reference | required |

The GitHub identity must have only the minimum read/write scope needed for the
chosen publisher model. Branch protection and token/app permissions must make a
merge impossible independently of application logic.

The App's `contents` permission stays conditional until the push-model row is
decided. Every other permission is fixed by
[`github-app-identity.md`](github-app-identity.md).
