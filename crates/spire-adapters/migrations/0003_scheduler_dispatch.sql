ALTER TABLE work_items ADD COLUMN repository TEXT;
ALTER TABLE work_items ADD COLUMN ready_at INTEGER;
ALTER TABLE work_items ADD COLUMN manual_expedite INTEGER NOT NULL DEFAULT 0 CHECK (manual_expedite IN (0, 1));
ALTER TABLE work_items ADD COLUMN active_run_id TEXT;
ALTER TABLE work_items ADD COLUMN resume_state TEXT;
ALTER TABLE runs ADD COLUMN initiator TEXT NOT NULL DEFAULT 'human' CHECK (initiator IN ('human', 'ai', 'system'));
ALTER TABLE runs ADD COLUMN trigger_kind TEXT NOT NULL DEFAULT 'linear_ready';
CREATE INDEX work_items_scheduler_queue ON work_items(state, manual_expedite DESC, ready_at, linear_identifier);
