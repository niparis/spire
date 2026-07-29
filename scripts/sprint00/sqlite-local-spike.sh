#!/usr/bin/env bash
set -euo pipefail

spire_tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/spire-s00-sqlite.XXXXXX")"
trap 'rm -rf "$spire_tmpdir"' EXIT

spire_db="$spire_tmpdir/spire.db"
spire_backup="$spire_tmpdir/spire.backup.db"

sqlite3 "$spire_db" >/dev/null <<'SQL'
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA synchronous = FULL;
PRAGMA busy_timeout = 5000;
CREATE TABLE write_probe (id INTEGER PRIMARY KEY, value TEXT NOT NULL);
SQL

(
  for _ in $(seq 1 50); do
    spire_attempt=0
    until sqlite3 "$spire_db" 'SELECT count(*) FROM write_probe;' >/dev/null 2>&1; do
      spire_attempt=$((spire_attempt + 1))
      if [[ "$spire_attempt" -ge 20 ]]; then
        printf 'reader could not acquire the SQLite lock after %s attempts\n' \
          "$spire_attempt" >&2
        exit 1
      fi
      sleep 0.05
    done
  done
) &
spire_reader_pid=$!

for spire_number in $(seq 1 50); do
  spire_attempt=0
  until sqlite3 "$spire_db" "BEGIN IMMEDIATE; INSERT INTO write_probe(value) VALUES ('write-$spire_number'); COMMIT;" >/dev/null 2>&1; do
    spire_attempt=$((spire_attempt + 1))
    if [[ "$spire_attempt" -ge 20 ]]; then
      printf 'short write %s could not acquire the SQLite lock after %s attempts\n' \
        "$spire_number" "$spire_attempt" >&2
      exit 1
    fi
    sleep 0.05
  done
done
wait "$spire_reader_pid"

sqlite3 "$spire_db" ".backup '$spire_backup'"
spire_integrity="$(sqlite3 "$spire_backup" 'PRAGMA integrity_check;')"
if [[ "$spire_integrity" != "ok" ]]; then
  printf 'backup integrity check failed: %s\n' "$spire_integrity" >&2
  exit 1
fi

printf 'SQLite local spike passed: WAL, concurrent reader/short writes, and online backup restore.\n'
