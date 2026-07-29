ALTER TABLE work_items ADD COLUMN review_correction_cycles INTEGER NOT NULL DEFAULT 0 CHECK (review_correction_cycles >= 0);
ALTER TABLE work_items ADD COLUMN review_candidates_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE work_items ADD COLUMN review_policy_version INTEGER NOT NULL DEFAULT 1 CHECK (review_policy_version > 0);
ALTER TABLE work_items ADD COLUMN review_rule_id TEXT NOT NULL DEFAULT '';

ALTER TABLE review_cycles ADD COLUMN round INTEGER NOT NULL DEFAULT 1 CHECK (round > 0);
ALTER TABLE review_cycles ADD COLUMN implementation_run_id TEXT REFERENCES runs(id) ON DELETE RESTRICT;
ALTER TABLE review_cycles ADD COLUMN review_run_id TEXT REFERENCES runs(id) ON DELETE RESTRICT;
ALTER TABLE review_cycles ADD COLUMN base_sha TEXT;
ALTER TABLE review_cycles ADD COLUMN published_comment_id TEXT;
ALTER TABLE review_cycles ADD COLUMN completed_at INTEGER;

CREATE UNIQUE INDEX review_cycles_active_run_per_head
ON review_cycles(work_item_id, head_sha)
WHERE review_run_id IS NOT NULL;

CREATE TABLE review_findings (
    id TEXT PRIMARY KEY NOT NULL,
    review_cycle_id TEXT NOT NULL REFERENCES review_cycles(id) ON DELETE RESTRICT,
    stable_id TEXT NOT NULL,
    severity TEXT NOT NULL CHECK (severity IN ('critical', 'high', 'medium', 'low')),
    file TEXT NOT NULL,
    line INTEGER,
    title TEXT NOT NULL,
    rationale TEXT NOT NULL,
    requested_change TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE (review_cycle_id, stable_id)
);

CREATE TABLE review_waivers (
    id TEXT PRIMARY KEY NOT NULL,
    work_item_id TEXT NOT NULL REFERENCES work_items(id) ON DELETE RESTRICT,
    head_sha TEXT NOT NULL,
    actor TEXT NOT NULL,
    reason TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    invalidated_at INTEGER,
    UNIQUE (work_item_id, head_sha)
);
