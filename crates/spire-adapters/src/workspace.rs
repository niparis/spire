//! Git-aware maker and reviewer worktree ownership.
//!
//! SQLite intent is durable before Git or filesystem effects. Adoption and
//! cleanup require agreement between SQLite, the exact marker, the filesystem,
//! and `git worktree list --porcelain`.

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use spire_application::{
    ExternalResult, MakerWorkspaceRequest, ReviewerWorkspaceRequest, WorkspaceAllocationState,
    WorkspaceKind, WorkspacePort, WorkspaceRecord, WorkspaceRecoverySummary,
};
use thiserror::Error;

use crate::{
    diagnostics::{
        CommandExecutor, CommandRequest, DiagnosticAdapterError, normalize_github_remote,
    },
    sqlite::{SqliteAdapterError, SqliteDatabase},
};

const GIT_TIMEOUT: Duration = Duration::from_secs(20);
const GIT_OUTPUT_LIMIT: usize = 256 * 1024;
const MARKER_NAME: &str = ".spire-owner";

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("workspace identifier is unsafe")]
    UnsafeIdentifier,
    #[error("workspace root is unavailable")]
    RootUnavailable,
    #[error("registered repository source is invalid")]
    InvalidRepositorySource,
    #[error("workspace path escapes its configured root")]
    PathEscape,
    #[error("workspace identity conflicts with durable state")]
    IdentityConflict,
    #[error("workspace ownership evidence is missing or mismatched")]
    OwnershipMismatch,
    #[error("reviewer worktree was modified")]
    ReviewerModified,
    #[error("workspace cleanup is not authorized")]
    CleanupNotAuthorized,
    #[error("Git operation failed: {0}")]
    Git(#[from] DiagnosticAdapterError),
    #[error("workspace persistence failed: {0}")]
    Database(#[from] SqliteAdapterError),
    #[error("filesystem operation failed")]
    Io,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepositorySourceSnapshot {
    pub source_path: String,
    pub git_common_directory: String,
    pub remote_url: String,
    pub github_repository: String,
    pub default_branch: String,
    pub head_sha: String,
    pub worktree_capable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct OwnershipMarker {
    version: u8,
    workspace: WorkspaceRecord,
}

pub struct GitWorkspaceAdapter<E> {
    database: SqliteDatabase,
    executor: E,
    git_executable: PathBuf,
    terminal_retention_seconds: u64,
}

impl<E> GitWorkspaceAdapter<E> {
    pub fn new(
        database: SqliteDatabase,
        executor: E,
        git_executable: impl Into<PathBuf>,
        terminal_retention_seconds: u64,
    ) -> Self {
        Self {
            database,
            executor,
            git_executable: git_executable.into(),
            terminal_retention_seconds,
        }
    }
}

impl<E: CommandExecutor> GitWorkspaceAdapter<E> {
    pub fn inspect_repository_source(
        &self,
        source: impl AsRef<Path>,
    ) -> Result<RepositorySourceSnapshot, WorkspaceError> {
        let source = source
            .as_ref()
            .canonicalize()
            .map_err(|_| WorkspaceError::InvalidRepositorySource)?;
        if !source.is_dir() {
            return Err(WorkspaceError::InvalidRepositorySource);
        }
        let common = self.git(&source, &["rev-parse", "--git-common-dir"])?;
        let common = resolve_git_path(&source, common.stdout.trim())?;
        let remote = self.git(&source, &["remote", "get-url", "origin"])?;
        let remote_url = one_line(&remote.stdout)?;
        let github_repository =
            normalize_github_remote(&remote_url).ok_or(WorkspaceError::InvalidRepositorySource)?;
        let default = self.git(
            &source,
            &[
                "symbolic-ref",
                "--quiet",
                "--short",
                "refs/remotes/origin/HEAD",
            ],
        );
        let default_branch = match default {
            Ok(output) => {
                let branch = one_line(&output.stdout)?;
                branch.strip_prefix("origin/").unwrap_or(&branch).to_owned()
            }
            Err(_) => one_line(&self.git(&source, &["branch", "--show-current"])?.stdout)?,
        };
        let head_sha = one_line(&self.git(&source, &["rev-parse", "HEAD"])?.stdout)?;
        let worktree_capable = self
            .git(&source, &["worktree", "list", "--porcelain"])
            .is_ok();
        Ok(RepositorySourceSnapshot {
            source_path: source.to_string_lossy().into_owned(),
            git_common_directory: common.to_string_lossy().into_owned(),
            remote_url,
            github_repository,
            default_branch,
            head_sha,
            worktree_capable,
        })
    }

    fn git(
        &self,
        current_dir: &Path,
        args: &[&str],
    ) -> Result<crate::diagnostics::CommandOutput, WorkspaceError> {
        let output = self.executor.execute(&CommandRequest {
            executable: self.git_executable.clone(),
            args: args.iter().map(|value| (*value).to_owned()).collect(),
            current_dir: Some(current_dir.to_path_buf()),
            timeout: GIT_TIMEOUT,
            output_limit: GIT_OUTPUT_LIMIT,
        })?;
        if !output.success {
            return Err(WorkspaceError::Git(DiagnosticAdapterError::MalformedOutput));
        }
        Ok(output)
    }

    fn validate_source_identity(&self, workspace: &WorkspaceRecord) -> Result<(), WorkspaceError> {
        let snapshot = self.inspect_repository_source(&workspace.repository_source_path)?;
        let expected_source = Path::new(&workspace.repository_source_path)
            .canonicalize()
            .map_err(|_| WorkspaceError::InvalidRepositorySource)?;
        let expected_common = Path::new(&workspace.git_common_directory)
            .canonicalize()
            .map_err(|_| WorkspaceError::InvalidRepositorySource)?;
        if Path::new(&snapshot.source_path) != expected_source
            || Path::new(&snapshot.git_common_directory) != expected_common
        {
            return Err(WorkspaceError::IdentityConflict);
        }
        let workspace_root = canonical_workspace_root(&workspace.workspace_root)?;
        if expected_source.starts_with(&workspace_root) {
            return Err(WorkspaceError::IdentityConflict);
        }
        self.validate_workspace_path(workspace)?;
        Ok(())
    }

    fn validate_workspace_path(&self, workspace: &WorkspaceRecord) -> Result<(), WorkspaceError> {
        let root = canonical_workspace_root(&workspace.workspace_root)?;
        let (kind, owner) = match workspace.kind {
            WorkspaceKind::Maker => (
                "maker",
                workspace
                    .root_run_id
                    .as_deref()
                    .ok_or(WorkspaceError::IdentityConflict)?,
            ),
            WorkspaceKind::Reviewer => (
                "reviewer",
                workspace
                    .review_cycle_id
                    .as_deref()
                    .ok_or(WorkspaceError::IdentityConflict)?,
            ),
        };
        if !safe_component(owner) {
            return Err(WorkspaceError::UnsafeIdentifier);
        }
        let expected = root.join(kind).join(owner);
        let path = Path::new(&workspace.path);
        if path != expected {
            return Err(WorkspaceError::PathEscape);
        }
        if path.exists() {
            let metadata = fs::symlink_metadata(path).map_err(|_| WorkspaceError::PathEscape)?;
            let canonical = path
                .canonicalize()
                .map_err(|_| WorkspaceError::PathEscape)?;
            if metadata.file_type().is_symlink()
                || canonical != expected
                || !canonical.starts_with(root)
            {
                return Err(WorkspaceError::PathEscape);
            }
        }
        Ok(())
    }

    async fn allocate(
        &self,
        workspace: WorkspaceRecord,
    ) -> Result<ExternalResult<WorkspaceRecord>, WorkspaceError> {
        self.validate_source_identity(&workspace)?;
        self.database
            .insert_workspace_intent(&workspace, unix_now())
            .await?;

        let source = Path::new(&workspace.repository_source_path);
        let path = PathBuf::from(&workspace.path);
        let allocation = match workspace.kind {
            WorkspaceKind::Maker => self.git(
                source,
                &[
                    "worktree",
                    "add",
                    "-b",
                    workspace
                        .branch
                        .as_deref()
                        .ok_or(WorkspaceError::IdentityConflict)?,
                    path.to_str().ok_or(WorkspaceError::PathEscape)?,
                    &workspace.base_sha,
                ],
            ),
            WorkspaceKind::Reviewer => self.git(
                source,
                &[
                    "worktree",
                    "add",
                    "--detach",
                    path.to_str().ok_or(WorkspaceError::PathEscape)?,
                    workspace
                        .head_sha
                        .as_deref()
                        .ok_or(WorkspaceError::IdentityConflict)?,
                ],
            ),
        };
        if allocation.is_err() {
            let _ = self
                .database
                .set_workspace_state(
                    &workspace.id,
                    WorkspaceAllocationState::Quarantined,
                    Some("Git worktree allocation failed after durable intent"),
                    unix_now(),
                )
                .await;
            return allocation.map(|_| ExternalResult::Confirmed(workspace));
        }
        let mut ready = workspace;
        ready.allocation_state = WorkspaceAllocationState::Ready;
        write_marker(&path, &ready)?;
        self.database
            .set_workspace_state(&ready.id, WorkspaceAllocationState::Ready, None, unix_now())
            .await?;
        Ok(ExternalResult::Confirmed(ready))
    }

    fn exact_evidence(&self, workspace: &WorkspaceRecord) -> Result<bool, WorkspaceError> {
        self.validate_source_identity(workspace)?;
        let path = Path::new(&workspace.path);
        if !path.is_dir() || !same_workspace_identity(&read_marker(path)?, workspace) {
            return Ok(false);
        }
        let listed = self.git(
            Path::new(&workspace.repository_source_path),
            &["worktree", "list", "--porcelain"],
        )?;
        Ok(worktree_list_contains(&listed.stdout, workspace))
    }
}

impl<E: CommandExecutor> WorkspacePort for GitWorkspaceAdapter<E> {
    type Error = WorkspaceError;

    async fn allocate_maker(
        &self,
        request: MakerWorkspaceRequest,
    ) -> Result<ExternalResult<WorkspaceRecord>, Self::Error> {
        if let Some(existing) = self
            .database
            .workspace_for_root_run(&request.root_run_id)
            .await?
        {
            return if existing.allocation_state == WorkspaceAllocationState::Ready
                && self.exact_evidence(&existing)?
            {
                Ok(ExternalResult::Confirmed(existing))
            } else {
                Ok(ExternalResult::Ambiguous {
                    detail: "maker workspace exists but ownership evidence is not ready".into(),
                })
            };
        }
        let branch = maker_branch(&request.linear_identifier, &request.root_run_id)?;
        let path = owned_path(&request.workspace_root, "maker", &request.root_run_id)?;
        let workspace_root = canonical_workspace_root(&request.workspace_root)?;
        self.allocate(WorkspaceRecord {
            id: request.workspace_id,
            work_item_id: request.work_item_id,
            run_id: Some(request.root_run_id.clone()),
            kind: WorkspaceKind::Maker,
            root_run_id: Some(request.root_run_id),
            review_cycle_id: None,
            path: path.to_string_lossy().into_owned(),
            workspace_root: workspace_root.to_string_lossy().into_owned(),
            repository_source_path: request.repository_source_path,
            git_common_directory: request.git_common_directory,
            base_sha: request.base_sha,
            head_sha: None,
            branch: Some(branch),
            allocation_state: WorkspaceAllocationState::Allocating,
        })
        .await
    }

    async fn allocate_reviewer(
        &self,
        request: ReviewerWorkspaceRequest,
    ) -> Result<ExternalResult<WorkspaceRecord>, Self::Error> {
        if let Some(existing) = self
            .database
            .workspace_for_review_cycle(&request.review_cycle_id)
            .await?
        {
            return if existing.head_sha.as_deref() == Some(request.head_sha.as_str())
                && existing.allocation_state == WorkspaceAllocationState::Ready
                && self.exact_evidence(&existing)?
            {
                Ok(ExternalResult::Confirmed(existing))
            } else {
                Ok(ExternalResult::Ambiguous {
                    detail: "review workspace identity does not match the requested SHA".into(),
                })
            };
        }
        let path = owned_path(
            &request.workspace_root,
            "reviewer",
            &request.review_cycle_id,
        )?;
        let workspace_root = canonical_workspace_root(&request.workspace_root)?;
        self.allocate(WorkspaceRecord {
            id: request.workspace_id,
            work_item_id: request.work_item_id,
            run_id: Some(request.run_id),
            kind: WorkspaceKind::Reviewer,
            root_run_id: None,
            review_cycle_id: Some(request.review_cycle_id),
            path: path.to_string_lossy().into_owned(),
            workspace_root: workspace_root.to_string_lossy().into_owned(),
            repository_source_path: request.repository_source_path,
            git_common_directory: request.git_common_directory,
            base_sha: request.base_sha,
            head_sha: Some(request.head_sha),
            branch: None,
            allocation_state: WorkspaceAllocationState::Allocating,
        })
        .await
    }

    async fn verify_reviewer_clean(
        &self,
        workspace_id: &str,
    ) -> Result<ExternalResult<bool>, Self::Error> {
        let Some(workspace) = self.database.workspace(workspace_id).await? else {
            return Ok(ExternalResult::NotFound);
        };
        if workspace.kind != WorkspaceKind::Reviewer || !self.exact_evidence(&workspace)? {
            return Ok(ExternalResult::Ambiguous {
                detail: "review workspace ownership evidence is ambiguous".into(),
            });
        }
        let status = self.git(
            Path::new(&workspace.path),
            &[
                "status",
                "--porcelain=v1",
                "--untracked-files=all",
                "--",
                ".",
                ":(exclude).spire-owner",
            ],
        )?;
        Ok(ExternalResult::Confirmed(status.stdout.is_empty()))
    }

    async fn recover_allocations(&self) -> Result<WorkspaceRecoverySummary, Self::Error> {
        let mut summary = WorkspaceRecoverySummary {
            adopted: 0,
            quarantined: 0,
        };
        for workspace in self.database.allocating_workspaces().await? {
            if self.exact_evidence(&workspace).unwrap_or(false) {
                self.database
                    .set_workspace_state(
                        &workspace.id,
                        WorkspaceAllocationState::Ready,
                        None,
                        unix_now(),
                    )
                    .await?;
                summary.adopted += 1;
            } else {
                self.database
                    .set_workspace_state(
                        &workspace.id,
                        WorkspaceAllocationState::Quarantined,
                        Some("allocation recovery evidence did not exactly agree"),
                        unix_now(),
                    )
                    .await?;
                summary.quarantined += 1;
            }
        }
        Ok(summary)
    }

    async fn cleanup(&self, workspace_id: &str) -> Result<ExternalResult<()>, Self::Error> {
        let Some(workspace) = self.database.workspace(workspace_id).await? else {
            return Ok(ExternalResult::NotFound);
        };
        self.validate_source_identity(&workspace)?;
        let now = unix_now();
        let terminal_before = now.saturating_sub(self.terminal_retention_seconds as i64);
        if !self
            .database
            .workspace_cleanup_authorized(workspace_id, terminal_before, now)
            .await?
        {
            return Err(WorkspaceError::CleanupNotAuthorized);
        }
        if workspace.allocation_state == WorkspaceAllocationState::Removing
            && !Path::new(&workspace.path).exists()
        {
            self.prune_worktree_metadata(&workspace)?;
            self.database
                .set_workspace_state(
                    workspace_id,
                    WorkspaceAllocationState::Removed,
                    None,
                    unix_now(),
                )
                .await?;
            return Ok(ExternalResult::Confirmed(()));
        }
        if !self.exact_evidence(&workspace)? {
            return Err(WorkspaceError::OwnershipMismatch);
        }
        self.database
            .set_workspace_state(
                workspace_id,
                WorkspaceAllocationState::Removing,
                None,
                unix_now(),
            )
            .await?;
        self.git(
            Path::new(&workspace.repository_source_path),
            &[
                "worktree",
                "remove",
                "--force",
                "--",
                Path::new(&workspace.path)
                    .to_str()
                    .ok_or(WorkspaceError::PathEscape)?,
            ],
        )?;
        self.prune_worktree_metadata(&workspace)?;
        self.database
            .set_workspace_state(
                workspace_id,
                WorkspaceAllocationState::Removed,
                None,
                unix_now(),
            )
            .await?;
        Ok(ExternalResult::Confirmed(()))
    }
}

impl<E: CommandExecutor> GitWorkspaceAdapter<E> {
    fn prune_worktree_metadata(&self, workspace: &WorkspaceRecord) -> Result<(), WorkspaceError> {
        self.git(
            Path::new(&workspace.repository_source_path),
            &["worktree", "prune", "--expire", "now"],
        )?;
        Ok(())
    }
}

fn one_line(value: &str) -> Result<String, WorkspaceError> {
    let value = value.trim();
    if value.is_empty() || value.contains(['\r', '\n', '\0']) || value.len() > 4096 {
        return Err(WorkspaceError::InvalidRepositorySource);
    }
    Ok(value.to_owned())
}

fn resolve_git_path(source: &Path, value: &str) -> Result<PathBuf, WorkspaceError> {
    let path = Path::new(value);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        source.join(path)
    };
    path.canonicalize()
        .map_err(|_| WorkspaceError::InvalidRepositorySource)
}

fn maker_branch(identifier: &str, root_run_id: &str) -> Result<String, WorkspaceError> {
    let identifier = identifier
        .bytes()
        .filter(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
        .map(|byte| byte.to_ascii_lowercase() as char)
        .collect::<String>();
    let run_short = root_run_id
        .bytes()
        .filter(|byte| byte.is_ascii_alphanumeric())
        .take(8)
        .map(char::from)
        .collect::<String>();
    if identifier.is_empty() || identifier.len() > 64 || run_short.len() < 6 {
        return Err(WorkspaceError::UnsafeIdentifier);
    }
    Ok(format!("spire/{identifier}-{run_short}"))
}

fn owned_path(root: &str, kind: &str, owner: &str) -> Result<PathBuf, WorkspaceError> {
    if !safe_component(owner) {
        return Err(WorkspaceError::UnsafeIdentifier);
    }
    let root = canonical_workspace_root(root)?;
    let parent = root.join(kind);
    fs::create_dir_all(&parent).map_err(|_| WorkspaceError::Io)?;
    let parent = parent.canonicalize().map_err(|_| WorkspaceError::Io)?;
    if !parent.starts_with(&root) {
        return Err(WorkspaceError::PathEscape);
    }
    Ok(parent.join(owner))
}

fn canonical_workspace_root(root: &str) -> Result<PathBuf, WorkspaceError> {
    let root = Path::new(root);
    if !root.is_absolute() || !root.is_dir() {
        return Err(WorkspaceError::RootUnavailable);
    }
    root.canonicalize()
        .map_err(|_| WorkspaceError::RootUnavailable)
}

fn same_workspace_identity(left: &WorkspaceRecord, right: &WorkspaceRecord) -> bool {
    left.id == right.id
        && left.work_item_id == right.work_item_id
        && left.run_id == right.run_id
        && left.kind == right.kind
        && left.root_run_id == right.root_run_id
        && left.review_cycle_id == right.review_cycle_id
        && left.path == right.path
        && left.workspace_root == right.workspace_root
        && left.repository_source_path == right.repository_source_path
        && left.git_common_directory == right.git_common_directory
        && left.base_sha == right.base_sha
        && left.head_sha == right.head_sha
        && left.branch == right.branch
}

fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn write_marker(path: &Path, workspace: &WorkspaceRecord) -> Result<(), WorkspaceError> {
    let marker = serde_json::to_vec(&OwnershipMarker {
        version: 1,
        workspace: workspace.clone(),
    })
    .map_err(|_| WorkspaceError::Io)?;
    let temporary = path.join(format!("{MARKER_NAME}.tmp-{}", std::process::id()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|_| WorkspaceError::Io)?;
    file.write_all(&marker).map_err(|_| WorkspaceError::Io)?;
    file.sync_all().map_err(|_| WorkspaceError::Io)?;
    fs::rename(&temporary, path.join(MARKER_NAME)).map_err(|_| WorkspaceError::Io)?;
    OpenOptions::new()
        .read(true)
        .open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| WorkspaceError::Io)
}

fn read_marker(path: &Path) -> Result<WorkspaceRecord, WorkspaceError> {
    let marker = fs::symlink_metadata(path.join(MARKER_NAME))
        .map_err(|_| WorkspaceError::OwnershipMismatch)?;
    if marker.file_type().is_symlink() || !marker.is_file() || marker.len() > 64 * 1024 {
        return Err(WorkspaceError::OwnershipMismatch);
    }
    let marker = fs::read(path.join(MARKER_NAME)).map_err(|_| WorkspaceError::OwnershipMismatch)?;
    let marker: OwnershipMarker =
        serde_json::from_slice(&marker).map_err(|_| WorkspaceError::OwnershipMismatch)?;
    if marker.version != 1 {
        return Err(WorkspaceError::OwnershipMismatch);
    }
    Ok(marker.workspace)
}

fn worktree_list_contains(output: &str, workspace: &WorkspaceRecord) -> bool {
    output.split("\n\n").any(|entry| {
        let mut path = None;
        let mut head = None;
        let mut branch = None;
        let mut detached = false;
        for line in entry.lines() {
            if let Some(value) = line.strip_prefix("worktree ") {
                path = Some(value);
            } else if let Some(value) = line.strip_prefix("HEAD ") {
                head = Some(value);
            } else if let Some(value) = line.strip_prefix("branch refs/heads/") {
                branch = Some(value);
            } else if line == "detached" {
                detached = true;
            }
        }
        path == Some(workspace.path.as_str())
            && match workspace.kind {
                WorkspaceKind::Maker => branch == workspace.branch.as_deref(),
                WorkspaceKind::Reviewer => detached && head == workspace.head_sha.as_deref(),
            }
    })
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use std::{
        process::Command,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use spire_application::{MakerWorkspaceRequest, ReviewerWorkspaceRequest, WorkspacePort};

    use super::*;
    use crate::diagnostics::SystemCommandExecutor;

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    async fn fixture() -> (PathBuf, PathBuf, SqliteDatabase, String) {
        let root = std::env::temp_dir().join(format!(
            "spire-git-worktree-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let source = root.join("source");
        let workspaces = root.join("workspaces");
        let data = root.join("data");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&workspaces).unwrap();
        fs::create_dir_all(&data).unwrap();
        run(&source, &["init", "-b", "main"]);
        run(&source, &["config", "user.email", "fixture@example.test"]);
        run(&source, &["config", "user.name", "Fixture"]);
        fs::write(source.join("tracked.txt"), "base\n").unwrap();
        run(&source, &["add", "tracked.txt"]);
        run(&source, &["commit", "-m", "fixture"]);
        run(
            &source,
            &["remote", "add", "origin", "git@github.com:owner/repo.git"],
        );
        let head = output(&source, &["rev-parse", "HEAD"]);

        let database = SqliteDatabase::initialize(data.join("spire.db"), 4)
            .await
            .unwrap();
        sqlx::query("INSERT INTO work_items (id, state, revision, created_at, updated_at) VALUES ('workitem123', 'implementing', 'revision', 0, 0)")
            .execute(database.pool())
            .await
            .unwrap();
        sqlx::query("INSERT INTO runs (id, work_item_id, root_run_id, role, harness, model, effort, status, created_at, updated_at) VALUES ('rootrun123', 'workitem123', 'rootrun123', 'implementation', 'codex', 'model', 'medium', 'running', 0, 0)")
            .execute(database.pool())
            .await
            .unwrap();
        (source, workspaces, database, head)
    }

    #[tokio::test]
    async fn maker_and_reviewer_worktrees_are_isolated_reused_and_git_cleaned() {
        let (source, root, database, head) = fixture().await;
        let adapter = GitWorkspaceAdapter::new(database.clone(), SystemCommandExecutor, "git", 0);
        let snapshot = adapter.inspect_repository_source(&source).unwrap();
        let source_head = output(&source, &["rev-parse", "HEAD"]);
        let source_status = output(&source, &["status", "--porcelain=v1"]);

        let maker = match adapter
            .allocate_maker(MakerWorkspaceRequest {
                workspace_id: "makerworkspace123".into(),
                work_item_id: "workitem123".into(),
                linear_identifier: "SPI-14".into(),
                root_run_id: "rootrun123".into(),
                repository_source_path: snapshot.source_path.clone(),
                git_common_directory: snapshot.git_common_directory.clone(),
                base_sha: head.clone(),
                workspace_root: root.to_string_lossy().into_owned(),
            })
            .await
            .unwrap()
        {
            ExternalResult::Confirmed(workspace) => workspace,
            result => panic!("unexpected maker allocation: {result:?}"),
        };
        assert_eq!(maker.branch.as_deref(), Some("spire/spi-14-rootrun1"));
        let reused = adapter
            .allocate_maker(MakerWorkspaceRequest {
                workspace_id: "ignoredworkspace".into(),
                work_item_id: "workitem123".into(),
                linear_identifier: "HOSTILE-title-is-ignored".into(),
                root_run_id: "rootrun123".into(),
                repository_source_path: snapshot.source_path.clone(),
                git_common_directory: snapshot.git_common_directory.clone(),
                base_sha: head.clone(),
                workspace_root: root.to_string_lossy().into_owned(),
            })
            .await
            .unwrap();
        assert!(matches!(
            reused,
            ExternalResult::Confirmed(ref workspace) if workspace.id == maker.id
        ));
        assert_eq!(output(&source, &["rev-parse", "HEAD"]), source_head);
        assert_eq!(
            output(&source, &["status", "--porcelain=v1"]),
            source_status
        );

        sqlx::query("UPDATE runs SET status = 'succeeded' WHERE id = 'rootrun123'")
            .execute(database.pool())
            .await
            .unwrap();
        sqlx::query("INSERT INTO runs (id, work_item_id, root_run_id, role, harness, model, effort, status, created_at, updated_at) VALUES ('reviewrun123', 'workitem123', 'reviewrun123', 'review', 'claude-code', 'model', 'medium', 'running', 1, 1)")
            .execute(database.pool())
            .await
            .unwrap();
        sqlx::query("INSERT INTO review_cycles (id, work_item_id, head_sha, ci_state, review_state, review_run_id, base_sha, created_at, updated_at) VALUES ('reviewcycle123', 'workitem123', ?, 'succeeded', 'running', 'reviewrun123', ?, 1, 1)")
            .bind(&head)
            .bind(&head)
            .execute(database.pool())
            .await
            .unwrap();
        let reviewer = match adapter
            .allocate_reviewer(ReviewerWorkspaceRequest {
                workspace_id: "reviewworkspace123".into(),
                work_item_id: "workitem123".into(),
                run_id: "reviewrun123".into(),
                review_cycle_id: "reviewcycle123".into(),
                repository_source_path: snapshot.source_path,
                git_common_directory: snapshot.git_common_directory,
                base_sha: head.clone(),
                head_sha: head,
                workspace_root: root.to_string_lossy().into_owned(),
            })
            .await
            .unwrap()
        {
            ExternalResult::Confirmed(workspace) => workspace,
            result => panic!("unexpected reviewer allocation: {result:?}"),
        };
        assert!(reviewer.branch.is_none());
        assert!(matches!(
            adapter
                .verify_reviewer_clean("reviewworkspace123")
                .await
                .unwrap(),
            ExternalResult::Confirmed(true)
        ));
        fs::write(Path::new(&reviewer.path).join("mutation"), "changed").unwrap();
        assert!(matches!(
            adapter
                .verify_reviewer_clean("reviewworkspace123")
                .await
                .unwrap(),
            ExternalResult::Confirmed(false)
        ));
        fs::remove_file(Path::new(&reviewer.path).join("mutation")).unwrap();

        sqlx::query("UPDATE runs SET status = 'succeeded' WHERE id = 'reviewrun123'")
            .execute(database.pool())
            .await
            .unwrap();
        sqlx::query(
            "UPDATE review_cycles SET review_state = 'approved' WHERE id = 'reviewcycle123'",
        )
        .execute(database.pool())
        .await
        .unwrap();
        sqlx::query(
            "UPDATE work_items SET state = 'completed', updated_at = 0 WHERE id = 'workitem123'",
        )
        .execute(database.pool())
        .await
        .unwrap();
        sqlx::query(
            "UPDATE runs SET lease_owner = 'worker', lease_expires_at = ? WHERE id = 'reviewrun123'",
        )
        .bind(i64::MAX)
        .execute(database.pool())
        .await
        .unwrap();
        assert!(matches!(
            adapter.cleanup("reviewworkspace123").await,
            Err(WorkspaceError::CleanupNotAuthorized)
        ));
        sqlx::query(
            "UPDATE runs SET lease_owner = NULL, lease_expires_at = NULL WHERE id = 'reviewrun123'",
        )
        .execute(database.pool())
        .await
        .unwrap();
        assert!(matches!(
            adapter.cleanup("reviewworkspace123").await.unwrap(),
            ExternalResult::Confirmed(())
        ));
        database
            .set_workspace_state(
                "makerworkspace123",
                WorkspaceAllocationState::Removing,
                None,
                unix_now(),
            )
            .await
            .unwrap();
        assert!(matches!(
            adapter.cleanup("makerworkspace123").await.unwrap(),
            ExternalResult::Confirmed(())
        ));
        assert!(!Path::new(&reviewer.path).exists());
        assert!(!Path::new(&maker.path).exists());
        assert!(
            output(&source, &["branch", "--list", "spire/spi-14-rootrun1"])
                .contains("spire/spi-14-rootrun1")
        );
    }

    #[tokio::test]
    async fn recovery_quarantines_git_success_without_an_exact_marker() {
        let (source, root, database, head) = fixture().await;
        let adapter = GitWorkspaceAdapter::new(database.clone(), SystemCommandExecutor, "git", 0);
        let snapshot = adapter.inspect_repository_source(&source).unwrap();
        let canonical_root = root.canonicalize().unwrap();
        let path = canonical_root.join("maker").join("rootrun123");
        let workspace = WorkspaceRecord {
            id: "gitonlyworkspace".into(),
            work_item_id: "workitem123".into(),
            run_id: Some("rootrun123".into()),
            kind: WorkspaceKind::Maker,
            root_run_id: Some("rootrun123".into()),
            review_cycle_id: None,
            path: path.to_string_lossy().into_owned(),
            workspace_root: canonical_root.to_string_lossy().into_owned(),
            repository_source_path: snapshot.source_path,
            git_common_directory: snapshot.git_common_directory,
            base_sha: head,
            head_sha: None,
            branch: Some("spire/spi-14-rootrun1".into()),
            allocation_state: WorkspaceAllocationState::Allocating,
        };
        database
            .insert_workspace_intent(&workspace, 1)
            .await
            .unwrap();
        run(
            &source,
            &[
                "worktree",
                "add",
                "-b",
                "spire/spi-14-rootrun1",
                path.to_str().unwrap(),
                &workspace.base_sha,
            ],
        );

        let summary = adapter.recover_allocations().await.unwrap();

        assert_eq!(summary.adopted, 0);
        assert_eq!(summary.quarantined, 1);
        assert_eq!(
            database
                .workspace("gitonlyworkspace")
                .await
                .unwrap()
                .unwrap()
                .allocation_state,
            WorkspaceAllocationState::Quarantined
        );
    }

    #[tokio::test]
    async fn cleanup_recovers_after_git_removal_before_terminal_persistence() {
        let (source, root, database, head) = fixture().await;
        let adapter = GitWorkspaceAdapter::new(database.clone(), SystemCommandExecutor, "git", 0);
        let snapshot = adapter.inspect_repository_source(&source).unwrap();
        let workspace = match adapter
            .allocate_maker(MakerWorkspaceRequest {
                workspace_id: "makerworkspace123".into(),
                work_item_id: "workitem123".into(),
                linear_identifier: "SPI-14".into(),
                root_run_id: "rootrun123".into(),
                repository_source_path: snapshot.source_path,
                git_common_directory: snapshot.git_common_directory,
                base_sha: head,
                workspace_root: root.to_string_lossy().into_owned(),
            })
            .await
            .unwrap()
        {
            ExternalResult::Confirmed(workspace) => workspace,
            result => panic!("unexpected maker allocation: {result:?}"),
        };
        sqlx::query("UPDATE runs SET status = 'succeeded' WHERE id = 'rootrun123'")
            .execute(database.pool())
            .await
            .unwrap();
        sqlx::query(
            "UPDATE work_items SET state = 'completed', updated_at = 0 WHERE id = 'workitem123'",
        )
        .execute(database.pool())
        .await
        .unwrap();
        database
            .set_workspace_state(
                &workspace.id,
                WorkspaceAllocationState::Removing,
                None,
                unix_now(),
            )
            .await
            .unwrap();
        run(
            &source,
            &[
                "worktree",
                "remove",
                "--force",
                "--",
                workspace.path.as_str(),
            ],
        );

        assert!(matches!(
            adapter.cleanup(&workspace.id).await.unwrap(),
            ExternalResult::Confirmed(())
        ));
        assert_eq!(
            database
                .workspace(&workspace.id)
                .await
                .unwrap()
                .unwrap()
                .allocation_state,
            WorkspaceAllocationState::Removed
        );
    }

    #[tokio::test]
    async fn recovery_adopts_only_exact_marker_and_git_evidence() {
        let (source, root, database, head) = fixture().await;
        let adapter = GitWorkspaceAdapter::new(database.clone(), SystemCommandExecutor, "git", 0);
        let snapshot = adapter.inspect_repository_source(&source).unwrap();
        let canonical_root = root.canonicalize().unwrap();
        let incomplete = WorkspaceRecord {
            id: "incompleteworkspace".into(),
            work_item_id: "workitem123".into(),
            run_id: Some("rootrun123".into()),
            kind: WorkspaceKind::Maker,
            root_run_id: Some("rootrun123".into()),
            review_cycle_id: None,
            path: canonical_root
                .join("maker")
                .join("rootrun123")
                .to_string_lossy()
                .into_owned(),
            workspace_root: canonical_root.to_string_lossy().into_owned(),
            repository_source_path: snapshot.source_path,
            git_common_directory: snapshot.git_common_directory,
            base_sha: head,
            head_sha: None,
            branch: Some("spire/spi-14-rootrun1".into()),
            allocation_state: WorkspaceAllocationState::Allocating,
        };
        database
            .insert_workspace_intent(&incomplete, 1)
            .await
            .unwrap();
        let summary = adapter.recover_allocations().await.unwrap();
        assert_eq!(summary.quarantined, 1);
        assert_eq!(
            database
                .workspace("incompleteworkspace")
                .await
                .unwrap()
                .unwrap()
                .allocation_state,
            WorkspaceAllocationState::Quarantined
        );

        sqlx::query("UPDATE runs SET status = 'succeeded' WHERE id = 'rootrun123'")
            .execute(database.pool())
            .await
            .unwrap();
        sqlx::query("INSERT INTO runs (id, work_item_id, root_run_id, role, harness, model, effort, status, created_at, updated_at) VALUES ('rootrun456', 'workitem123', 'rootrun456', 'implementation', 'codex', 'model', 'medium', 'running', 2, 2)")
            .execute(database.pool())
            .await
            .unwrap();
        let exact_path = canonical_root.join("maker").join("rootrun456");
        let mut exact = WorkspaceRecord {
            id: "exactworkspace".into(),
            work_item_id: "workitem123".into(),
            run_id: Some("rootrun456".into()),
            kind: WorkspaceKind::Maker,
            root_run_id: Some("rootrun456".into()),
            review_cycle_id: None,
            path: exact_path.to_string_lossy().into_owned(),
            workspace_root: canonical_root.to_string_lossy().into_owned(),
            repository_source_path: incomplete.repository_source_path,
            git_common_directory: incomplete.git_common_directory,
            base_sha: incomplete.base_sha,
            head_sha: None,
            branch: Some("spire/spi-14-rootrun4".into()),
            allocation_state: WorkspaceAllocationState::Allocating,
        };
        database.insert_workspace_intent(&exact, 2).await.unwrap();
        run(
            &source,
            &[
                "worktree",
                "add",
                "-b",
                "spire/spi-14-rootrun4",
                exact_path.to_str().unwrap(),
                &exact.base_sha,
            ],
        );
        exact.allocation_state = WorkspaceAllocationState::Ready;
        write_marker(&exact_path, &exact).unwrap();
        let summary = adapter.recover_allocations().await.unwrap();
        assert_eq!(summary.adopted, 1);
        assert_eq!(
            database
                .workspace("exactworkspace")
                .await
                .unwrap()
                .unwrap()
                .allocation_state,
            WorkspaceAllocationState::Ready
        );
    }

    fn run(repository: &Path, args: &[&str]) {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(repository)
                .status()
                .unwrap()
                .success(),
            "git {args:?}"
        );
    }

    fn output(repository: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(repository)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?}");
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }
}
