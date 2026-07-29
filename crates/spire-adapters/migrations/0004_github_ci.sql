ALTER TABLE work_items ADD COLUMN pull_request_number INTEGER;
ALTER TABLE work_items ADD COLUMN pull_request_url TEXT;
ALTER TABLE work_items ADD COLUMN pull_request_branch TEXT;
ALTER TABLE work_items ADD COLUMN base_branch TEXT;
ALTER TABLE work_items ADD COLUMN base_sha TEXT;
ALTER TABLE work_items ADD COLUMN current_head_sha TEXT;
ALTER TABLE work_items ADD COLUMN ci_correction_cycles INTEGER NOT NULL DEFAULT 0 CHECK (ci_correction_cycles >= 0);
ALTER TABLE work_items ADD COLUMN github_conflict_reason TEXT;

CREATE UNIQUE INDEX work_items_pull_request_unique
ON work_items(repository, pull_request_number)
WHERE pull_request_number IS NOT NULL;

CREATE TABLE github_check_evidence (
    id TEXT PRIMARY KEY NOT NULL,
    work_item_id TEXT NOT NULL REFERENCES work_items(id) ON DELETE RESTRICT,
    head_sha TEXT NOT NULL,
    check_name TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'succeeded', 'failed', 'cancelled', 'skipped')),
    details_url TEXT,
    completed_at INTEGER,
    observed_at INTEGER NOT NULL,
    UNIQUE (work_item_id, head_sha, check_name, status, details_url)
);

CREATE INDEX github_check_evidence_current_head
ON github_check_evidence(work_item_id, head_sha);
