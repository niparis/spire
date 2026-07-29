# SQLite backup and restore

`spire db backup --database <absolute-live-path> --destination <absolute-backup-path>`
uses SQLite `VACUUM INTO` to create a consistent online copy. The destination must
be in the configured backup root, outside database, evidence, and worktree cleanup
roots; the command refuses to overwrite the live database.

Verify a backup before retaining it:

```sh
spire db check --database /var/lib/spire/backups/spire-2026-07-29.db
```

Restore is an operator action while Spire is stopped:

1. Keep the current database, WAL, and SHM as a timestamped recovery set.
2. Verify the selected backup with `spire db check`.
3. Copy the verified backup to the configured live database path.
4. Do not copy a stale `-wal` or `-shm` file alongside the restored backup.
5. Start Spire and inspect recovery-pending inbox/outbox leases before admitting
   new work.

The adapter uses UTC epoch seconds for persisted timestamps. Retention count and
checkpoint scheduling remain operational configuration pending approval.
