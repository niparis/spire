# Sprint 02 — SQLite Durability and Recovery Core

**Last Verified:** 2026-07-28  
**Depends on:** Sprint 01  
**Unlocks:** Sprint 03

## Outcome

SQLite becomes the durable execution source of truth. The service can ingest an
event, commit lifecycle state and outbox effects atomically, lease work, restart,
and recover without calling Linear, GitHub, or a harness.

## Entry criteria

- Domain identifiers, transitions, and ports compile.
- SQLite local path and backup destination are approved.
- The target VM filesystem assumptions passed Sprint 00.

## Work packages

### S02.1 Establish SQLx SQLite connection policy

Implementation:

1. Create the SQLite adapter and connection factory.
2. Apply on every connection:

```sql
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
PRAGMA synchronous = FULL;
```

3. Establish WAL mode during database initialization.
4. Bound the connection pool; do not use unbounded async writers.
5. Add startup checks for filesystem path, permissions, migrations, and integrity.
6. Stop readiness when initialization fails.

Verification:

- Tests read back every pragma.
- Concurrent readers remain available during a short write.
- Database on a rejected/network filesystem fails configuration validation.

### S02.2 Create forward-only migrations

Create tables for:

- `webhook_inbox`.
- `outbox`.
- `work_items`.
- `runs`.
- `dispatch_decisions`.
- `provider_health`.
- `review_cycles`.
- `workspaces`.
- `reconciliation_cursors`.
- `notifications`.

Implementation rules:

- Use UTC timestamps with one encoding.
- Use explicit enum checks where SQLite can enforce them.
- Store raw event bodies separately from normalized state.
- Store candidate/evaluation JSON with a schema version.
- Add created/updated timestamps and diagnostic failure fields.

Verification:

- Migrations apply from an empty database.
- Reapplying migrations is a no-op.
- A migration checksum mismatch stops readiness.

### S02.3 Enforce uniqueness and lifecycle constraints

Implement:

- Unique `(source, delivery_id)` inbox key.
- Unique external idempotency key per outbox action.
- Partial uniqueness for active run per work item.
- Unique review cycle per `(work_item_id, head_sha)`.
- Unique provider health per `(harness, model, credential_profile)`.
- Parent/root work-item consistency.
- Immutable dispatch-decision identity.
- Foreign keys with deliberate delete behavior; prefer restriction over cascade for
  audit records.

Verification:

- Parallel attempts to create the same active run yield one winner.
- Invalid parent/root relationships fail at the database boundary.
- Terminal audit rows cannot disappear through an accidental parent deletion.

### S02.4 Implement repositories and mapping

Implementation:

1. Map rows to domain values with validation.
2. Treat invalid persisted values as corruption, not defaultable input.
3. Implement repositories for each aggregate.
4. Support optimistic revision checks where human/external state can race.
5. Keep SQL and SQLx types inside the adapter crate.

Verification:

- Repository contract tests run against a temporary real SQLite database.
- Round trips preserve every dispatch and lineage field.

### S02.5 Implement serialized Unit of Work

Implementation:

1. Route scheduler state-changing commands through one application command channel.
2. Start claim/state transactions with `BEGIN IMMEDIATE`.
3. Keep transactions short and deterministic.
4. Prohibit network, process, or filesystem awaits inside a transaction.
5. Return committed outbox work to asynchronous deliverers.

Verification:

- Instrumented tests fail if mocked external IO occurs while a transaction is open.
- Write contention retries with bounded jitter.
- A busy database never creates a duplicate claim.

### S02.6 Implement webhook inbox mechanics

Implementation:

1. Persist source, delivery ID, headers, raw body, receipt time, and initial status.
2. Acknowledge only after commit.
3. Claim pending rows with leases/attempt counters.
4. Mark processed in the same transaction as normalized state changes.
5. Quarantine permanently malformed events.

Verification:

- Duplicate delivery returns success and creates one row.
- Crash after insert but before processing is recoverable.
- Poison events stop retrying and remain inspectable.

### S02.7 Implement transactional outbox

Implementation:

1. Record external writes in the same transaction as their causal state transition.
2. Lease ready actions to an outbox worker.
3. Use deterministic idempotency keys.
4. Persist attempts, next-attempt time, final external reference, and error class.
5. Separate retry policy by action kind.

Verification:

- Crash after state commit cannot lose the external action.
- Crash after external success but before local acknowledgement converges without
  duplicate visible effects.
- One failing destination does not block unrelated outbox actions.

### S02.8 Implement leases and restart recovery

Implementation:

1. Acquire, renew, release, and expire run/outbox leases.
2. Store lease owner and monotonic logical deadlines using injected clock values.
3. On startup, mark expired controls as `recovery_pending`.
4. Do not infer that an external process is dead from lease expiry alone.

Verification:

- Clock-controlled tests cover renewal and expiry boundaries.
- Two workers cannot simultaneously own one lease.
- Restart reconstructs pending work from the database.

### S02.9 Implement backup, restore, and integrity commands

Implementation:

1. Add `spire db backup`.
2. Use SQLite online backup API.
3. Write to an explicit backup directory outside cleanup roots.
4. Add `spire db check` and a documented restore procedure.
5. Include WAL checkpoint policy and backup retention configuration.

Verification:

- Restore produces the same nonterminal work/run state.
- `PRAGMA integrity_check` succeeds.
- Failure to back up alerts but does not corrupt the live database.

## Suggested pull-request slices

1. Connection policy and migrations.
2. Repositories and constraints.
3. Unit of Work, inbox, and outbox.
4. Leases, recovery, backup, and integrity tooling.

## Sprint demo

Insert a duplicate event, process it into a work-item transition plus outbox action,
kill the service between commits, restart, deliver the action once, and restore the
result from backup.

## Exit criteria

- SQLite integration suite passes under concurrent reads/writes.
- Inbox and outbox crash windows are demonstrated.
- Claims can be serialized transactionally.
- Leases and startup recovery work without external providers.
- Backup and restore have been exercised on the target VM.

## Evidence Sources

- [`../ai_harness_implementation.md`](../ai_harness_implementation.md), persistence,
  transaction, lease, recovery, and backup sections.
- Sprint 00 SQLite operating evidence, when completed.

## Unknown / Unverified

- Final retention intervals require operational approval.
- Real write-contention behavior at pilot load remains a Sprint 09 observation.

