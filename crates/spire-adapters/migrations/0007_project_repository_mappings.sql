CREATE TABLE project_repository_mappings (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 256),
    linear_organization_id TEXT NOT NULL CHECK (length(linear_organization_id) BETWEEN 1 AND 256),
    linear_team_id TEXT NOT NULL CHECK (length(linear_team_id) BETWEEN 1 AND 256),
    linear_project_id TEXT NOT NULL CHECK (length(linear_project_id) BETWEEN 1 AND 256),
    linear_project_name_snapshot TEXT NOT NULL CHECK (length(linear_project_name_snapshot) BETWEEN 1 AND 256),
    github_repository TEXT NOT NULL CHECK (length(github_repository) BETWEEN 1 AND 256),
    repository_source_path TEXT NOT NULL CHECK (length(repository_source_path) BETWEEN 1 AND 4096),
    git_common_directory TEXT NOT NULL CHECK (length(git_common_directory) BETWEEN 1 AND 4096),
    git_remote_url TEXT NOT NULL CHECK (length(git_remote_url) BETWEEN 1 AND 4096),
    default_branch TEXT NOT NULL CHECK (length(default_branch) BETWEEN 1 AND 256),
    status TEXT NOT NULL CHECK (status IN ('enabled', 'disabled', 'removed')),
    revision INTEGER NOT NULL CHECK (revision > 0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (linear_organization_id, linear_project_id)
);

CREATE TABLE project_repository_mapping_history (
    id INTEGER PRIMARY KEY NOT NULL,
    mapping_id TEXT NOT NULL REFERENCES project_repository_mappings(id) ON DELETE RESTRICT,
    actor TEXT NOT NULL CHECK (length(actor) BETWEEN 1 AND 256),
    operation TEXT NOT NULL CHECK (operation IN ('created', 'revised', 'disabled', 'removed')),
    previous_revision INTEGER CHECK (previous_revision IS NULL OR previous_revision > 0),
    new_revision INTEGER NOT NULL CHECK (new_revision > 0),
    reason TEXT,
    mapping_snapshot_json TEXT NOT NULL CHECK (length(mapping_snapshot_json) BETWEEN 2 AND 32768),
    created_at INTEGER NOT NULL
);

CREATE INDEX project_repository_mappings_status
ON project_repository_mappings(status, linear_organization_id, linear_project_id);

CREATE INDEX project_repository_mapping_history_mapping
ON project_repository_mapping_history(mapping_id, id);
