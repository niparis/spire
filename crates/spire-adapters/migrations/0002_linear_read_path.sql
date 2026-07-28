ALTER TABLE work_items ADD COLUMN linear_identifier TEXT;
ALTER TABLE work_items ADD COLUMN team_id TEXT;
ALTER TABLE work_items ADD COLUMN workflow_state_id TEXT;
ALTER TABLE work_items ADD COLUMN raw_estimate INTEGER;
ALTER TABLE work_items ADD COLUMN complexity_class TEXT;
ALTER TABLE work_items ADD COLUMN eligibility_reason TEXT;
CREATE UNIQUE INDEX work_items_linear_issue_id_unique ON work_items(linear_issue_id);
