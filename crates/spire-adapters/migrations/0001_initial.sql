CREATE TABLE webhook_inbox (
    id TEXT PRIMARY KEY NOT NULL,
    source TEXT NOT NULL,
    delivery_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    raw_headers TEXT NOT NULL,
    raw_body BLOB NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'processing', 'processed', 'quarantined')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    lease_owner TEXT,
    lease_expires_at INTEGER,
    received_at INTEGER NOT NULL,
    processed_at INTEGER,
    last_error TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (source, delivery_id)
);

CREATE TABLE work_items (
    id TEXT PRIMARY KEY NOT NULL,
    linear_issue_id TEXT,
    state TEXT NOT NULL CHECK (state IN ('observed', 'eligible', 'claiming', 'queued', 'implementing', 'waiting_for_ci', 'waiting_for_review', 'human_ready', 'waiting_for_provider', 'blocked', 'completed', 'canceled')),
    revision TEXT NOT NULL,
    sticky_maker_harness TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE dispatch_decisions (
    id TEXT PRIMARY KEY NOT NULL,
    work_item_id TEXT NOT NULL REFERENCES work_items(id) ON DELETE RESTRICT,
    run_id TEXT NOT NULL,
    policy_version INTEGER NOT NULL CHECK (policy_version > 0),
    rule_id TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('implementation', 'review')),
    complexity_estimate INTEGER NOT NULL CHECK (complexity_estimate > 0),
    complexity_class TEXT NOT NULL CHECK (complexity_class IN ('small', 'medium', 'large', 'xlarge')),
    candidate_schema_version INTEGER NOT NULL CHECK (candidate_schema_version > 0),
    candidates_json TEXT NOT NULL,
    selected_candidate_index INTEGER,
    candidate_evaluation_schema_version INTEGER NOT NULL CHECK (candidate_evaluation_schema_version > 0),
    candidate_evaluations_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE (work_item_id, run_id)
);

CREATE TRIGGER dispatch_decisions_are_immutable
BEFORE UPDATE ON dispatch_decisions
BEGIN
    SELECT RAISE(ABORT, 'dispatch decisions are immutable');
END;

CREATE TABLE runs (
    id TEXT PRIMARY KEY NOT NULL,
    work_item_id TEXT NOT NULL REFERENCES work_items(id) ON DELETE RESTRICT,
    parent_run_id TEXT REFERENCES runs(id) ON DELETE RESTRICT,
    root_run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE RESTRICT,
    role TEXT NOT NULL CHECK (role IN ('implementation', 'review')),
    harness TEXT NOT NULL,
    model TEXT NOT NULL,
    effort TEXT NOT NULL CHECK (effort IN ('low', 'medium', 'high')),
    status TEXT NOT NULL CHECK (status IN ('queued', 'starting', 'running', 'succeeded', 'failed', 'capacity_rejected', 'capacity_exhausted', 'cancel_requested', 'canceled', 'timed_out', 'lost')),
    lease_owner TEXT,
    lease_expires_at INTEGER,
    recovery_pending INTEGER NOT NULL DEFAULT 0 CHECK (recovery_pending IN (0, 1)),
    dispatch_decision_id TEXT REFERENCES dispatch_decisions(id) ON DELETE RESTRICT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX one_active_run_per_work_item
ON runs(work_item_id)
WHERE status IN ('queued', 'starting', 'running', 'cancel_requested');

CREATE TRIGGER run_lineage_must_stay_within_work_item
BEFORE INSERT ON runs
BEGIN
    SELECT CASE WHEN NEW.parent_run_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM runs WHERE id = NEW.parent_run_id AND work_item_id = NEW.work_item_id
    )
        THEN RAISE(ABORT, 'parent run belongs to another work item') END;
    SELECT CASE WHEN NEW.root_run_id != NEW.id AND NOT EXISTS (
        SELECT 1 FROM runs WHERE id = NEW.root_run_id AND work_item_id = NEW.work_item_id
    )
        THEN RAISE(ABORT, 'root run belongs to another work item') END;
END;

CREATE TABLE outbox (
    id TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL,
    aggregate_id TEXT NOT NULL,
    idempotency_key TEXT NOT NULL UNIQUE,
    payload_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'leased', 'delivered', 'failed')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    next_attempt_at INTEGER NOT NULL,
    lease_owner TEXT,
    lease_expires_at INTEGER,
    external_reference TEXT,
    error_class TEXT,
    last_error TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE provider_health (
    harness TEXT NOT NULL,
    model TEXT NOT NULL,
    credential_profile TEXT NOT NULL,
    state TEXT NOT NULL,
    reason TEXT,
    retry_at INTEGER,
    last_probe_at INTEGER,
    consecutive_failures INTEGER NOT NULL DEFAULT 0 CHECK (consecutive_failures >= 0),
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (harness, model, credential_profile)
);

CREATE TABLE review_cycles (
    id TEXT PRIMARY KEY NOT NULL,
    work_item_id TEXT NOT NULL REFERENCES work_items(id) ON DELETE RESTRICT,
    head_sha TEXT NOT NULL,
    ci_state TEXT NOT NULL,
    review_state TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (work_item_id, head_sha)
);

CREATE TABLE workspaces (
    id TEXT PRIMARY KEY NOT NULL,
    work_item_id TEXT NOT NULL REFERENCES work_items(id) ON DELETE RESTRICT,
    run_id TEXT REFERENCES runs(id) ON DELETE RESTRICT,
    path TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE reconciliation_cursors (
    source TEXT PRIMARY KEY NOT NULL,
    cursor TEXT,
    updated_at INTEGER NOT NULL
);

CREATE TABLE notifications (
    id TEXT PRIMARY KEY NOT NULL,
    idempotency_key TEXT NOT NULL UNIQUE,
    channel TEXT NOT NULL,
    severity TEXT NOT NULL,
    subject TEXT NOT NULL,
    body TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
