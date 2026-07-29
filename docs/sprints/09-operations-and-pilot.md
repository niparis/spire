# Sprint 09 — Operations, Hardening, and Controlled Pilot

**Last Verified:** 2026-07-28  
**Depends on:** Sprint 08  
**Unlocks:** Limited production use

## Outcome

The complete orchestrator runs continuously on the homelab VM with approved
concurrency, backups, observability, safe cleanup, recovery runbooks, and a
progressive repository rollout.

## Entry criteria

- One ticket can reach human-ready through different-harness review.
- Linear and GitHub reconciliation repairs missed webhooks.
- No harness credential can merge.
- All earlier sprint tests pass.

## Work packages

### S09.1 Package the systemd service

Implementation:

1. Create dedicated service user and groups.
2. Define filesystem ownership for binary, config, SQLite, backups, evidence, and
   workspaces.
3. Install `spire-orchestrator.service` with restart policy and graceful timeout.
4. Load secrets through approved systemd credential/environment mechanism.
5. Apply service hardening compatible with Git/worktree/harness execution.
6. Order local service and `cloudflared` without making scheduler correctness depend
   on tunnel availability.

Verification:

- Reboot starts services and recovers active state.
- Service user cannot read unrelated user secrets.
- Configuration validation failure prevents readiness and admission.

### S09.2 Activate approved concurrency

Configuration:

```yaml
total_active_harness_runs: 3
ai_initiated_active_harness_runs: 1
mutating_runs_per_repository: 1
active_runs_per_ticket: 1
cleanup_global: 1
```

Rollout:

1. Begin at one total/one AI while validating operations.
2. Raise total to three after resource measurements.
3. Keep AI at one.
4. Record CPU, memory, disk, and token usage by harness/role.
5. Keep limits editable through validated configuration and service restart.

Verification:

- Config change takes effect only for new decisions.
- Existing snapshotted plans and active runs survive restart.
- Three workloads do not violate VM resource guard thresholds.

### S09.3 Implement resource guards

Before admission check:

- Free disk and inode threshold.
- Workspace-root health.
- SQLite integrity/migration state.
- Runner/systemd availability.
- Repository maintenance flag.
- Provider candidate health.

Actions:

- Stop new admission on unsafe state.
- Continue monitoring, reconciliation, outbox delivery where safe, and cleanup.
- Notify operator with exact failed guard.

Verification:

- Disk pressure blocks starts but never deletes active/unowned paths.
- Failed readiness and admission are distinguishable.

### S09.4 Implement metrics, logs, and alerts

Metrics:

- Inbox/outbox depth and oldest age.
- Ready-to-claim latency.
- Queue depth by initiator.
- Active runs by harness/model/effort/role/repository.
- Total and AI slot utilization.
- Dispatch selections/skips.
- Provider circuit state/reset time.
- Run duration/outcome and token usage when exposed.
- CI/review wait and correction rounds.
- Reconciliation drift.
- Cleanup backlog and reclaimed bytes.

Alerts:

- Lost/stuck run.
- Provider wait without known reset.
- No valid checker for CI-green SHA.
- Webhook disabled or tunnel unavailable.
- Backup/integrity failure.
- Disk/resource threshold.
- Old inbox/outbox row.

Verification:

- Each alert has a synthetic trigger and runbook link.
- Correlation fields link ticket, root run, child run, PR, and SHA.

### S09.5 Implement safe cleanup and retention

Implementation:

1. Select only terminal Workspaces past configured retention.
2. Verify ownership marker, database identity, no active lease, and no live unit.
3. Refuse unresolved/symlink-escaped paths.
4. Remove worktree through Git-aware operation.
5. Remove branch only under explicit policy; never delete a remote branch needed by
   an open PR.
6. Record reclaimed bytes and terminal cleanup state.
7. Quarantine failures.

Verification:

- Active, unowned, mismatched, and out-of-root paths are never removed.
- Cleanup crash is idempotently recoverable.

### S09.6 Automate backup and restore drills

Implementation:

1. Schedule daily online SQLite backup.
2. Retain multiple dated backups.
3. Protect backup directory from workspace cleanup.
4. Schedule periodic restore into a temporary path.
5. Run integrity check and compare key counts/cursors.
6. Alert on missing/repeated failed backup.

Verification:

- Operator performs a documented full restore.
- Recovery point and recovery time are measured.

### S09.7 Complete failure-drill matrix

Execute and retain evidence for:

1. Duplicate and missed Linear webhook.
2. Crash after inbox commit.
3. Crash after state/outbox commit.
4. Orchestrator restart with live transient unit.
5. Linear/GitHub outage during projection.
6. New PR SHA during review.
7. Preferred provider exhausted before start.
8. Sticky maker exhausted mid-run.
9. Preferred checker exhausted.
10. Unknown provider error.
11. Full AI slot with human capacity remaining.
12. PR close and human merge during webhook outage.
13. Tunnel outage beyond retry window.
14. Disk pressure and cleanup failure.
15. SQLite backup/restore.

Pass condition:

- Every drill converges to one explainable durable state with no duplicate mutating
  run, same-harness review, or autonomous merge.

### S09.8 Write operator runbooks

Runbooks:

- Service/tunnel start, stop, and upgrade.
- Configuration validation and policy-version rollout.
- Provider credential rotation.
- Circuit inspection/clear.
- Waiting-for-provider diagnosis.
- Lost-run recovery.
- Manual cancel/retry/reassign.
- Linear/GitHub conflict handling.
- Review waiver.
- Disk pressure and quarantine.
- Backup/restore.
- Kill switch and rollback.

Verification:

- A second operator follows each critical runbook without author assistance.

### S09.9 Execute progressive pilot

Stages:

1. Disposable repository and synthetic tickets.
2. One real repository, `chore` only, concurrency one.
3. Add bugs.
4. Raise total concurrency to three.
5. Add features/refactors only after observed stability.

For every stage define:

- Minimum sample size.
- Observation period.
- Success/error thresholds.
- Rollback/kill-switch condition.
- Human owner.

Verification:

- Stage advancement is an explicit recorded decision.
- A failed stage can return to the previous scope without database rollback.

### S09.10 Establish operational review cadence

Review periodically:

- Dispatch outcomes by complexity and candidate.
- Token/cost distribution.
- Provider circuit frequency.
- Human override and waiver frequency.
- CI/review correction effectiveness.
- False Specs Needed/Blocked classifications.
- SQLite contention and database growth.
- VM resource headroom.

Actions:

- Tune dispatch through a new policy version.
- Tune concurrency only from measured headroom.
- Promote unresolved patterns into ADRs or backlog tickets.

## Suggested pull-request slices

1. systemd packaging and resource guards.
2. observability and alerting.
3. cleanup, backup, and runbooks.
4. failure-drill evidence and pilot configuration.

## Sprint demo

Reboot the VM with an active controlled run, recover it, exercise provider and
webhook outages, restore SQLite from backup, complete one real chore through
human-ready, and require a human to merge.

## Exit criteria

- All failure drills pass.
- Backup and restore are proven.
- Approved `3 total / 1 AI` configuration is stable on measured VM resources.
- Alerts and runbooks are actionable.
- Cleanup is demonstrably safe.
- Pilot stage owner approves limited production use.

## Evidence Sources

- [`../ai_harness_implementation.md`](../ai_harness_implementation.md), deployment,
  observability, security, backup, cleanup, testing, and failure drills.
- Evidence generated by Sprints 00–08.

## Unknown / Unverified

- Production SLOs and pilot advancement thresholds require observed data and owner
  approval.
- Long-term need for a second host or PostgreSQL is deliberately unevaluated until
  SQLite shows real limits.

