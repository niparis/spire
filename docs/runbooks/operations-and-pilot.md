# Operations and controlled-pilot runbook

**Status:** The commands and local safeguards below are implemented. VM-specific
identity, disk/inode measurements, provider recovery, reboot drills, and pilot
stage approval remain operator evidence—not assumptions encoded in Spire.

## Install and start

1. Create a dedicated, non-login `spire` user and group. The user owns only
   `/var/lib/spire`, `/etc/spire`, and the service credential files; it must not
   join a human login group or receive unrelated secret access.
2. Install the binary at `/usr/local/bin/spire`, the validated configuration at
   `/etc/spire/spire.yaml`, and the units from `deploy/systemd/` under
   `/etc/systemd/system/`.
3. Store each secret in the matching `/etc/spire/credentials/<name>` file with
   mode `0600` and owner `root:spire`. Never copy credentials into YAML, logs,
   workspaces, backups, or shell history.
4. Run `systemctl daemon-reload` then `systemctl enable --now spire.service
   spire-backup.timer spire-restore-drill.timer cloudflared.service`.

`spire.service` validates configuration before start, runs as `spire`, has a
120-second graceful stop period, and never requires Cloudflare Tunnel for local
correctness. The tunnel depends on Spire; Spire does not depend on the tunnel.

## Health, guards, and alerts

Use the loopback admin endpoint:

```text
curl --fail http://127.0.0.1:8081/health/live
curl --fail http://127.0.0.1:8081/health/ready
curl --fail http://127.0.0.1:8081/admin/operations
spire ops status --config /etc/spire/spire.yaml
```

The operations snapshot exposes aggregate inbox/outbox depth, active total and
AI runs, and terminal-workspace cleanup backlog. It intentionally excludes ticket
text and secrets. Alert immediately on a nonzero old queue, a stuck active run,
failed integrity check, absent backup, an unknown provider reset, no eligible
checker, tunnel outage, or resource-guard breach. Correlate an alert using the
structured `work_item_id`, `run_id`, `root_run_id`, PR, and SHA fields.

The host collector must compare free disk and inode values against the validated
`operations.minimum_free_*` thresholds before new harness admission. A failed
guard blocks admission only; recovery, reconciliation, outbox delivery, backup,
and cleanup continue. Do not solve disk pressure by deleting a workspace manually.

## Backup, restore, and cleanup

The daily timer calls:

```text
spire db backup-daily --config /etc/spire/spire.yaml
```

It creates a dated online SQLite backup below `runtime.backup_root` and retains
only the validated number of `spire-*.db` files. Backups are never workspace
cleanup candidates. Verify a backup by restoring to a new empty path:

```text
spire db restore-check \
  --backup /var/lib/spire/backups/spire-<timestamp>.db \
  --destination /var/lib/spire/restore-drills/spire-<timestamp>.db
```

The weekly restore-drill timer uses `spire db restore-latest --config
/etc/spire/spire.yaml`, which selects only a dated backup from the configured
backup root and restores it below `runtime.data_root/restore-drills/`.

Record the backup timestamp, restore duration, integrity outcome, and operations
snapshot in the drill evidence. A production restore follows
[SQLite backup and restore](sqlite-backup-restore.md); stop the service before
replacing its live database.

Cleanup must first prove terminal database state, expired retention, no active
lease/live unit, an in-root path, and an exact `.spire-owner` marker. The cleanup
adapter rejects symlinked, unowned, mismatched, missing, and out-of-root paths.
Quarantine a failed cleanup and investigate; never force-remove it from a broad
workspace root.

## Recovery and rollback

- **Lost run:** retain the workspace/evidence, inspect the external harness or
  process group, then use the durable run lease state. Never launch a second
  mutating run merely because a heartbeat stopped.
- **Provider wait:** inspect candidate health and reset time. Do not switch a
  sticky maker or select the maker as reviewer; capacity waits consume no
  engineering correction round.
- **Webhook/tunnel outage:** leave the service running; use narrow Linear and
  GitHub reconciliation after connectivity returns. Do not replay raw requests
  by hand.
- **Linear/GitHub conflict:** preserve the human action, capture the canonical
  facts, and block/escalate ambiguous state rather than force-pushing or merging.
- **Review waiver:** require an authenticated human actor, reason, and exact
  current SHA. A later push invalidates it; a waiver never overrides failed CI.
- **Kill switch:** set `rollout.linear_writes_enabled: false`, validate config,
  and restart `spire.service`. This stops admission while recovery and
  observability continue. Re-enable only after a recorded stage decision.

## Pilot stages and failure-drill evidence

| Stage | Scope | Minimum evidence before advance | Rollback condition | Owner |
|---|---|---|---|---|
| 0 | Disposable repo, synthetic tickets | All 15 drill cases converge; restore succeeds | Any duplicate mutating run or autonomous merge | Named operator |
| 1 | One repo, `chore`, total/AI `1/1` | Ten completed tickets and seven-day observation | Any unexplainable durable state or guard breach | Named operator |
| 2 | Add bugs | Ten additional tickets; no same-provider review | Correction/waiver rate exceeds agreed threshold | Named operator |
| 3 | Raise total/AI to `3/1` | Measured CPU, memory, disk, and inode headroom | Resource guard or queue-age alert | Named operator |
| 4 | Features/refactors | Explicit operations review approval | Any prior-stage rollback trigger | Named operator |

For every drill retain timestamp, operator, correlation IDs, expected state,
observed state, evidence path, and corrective action. Required cases are duplicate
and missed webhooks, both crash windows, restart with a live unit, provider
capacity/error permutations, head change during review, full AI capacity, PR
close/merge during outage, tunnel outage, disk/cleanup failure, and backup/restore.

Do not claim Sprint 09 exit criteria until a second operator has followed the
backup/restore, lost-run, kill-switch, and credential-rotation procedures on the
target VM without author assistance.
