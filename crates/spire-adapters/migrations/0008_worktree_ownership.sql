ALTER TABLE project_repository_mappings
ADD COLUMN authority_state TEXT NOT NULL DEFAULT 'healthy'
CHECK (authority_state IN ('healthy', 'stale', 'unhealthy'));

ALTER TABLE work_items ADD COLUMN linear_project_id TEXT;
ALTER TABLE work_items ADD COLUMN linear_project_name_snapshot TEXT;
ALTER TABLE work_items
ADD COLUMN project_mapping_id TEXT
REFERENCES project_repository_mappings(id) ON DELETE RESTRICT;
ALTER TABLE work_items
ADD COLUMN project_mapping_revision INTEGER
CHECK (project_mapping_revision IS NULL OR project_mapping_revision > 0);

ALTER TABLE workspaces
ADD COLUMN kind TEXT NOT NULL DEFAULT 'historical'
CHECK (kind IN ('historical', 'maker', 'reviewer'));
ALTER TABLE workspaces
ADD COLUMN root_run_id TEXT REFERENCES runs(id) ON DELETE RESTRICT;
ALTER TABLE workspaces
ADD COLUMN review_cycle_id TEXT REFERENCES review_cycles(id) ON DELETE RESTRICT;
ALTER TABLE workspaces ADD COLUMN workspace_root TEXT;
ALTER TABLE workspaces ADD COLUMN repository_source_path TEXT;
ALTER TABLE workspaces ADD COLUMN git_common_directory TEXT;
ALTER TABLE workspaces ADD COLUMN base_sha TEXT;
ALTER TABLE workspaces ADD COLUMN head_sha TEXT;
ALTER TABLE workspaces ADD COLUMN branch TEXT;
ALTER TABLE workspaces
ADD COLUMN allocation_state TEXT NOT NULL DEFAULT 'historical'
CHECK (
    allocation_state IN (
        'historical',
        'allocating',
        'ready',
        'quarantined',
        'removing',
        'removed'
    )
);
ALTER TABLE workspaces
ADD COLUMN marker_version INTEGER NOT NULL DEFAULT 0
CHECK (marker_version IN (0, 1));

CREATE UNIQUE INDEX workspaces_maker_root
ON workspaces(root_run_id)
WHERE kind = 'maker' AND allocation_state != 'removed';

CREATE UNIQUE INDEX workspaces_reviewer_cycle
ON workspaces(review_cycle_id)
WHERE kind = 'reviewer' AND allocation_state != 'removed';

CREATE UNIQUE INDEX workspaces_owned_branch
ON workspaces(branch)
WHERE branch IS NOT NULL AND allocation_state != 'removed';

CREATE TRIGGER workspaces_git_identity_insert
BEFORE INSERT ON workspaces
WHEN NEW.kind IN ('maker', 'reviewer')
BEGIN
    SELECT CASE
        WHEN NEW.marker_version != 1
          OR NEW.workspace_root IS NULL
          OR NEW.repository_source_path IS NULL
          OR NEW.git_common_directory IS NULL
          OR NEW.base_sha IS NULL
          OR (NEW.kind = 'maker' AND (NEW.root_run_id IS NULL OR NEW.branch IS NULL))
          OR (NEW.kind = 'reviewer' AND (
              NEW.review_cycle_id IS NULL
              OR NEW.head_sha IS NULL
              OR NEW.branch IS NOT NULL
          ))
        THEN RAISE(ABORT, 'invalid Git worktree identity')
    END;
END;

CREATE TRIGGER workspaces_git_identity_update
BEFORE UPDATE OF
    kind,
    root_run_id,
    review_cycle_id,
    workspace_root,
    repository_source_path,
    git_common_directory,
    base_sha,
    head_sha,
    branch,
    marker_version
ON workspaces
WHEN NEW.kind IN ('maker', 'reviewer')
BEGIN
    SELECT CASE
        WHEN NEW.marker_version != 1
          OR NEW.workspace_root IS NULL
          OR NEW.repository_source_path IS NULL
          OR NEW.git_common_directory IS NULL
          OR NEW.base_sha IS NULL
          OR (NEW.kind = 'maker' AND (NEW.root_run_id IS NULL OR NEW.branch IS NULL))
          OR (NEW.kind = 'reviewer' AND (
              NEW.review_cycle_id IS NULL
              OR NEW.head_sha IS NULL
              OR NEW.branch IS NOT NULL
          ))
        THEN RAISE(ABORT, 'invalid Git worktree identity')
    END;
END;

CREATE TRIGGER work_items_mapping_snapshot_pair_insert
BEFORE INSERT ON work_items
WHEN (NEW.project_mapping_id IS NULL) != (NEW.project_mapping_revision IS NULL)
BEGIN
    SELECT RAISE(ABORT, 'incomplete project mapping snapshot');
END;

CREATE TRIGGER work_items_mapping_snapshot_pair_update
BEFORE UPDATE OF project_mapping_id, project_mapping_revision ON work_items
WHEN (NEW.project_mapping_id IS NULL) != (NEW.project_mapping_revision IS NULL)
BEGIN
    SELECT RAISE(ABORT, 'incomplete project mapping snapshot');
END;

CREATE TRIGGER project_mapping_history_reason_insert
BEFORE INSERT ON project_repository_mapping_history
WHEN NEW.reason IS NOT NULL
  AND (length(trim(NEW.reason)) = 0 OR length(NEW.reason) > 1024)
BEGIN
    SELECT RAISE(ABORT, 'invalid project mapping history reason');
END;
