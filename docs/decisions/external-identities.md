# External identities and authority map

**Status:** runtime identity contract accepted; provider/VM evidence remains operator-supplied
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

## Runtime identities and secret references

Spire runs under the invoking login user by default. Codex and Claude Code use
that user's provider-native authentication; their configuration contains a
provider, model, and effort but no Spire-managed harness credential reference.
A dedicated system identity is an explicit advanced installation profile.

## Managed secret references

Only the following reference forms are permitted in configuration and decision
records. Values must be injected by the deployment environment, never committed.

| Capability | Reference name | Scope |
|---|---|---|
| Linear read/write adapter | `systemd:credentials/linear-api-token` | Linear adapter only |
| GitHub App signing key | `systemd:credentials/github-app-private-key` | publisher only |
| Webhook verification | `systemd:credentials/linear-webhook-secret` | ingress only |

## Completion procedure

1. An operator performs the Sprint 00 read-only API calls and records the IDs in
   their deployment configuration, not this file.
2. Validate that every workflow-state ID belongs to the configured team and that
   the returned estimate scale maps every author-selectable value.
3. Populate the corresponding non-secret values in an operator-owned config file.
4. Attach redacted responses to the change record and update this table to
   `verified` with the observation date.

Provider-native harness authentication and user-systemd persistence need target
VM evidence in Sprints 12 and 13; neither is inferred from an interactive shell.

No production Linear, GitHub, or Cloudflare mutation is authorized by this
document.
