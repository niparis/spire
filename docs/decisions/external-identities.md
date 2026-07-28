# External identities and authority map

**Status:** blocked on operator-supplied read-only evidence
**Decision owner:** platform operator
**Last checked:** 2026-07-29

Sprint 00 must not guess identifiers or copy secret values into the repository.
The committed example configuration contains names and credential references only.

| System | Required value | Evidence to retain | Status |
|---|---|---|---|
| Linear | organization ID | redacted read-only organization response | unverified |
| Linear | test-team ID and workflow-state IDs | each state resolves and belongs to that team | unverified |
| Linear | estimate scale and bot actor ID | redacted workspace/team response | unverified |
| GitHub | pilot repository, base branch, required checks | read-only repository and branch-protection response | unverified |
| GitHub | App installation identity | read-only installation response | unverified |
| Cloudflare | account/zone owner and webhook hostname | operator-owned DNS/Access record | unverified |
| VM | distribution and systemd version | `cat /etc/os-release`; `systemctl --version` | unverified |

## Secret references

Only the following reference forms are permitted in configuration and decision
records. Values must be injected by the deployment environment, never committed.

| Capability | Reference name | Scope |
|---|---|---|
| Linear read/write adapter | `systemd:credentials/linear-api-token` | Linear adapter only |
| GitHub App signing key | `systemd:credentials/github-app-private-key` | publisher only |
| Codex authentication | `systemd:credentials/codex-auth` | Codex runner only |
| Claude authentication | `systemd:credentials/claude-auth` | Claude runner only |
| Webhook verification | `systemd:credentials/linear-webhook-secret` | ingress only |

## Completion procedure

1. An operator performs the Sprint 00 read-only API calls and records the IDs in
   their deployment configuration, not this file.
2. Validate that every workflow-state ID belongs to the configured team and that
   the returned estimate scale maps every author-selectable value.
3. Populate the corresponding non-secret values in an operator-owned config file.
4. Attach redacted responses to the change record and update this table to
   `verified` with the observation date.

No production Linear, GitHub, or Cloudflare mutation is authorized by this
document.
