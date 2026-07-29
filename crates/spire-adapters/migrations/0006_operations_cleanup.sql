ALTER TABLE workspaces ADD COLUMN cleanup_started_at INTEGER;
ALTER TABLE workspaces ADD COLUMN cleanup_completed_at INTEGER;
ALTER TABLE workspaces ADD COLUMN reclaimed_bytes INTEGER;
ALTER TABLE workspaces ADD COLUMN quarantine_reason TEXT;

CREATE INDEX workspaces_terminal_cleanup
ON workspaces(status, updated_at);
