# SQLite operating contract

**Status:** local WAL evidence captured; target-VM filesystem and backup location blocked
**Last checked:** 2026-07-29

## Required settings

```sql
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA synchronous = FULL;
PRAGMA busy_timeout = 5000;
```

SQLite is the single-node consistency boundary. The database must reside on a
local filesystem; network-mounted database storage and multiple active scheduler
instances are unsupported.

Run `scripts/sprint00/sqlite-local-spike.sh` for a non-production local check of
WAL, concurrent readers with short writes, online backup, and restore integrity.
It uses an isolated temporary directory and does not touch configured data roots.
It passed on macOS 26.5.1 with the system `sqlite3` binary on 2026-07-29. This is
not evidence for the target VM's filesystem, backup destination, or systemd setup.

## Path-separation policy

| Data | Configured root | Permitted cleanup selector |
|---|---|---|
| database and WAL files | `data_root` | never |
| SQLite backups | `backup_root` | retention-only backup selector |
| Git worktrees | `workspace_root` | owned-worktree selector only |
| harness evidence | `evidence_root` | per-run retention selector only |

All four configured roots must canonicalize to distinct paths. Cleanup refuses a
path equal to or containing the database root, even if an ownership marker exists.

## Target-VM evidence still required

- filesystem type for `data_root` and `backup_root`;
- concurrent-reader and short `BEGIN IMMEDIATE` behavior;
- online backup and restore followed by `PRAGMA integrity_check`;
- forced-controller-kill result with no corruption; and
- a cleanup-negative test demonstrating that the database root cannot be selected.
