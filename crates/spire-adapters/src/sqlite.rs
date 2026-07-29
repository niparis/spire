use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use spire_application::{
    CanonicalPullRequest, CapacityCounts, CapacityLimits, CheckRun, NewProjectRepositoryMapping,
    OperationsSnapshot, ProjectMappingPort, ProjectRepositoryMapping, ProjectRoutingDecision,
    PullRequestState, RequiredCheckGate, ReviewResult, ReviewVerdict, SchedulerInitiator,
    capacity_allows,
};
use spire_domain::{
    LinearProjectId, ProjectMappingRevision, ProjectMappingStatus, ProjectRepositoryMappingId,
    RepositoryName,
};
use sqlx::{
    Row, Sqlite, SqlitePool,
    pool::PoolConnection,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use thiserror::Error;
use tokio::sync::Mutex;
use uuid::Uuid;

const BUSY_TIMEOUT_MILLIS: u64 = 5_000;
const MAX_WRITE_ATTEMPTS: u8 = 3;
const MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

async fn begin_immediate(connection: &mut PoolConnection<Sqlite>) -> Result<(), sqlx::Error> {
    for attempt in 0..MAX_WRITE_ATTEMPTS {
        match sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut **connection)
            .await
        {
            Ok(_) => return Ok(()),
            Err(error) if is_busy(&error) && attempt + 1 < MAX_WRITE_ATTEMPTS => {
                tokio::time::sleep(Duration::from_millis(10 * u64::from(attempt + 1))).await;
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("the final write-attempt returns")
}

async fn commit_result<T>(
    connection: &mut PoolConnection<Sqlite>,
    result: Result<T, sqlx::Error>,
) -> Result<T, SqliteAdapterError> {
    match result {
        Ok(value) => {
            sqlx::query("COMMIT").execute(&mut **connection).await?;
            Ok(value)
        }
        Err(error) => {
            let _ = sqlx::query("ROLLBACK").execute(&mut **connection).await;
            Err(error.into())
        }
    }
}

async fn capacity_counts_for(
    connection: &mut PoolConnection<Sqlite>,
    work_item_id: &str,
    repository: &str,
) -> Result<CapacityCounts, sqlx::Error> {
    Ok(CapacityCounts {
        total: sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM runs WHERE status IN ('starting', 'running')",
        )
        .fetch_one(&mut **connection)
        .await? as u16,
        ai: sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM runs WHERE status IN ('starting', 'running') AND initiator = 'ai'",
        )
        .fetch_one(&mut **connection)
        .await? as u16,
        repository: sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM runs r JOIN work_items w ON w.id = r.work_item_id WHERE r.status IN ('starting', 'running') AND w.repository = ?",
        )
        .bind(repository)
        .fetch_one(&mut **connection)
        .await? as u16,
        ticket: sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM runs WHERE status IN ('starting', 'running') AND work_item_id = ?",
        )
        .bind(work_item_id)
        .fetch_one(&mut **connection)
        .await? as u16,
    })
}

fn is_busy(error: &sqlx::Error) -> bool {
    error.as_database_error().is_some_and(|database| {
        matches!(database.code().as_deref(), Some("5") | Some("SQLITE_BUSY"))
    })
}

fn initiator_name(initiator: SchedulerInitiator) -> &'static str {
    match initiator {
        SchedulerInitiator::Human => "human",
        SchedulerInitiator::Ai => "ai",
        SchedulerInitiator::System => "system",
    }
}

fn effort_name(effort: spire_domain::Effort) -> &'static str {
    match effort {
        spire_domain::Effort::Low => "low",
        spire_domain::Effort::Medium => "medium",
        spire_domain::Effort::High => "high",
    }
}

#[derive(Debug, Error)]
pub enum SqliteAdapterError {
    #[error("database path must be absolute: {0}")]
    RelativePath(PathBuf),
    #[error("database parent directory is unavailable: {0}")]
    MissingParent(PathBuf),
    #[error("backup path must not equal the live database path")]
    BackupWouldOverwriteLiveDatabase,
    #[error("database integrity check failed: {0}")]
    IntegrityCheckFailed(String),
    #[error("SQLite error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("review result contract is invalid: {0}")]
    ReviewContract(String),
    #[error("migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("project mapping data is invalid: {0}")]
    ProjectMappingContract(String),
}

#[derive(Debug, Clone)]
pub struct SqliteDatabase {
    pool: SqlitePool,
    database_path: PathBuf,
    write_gate: Arc<Mutex<()>>,
}

#[derive(Debug, Clone)]
pub struct InboxEvent<'a> {
    pub id: &'a str,
    pub source: &'a str,
    pub delivery_id: &'a str,
    pub event_type: &'a str,
    pub raw_headers: &'a str,
    pub raw_body: &'a [u8],
}

#[derive(Debug, Clone)]
pub struct OutboxAction<'a> {
    pub id: &'a str,
    pub kind: &'a str,
    pub aggregate_id: &'a str,
    pub idempotency_key: &'a str,
    pub payload_json: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeasedOutboxAction {
    pub id: String,
    pub idempotency_key: String,
    pub attempts: i64,
}

#[derive(Debug, Clone)]
pub struct RunRecord<'a> {
    pub id: &'a str,
    pub work_item_id: &'a str,
    pub parent_run_id: Option<&'a str>,
    pub root_run_id: &'a str,
    pub role: &'a str,
    pub harness: &'a str,
    pub model: &'a str,
    pub effort: &'a str,
    pub status: &'a str,
}

#[derive(Debug, Clone)]
pub struct LinearObservation<'a> {
    pub work_item_id: &'a str,
    pub linear_issue_id: &'a str,
    pub linear_identifier: &'a str,
    pub team_id: &'a str,
    pub workflow_state_id: &'a str,
    pub revision: &'a str,
    pub raw_estimate: Option<u8>,
    pub complexity_class: Option<&'a str>,
    pub eligibility_reason: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct RootClaim<'a> {
    pub work_item_id: &'a str,
    pub expected_revision: &'a str,
    pub repository: &'a str,
    pub run_id: &'a str,
    pub decision_id: &'a str,
    pub raw_estimate: u8,
    pub complexity_class: &'a str,
    pub implementation_rule_id: &'a str,
    pub review_rule_id: &'a str,
    pub policy_version: u32,
    pub candidate_json: &'a str,
    pub evaluation_json: &'a str,
    pub review_candidate_json: &'a str,
    pub harness: &'a str,
    pub model: &'a str,
    pub effort: &'a str,
    pub initiator: SchedulerInitiator,
    pub trigger_kind: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootClaimResult {
    Claimed,
    RevisionChanged,
    AlreadyOwned,
    CapacityBlocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullRequestPersistence {
    Updated,
    IgnoredRepositoryOrBranch,
    StaleWorkItem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CiGatePersistence {
    Applied,
    StaleHead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CiCorrectionPersistence {
    Scheduled { cycle: u8 },
    CapacityBlocked,
    Exhausted,
    NoStickyMaker,
    StaleWorkItem,
}

#[derive(Debug, Clone)]
pub struct ReviewDispatch<'a> {
    pub work_item_id: &'a str,
    pub run_id: &'a str,
    pub review_cycle_id: &'a str,
    pub decision_id: &'a str,
    pub evaluation_json: &'a str,
    pub selected_candidate_index: usize,
    pub harness: &'a str,
    pub model: &'a str,
    pub effort: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewDispatchPersistence {
    Scheduled { round: u8 },
    AlreadyScheduled,
    CapacityBlocked,
    SameAsMaker,
    MissingStickyMaker,
    CandidateNotSnapshotted,
    StaleWorkItem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewResultPersistence {
    Applied { verdict: ReviewVerdict },
    StaleHead,
    InvalidCiGate,
    StaleWorkItem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewCorrectionPersistence {
    Scheduled { cycle: u8 },
    CapacityBlocked,
    Exhausted,
    MissingStickyMaker,
    StaleWorkItem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewWaiverPersistence {
    Applied,
    Unauthorized,
    StaleHead,
    InvalidCiGate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewPublicationPersistence {
    Applied,
    StaleHead,
}

fn project_mapping_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<ProjectRepositoryMapping, SqliteAdapterError> {
    let status = match row.try_get::<String, _>("status")?.as_str() {
        "enabled" => ProjectMappingStatus::Enabled,
        "disabled" => ProjectMappingStatus::Disabled,
        "removed" => ProjectMappingStatus::Removed,
        value => {
            return Err(SqliteAdapterError::ProjectMappingContract(format!(
                "unknown mapping status {value}"
            )));
        }
    };
    Ok(ProjectRepositoryMapping {
        id: ProjectRepositoryMappingId::parse(&row.try_get::<String, _>("id")?)
            .map_err(|error| SqliteAdapterError::ProjectMappingContract(error.to_string()))?,
        linear_organization_id: row.try_get("linear_organization_id")?,
        linear_team_id: row.try_get("linear_team_id")?,
        linear_project_id: LinearProjectId::new(row.try_get::<String, _>("linear_project_id")?)
            .map_err(|error| SqliteAdapterError::ProjectMappingContract(error.to_string()))?,
        linear_project_name_snapshot: row.try_get("linear_project_name_snapshot")?,
        github_repository: RepositoryName::new(row.try_get::<String, _>("github_repository")?)
            .map_err(|error| SqliteAdapterError::ProjectMappingContract(error.to_string()))?,
        repository_source_path: row.try_get("repository_source_path")?,
        git_common_directory: row.try_get("git_common_directory")?,
        git_remote_url: row.try_get("git_remote_url")?,
        default_branch: row.try_get("default_branch")?,
        status,
        revision: ProjectMappingRevision::new(row.try_get::<i64, _>("revision")? as u64)
            .map_err(|error| SqliteAdapterError::ProjectMappingContract(error.to_string()))?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn mapping_status_name(status: ProjectMappingStatus) -> &'static str {
    match status {
        ProjectMappingStatus::Enabled => "enabled",
        ProjectMappingStatus::Disabled => "disabled",
        ProjectMappingStatus::Removed => "removed",
    }
}

fn mapping_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

impl SqliteDatabase {
    pub async fn initialize(
        path: impl Into<PathBuf>,
        max_connections: u32,
    ) -> Result<Self, SqliteAdapterError> {
        let database_path = path.into();
        if !database_path.is_absolute() {
            return Err(SqliteAdapterError::RelativePath(database_path));
        }
        let parent = database_path
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| SqliteAdapterError::MissingParent(database_path.clone()))?;
        if !parent.is_dir() {
            return Err(SqliteAdapterError::MissingParent(parent));
        }
        let options = SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full)
            .busy_timeout(Duration::from_millis(BUSY_TIMEOUT_MILLIS));
        let pool = SqlitePoolOptions::new()
            .max_connections(max_connections.max(1))
            .after_connect(|connection, _| {
                Box::pin(async move {
                    sqlx::query("PRAGMA foreign_keys = ON")
                        .execute(&mut *connection)
                        .await?;
                    sqlx::query("PRAGMA busy_timeout = 5000")
                        .execute(&mut *connection)
                        .await?;
                    sqlx::query("PRAGMA synchronous = FULL")
                        .execute(&mut *connection)
                        .await?;
                    Ok(())
                })
            })
            .connect_with(options)
            .await?;
        sqlx::query("PRAGMA journal_mode = WAL")
            .execute(&pool)
            .await?;
        MIGRATOR.run(&pool).await?;
        let database = Self {
            pool,
            database_path,
            write_gate: Arc::new(Mutex::new(())),
        };
        database.check_integrity().await?;
        Ok(database)
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub fn path(&self) -> &Path {
        &self.database_path
    }

    pub async fn capacity_counts(&self) -> Result<(u64, u64), SqliteAdapterError> {
        let total = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM runs WHERE status IN ('starting', 'running')",
        )
        .fetch_one(&self.pool)
        .await?;
        let ai = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM runs WHERE status IN ('starting', 'running') AND initiator = 'ai'",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok((total as u64, ai as u64))
    }

    /// A bounded, non-sensitive operational snapshot for the loopback admin
    /// surface. It is intentionally aggregate-only: ticket text and provider
    /// credentials never appear in operational telemetry.
    pub async fn operations_snapshot(&self) -> Result<OperationsSnapshot, SqliteAdapterError> {
        Ok(OperationsSnapshot {
            inbox_depth: sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM webhook_inbox WHERE status IN ('pending', 'processing')",
            )
            .fetch_one(&self.pool)
            .await? as u64,
            outbox_depth: sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM outbox WHERE status IN ('pending', 'leased')",
            )
            .fetch_one(&self.pool)
            .await? as u64,
            active_runs: sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM runs WHERE status IN ('starting', 'running', 'cancel_requested')",
            )
            .fetch_one(&self.pool)
            .await? as u64,
            active_ai_runs: sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM runs WHERE status IN ('starting', 'running', 'cancel_requested') AND initiator = 'ai'",
            )
            .fetch_one(&self.pool)
            .await? as u64,
            terminal_workspace_cleanup_backlog: sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM workspaces ws JOIN work_items wi ON wi.id = ws.work_item_id WHERE wi.state IN ('blocked', 'completed', 'canceled') AND ws.cleanup_completed_at IS NULL",
            )
            .fetch_one(&self.pool)
            .await? as u64,
        })
    }

    pub async fn pragma(&self, name: &str) -> Result<String, SqliteAdapterError> {
        let query = format!("PRAGMA {name}");
        let row = sqlx::query(&query).fetch_one(&self.pool).await?;
        Ok(row
            .try_get::<String, _>(0)
            .or_else(|_| row.try_get::<i64, _>(0).map(|value| value.to_string()))?)
    }

    pub async fn check_integrity(&self) -> Result<(), SqliteAdapterError> {
        let result = sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
            .fetch_one(&self.pool)
            .await?;
        if result == "ok" {
            Ok(())
        } else {
            Err(SqliteAdapterError::IntegrityCheckFailed(result))
        }
    }

    /// Inserts an inbox row exactly once. Duplicate provider deliveries are acknowledged safely.
    pub async fn insert_inbox(
        &self,
        event: InboxEvent<'_>,
        now: i64,
    ) -> Result<bool, SqliteAdapterError> {
        let _write_gate = self.write_gate.lock().await;
        let result = sqlx::query(
            "INSERT INTO webhook_inbox (id, source, delivery_id, event_type, raw_headers, raw_body, status, received_at, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, 'pending', ?, ?, ?) \
             ON CONFLICT(source, delivery_id) DO NOTHING",
        )
        .bind(event.id)
        .bind(event.source)
        .bind(event.delivery_id)
        .bind(event.event_type)
        .bind(event.raw_headers)
        .bind(event.raw_body)
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Commits normalized work state, processed inbox state, and its causal outbox action together.
    pub async fn process_inbox_and_enqueue(
        &self,
        inbox_id: &str,
        work_item_id: &str,
        action: OutboxAction<'_>,
        now: i64,
    ) -> Result<(), SqliteAdapterError> {
        let _write_gate = self.write_gate.lock().await;
        let mut connection = self.pool.acquire().await?;
        begin_immediate(&mut connection).await?;
        let result = async {
            sqlx::query(
                "INSERT INTO work_items (id, state, revision, created_at, updated_at) \
                 VALUES (?, 'observed', 'initial', ?, ?) ON CONFLICT(id) DO NOTHING",
            )
            .bind(work_item_id)
            .bind(now)
            .bind(now)
            .execute(&mut *connection)
            .await?;
            let processed = sqlx::query(
                "UPDATE webhook_inbox SET status = 'processed', processed_at = ?, updated_at = ? \
                 WHERE id = ? AND status IN ('pending', 'processing')",
            )
            .bind(now)
            .bind(now)
            .bind(inbox_id)
            .execute(&mut *connection)
            .await?;
            if processed.rows_affected() != 1 {
                return Err(sqlx::Error::RowNotFound);
            }
            sqlx::query(
                "INSERT INTO outbox (id, kind, aggregate_id, idempotency_key, payload_json, status, attempts, next_attempt_at, created_at, updated_at) \
                 VALUES (?, ?, ?, ?, ?, 'pending', 0, ?, ?, ?)",
            )
            .bind(action.id)
            .bind(action.kind)
            .bind(action.aggregate_id)
            .bind(action.idempotency_key)
            .bind(action.payload_json)
            .bind(now)
            .bind(now)
            .bind(now)
            .execute(&mut *connection)
            .await?;
            Ok::<(), sqlx::Error>(())
        }
        .await;
        match result {
            Ok(()) => {
                sqlx::query("COMMIT").execute(&mut *connection).await?;
                Ok(())
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                Err(error.into())
            }
        }
    }

    pub async fn claim_outbox(
        &self,
        owner: &str,
        now: i64,
        lease_until: i64,
    ) -> Result<Option<LeasedOutboxAction>, SqliteAdapterError> {
        let _write_gate = self.write_gate.lock().await;
        let mut connection = self.pool.acquire().await?;
        begin_immediate(&mut connection).await?;
        let result = async {
            let claim = sqlx::query(
                "UPDATE outbox SET status = 'leased', lease_owner = ?, lease_expires_at = ?, attempts = attempts + 1, updated_at = ? \
                 WHERE id = (SELECT id FROM outbox WHERE status = 'pending' AND next_attempt_at <= ? ORDER BY created_at LIMIT 1) \
                 AND status = 'pending'",
            )
            .bind(owner)
            .bind(lease_until)
            .bind(now)
            .bind(now)
            .execute(&mut *connection)
            .await?;
            if claim.rows_affected() == 0 {
                return Ok(None);
            }
            sqlx::query(
                "SELECT id, idempotency_key, attempts FROM outbox WHERE status = 'leased' AND lease_owner = ? AND lease_expires_at = ? ORDER BY updated_at DESC LIMIT 1",
            )
            .bind(owner)
            .bind(lease_until)
            .fetch_optional(&mut *connection)
            .await
        }
        .await;
        match result {
            Ok(row) => {
                sqlx::query("COMMIT").execute(&mut *connection).await?;
                Ok(row.map(|row| LeasedOutboxAction {
                    id: row.get("id"),
                    idempotency_key: row.get("idempotency_key"),
                    attempts: row.get("attempts"),
                }))
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                Err(error.into())
            }
        }
    }

    pub async fn recover_expired_leases(&self, now: i64) -> Result<u64, SqliteAdapterError> {
        let _write_gate = self.write_gate.lock().await;
        let result = sqlx::query(
            "UPDATE outbox SET status = 'pending', lease_owner = NULL, lease_expires_at = NULL, updated_at = ?, last_error = 'recovery_pending' \
             WHERE status = 'leased' AND lease_expires_at < ?",
        )
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn recover_expired_inbox_leases(&self, now: i64) -> Result<u64, SqliteAdapterError> {
        let _write_gate = self.write_gate.lock().await;
        let result = sqlx::query(
            "UPDATE webhook_inbox SET status = 'pending', lease_owner = NULL, lease_expires_at = NULL, updated_at = ? \
             WHERE status = 'processing' AND lease_expires_at < ?",
        )
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn quarantine_inbox(
        &self,
        inbox_id: &str,
        error: &str,
        now: i64,
    ) -> Result<(), SqliteAdapterError> {
        let _write_gate = self.write_gate.lock().await;
        sqlx::query(
            "UPDATE webhook_inbox SET status = 'quarantined', last_error = ?, updated_at = ? WHERE id = ?",
        )
        .bind(error)
        .bind(now)
        .bind(inbox_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn create_run(&self, run: RunRecord<'_>, now: i64) -> Result<(), SqliteAdapterError> {
        let _write_gate = self.write_gate.lock().await;
        sqlx::query(
            "INSERT INTO runs (id, work_item_id, parent_run_id, root_run_id, role, harness, model, effort, status, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(run.id)
        .bind(run.work_item_id)
        .bind(run.parent_run_id)
        .bind(run.root_run_id)
        .bind(run.role)
        .bind(run.harness)
        .bind(run.model)
        .bind(run.effort)
        .bind(run.status)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Persists only canonical GitHub data. A harness-reported URL or SHA is
    /// never accepted here without this adapter-facing canonical fact.
    pub async fn persist_canonical_pull_request(
        &self,
        work_item_id: &str,
        expected_repository: &str,
        expected_branch: &str,
        pull_request: &CanonicalPullRequest,
        now: i64,
    ) -> Result<PullRequestPersistence, SqliteAdapterError> {
        if pull_request.repository.as_str() != expected_repository
            || pull_request.head_branch != expected_branch
        {
            return Ok(PullRequestPersistence::IgnoredRepositoryOrBranch);
        }
        let _write_gate = self.write_gate.lock().await;
        let mut connection = self.pool.acquire().await?;
        begin_immediate(&mut connection).await?;
        let result = async {
            let current = sqlx::query("SELECT repository, current_head_sha FROM work_items WHERE id = ?")
                .bind(work_item_id).fetch_optional(&mut *connection).await?;
            let Some(current) = current else { return Ok::<_, sqlx::Error>(PullRequestPersistence::StaleWorkItem) };
            if current.get::<Option<String>, _>("repository").as_deref() != Some(expected_repository) {
                return Ok(PullRequestPersistence::IgnoredRepositoryOrBranch);
            }
            let head_changed = current.get::<Option<String>, _>("current_head_sha").as_deref() != Some(pull_request.head_sha.as_str());
            sqlx::query("UPDATE work_items SET pull_request_number = ?, pull_request_url = ?, pull_request_branch = ?, base_branch = ?, base_sha = ?, current_head_sha = ?, state = 'waiting_for_ci', github_conflict_reason = NULL, updated_at = ? WHERE id = ?")
                .bind(pull_request.number as i64).bind(&pull_request.url).bind(&pull_request.head_branch).bind(&pull_request.base_branch).bind(pull_request.base_sha.as_str()).bind(pull_request.head_sha.as_str()).bind(now).bind(work_item_id).execute(&mut *connection).await?;
            if head_changed {
                sqlx::query("UPDATE review_cycles SET ci_state = 'invalidated', review_state = 'invalidated', updated_at = ? WHERE work_item_id = ? AND head_sha != ?")
                    .bind(now).bind(work_item_id).bind(pull_request.head_sha.as_str()).execute(&mut *connection).await?;
                sqlx::query("UPDATE review_waivers SET invalidated_at = ? WHERE work_item_id = ? AND head_sha != ? AND invalidated_at IS NULL")
                    .bind(now).bind(work_item_id).bind(pull_request.head_sha.as_str()).execute(&mut *connection).await?;
                // A stale checker may finish, but it cannot advance an invalidated cycle.
                sqlx::query("UPDATE runs SET status = 'cancel_requested', updated_at = ? WHERE work_item_id = ? AND role = 'review' AND status IN ('queued', 'starting', 'running')")
                    .bind(now).bind(work_item_id).execute(&mut *connection).await?;
            }
            Ok(PullRequestPersistence::Updated)
        }.await;
        match result {
            Ok(value) => {
                sqlx::query("COMMIT").execute(&mut *connection).await?;
                Ok(value)
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                Err(error.into())
            }
        }
    }

    /// Commits CI evidence only if the SHA is still the work item's current
    /// head. The caller must fetch the PR head immediately before this command.
    pub async fn persist_required_check_gate(
        &self,
        work_item_id: &str,
        head_sha: &str,
        gate: &RequiredCheckGate,
        now: i64,
    ) -> Result<CiGatePersistence, SqliteAdapterError> {
        let _write_gate = self.write_gate.lock().await;
        let mut connection = self.pool.acquire().await?;
        begin_immediate(&mut connection).await?;
        let result = async {
            let current = sqlx::query_scalar::<_, Option<String>>("SELECT current_head_sha FROM work_items WHERE id = ?").bind(work_item_id).fetch_optional(&mut *connection).await?.flatten();
            if current.as_deref() != Some(head_sha) { return Ok::<_, sqlx::Error>(CiGatePersistence::StaleHead); }
            let checks: &[CheckRun] = match gate { RequiredCheckGate::Failed { failures } => failures, RequiredCheckGate::Succeeded { evidence } => evidence, _ => &[] };
            for check in checks {
                sqlx::query("INSERT INTO github_check_evidence (id, work_item_id, head_sha, check_name, status, details_url, completed_at, observed_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(work_item_id, head_sha, check_name, status, details_url) DO UPDATE SET observed_at = excluded.observed_at")
                    .bind(Uuid::new_v4().to_string()).bind(work_item_id).bind(head_sha).bind(&check.name).bind(match check.status { spire_application::CheckStatus::Pending => "pending", spire_application::CheckStatus::Succeeded => "succeeded", spire_application::CheckStatus::Failed => "failed", spire_application::CheckStatus::Cancelled => "cancelled", spire_application::CheckStatus::Skipped => "skipped" }).bind(&check.details_url).bind(check.completed_at_unix_seconds).bind(now).execute(&mut *connection).await?;
            }
            let state = match gate { RequiredCheckGate::Succeeded { .. } => "waiting_for_review", _ => "waiting_for_ci" };
            sqlx::query("UPDATE work_items SET state = ?, updated_at = ? WHERE id = ? AND current_head_sha = ?")
                .bind(state).bind(now).bind(work_item_id).bind(head_sha).execute(&mut *connection).await?;
            Ok(CiGatePersistence::Applied)
        }.await;
        match result {
            Ok(value) => {
                sqlx::query("COMMIT").execute(&mut *connection).await?;
                Ok(value)
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                Err(error.into())
            }
        }
    }

    pub async fn active_pull_request_work_items(
        &self,
    ) -> Result<Vec<(String, String, u64, String)>, SqliteAdapterError> {
        let rows = sqlx::query("SELECT id, repository, pull_request_number, pull_request_branch FROM work_items WHERE state NOT IN ('blocked', 'completed', 'canceled') AND repository IS NOT NULL AND pull_request_number IS NOT NULL")
            .fetch_all(&self.pool).await?;
        Ok(rows
            .into_iter()
            .map(|row| {
                (
                    row.get("id"),
                    row.get("repository"),
                    row.get::<i64, _>("pull_request_number") as u64,
                    row.get::<Option<String>, _>("pull_request_branch")
                        .unwrap_or_default(),
                )
            })
            .collect())
    }

    /// Applies a canonical close or merge observation. A human action never
    /// causes Spire to push, reopen, or otherwise overwrite repository state.
    pub async fn persist_terminal_pull_request(
        &self,
        work_item_id: &str,
        repository: &str,
        branch: &str,
        pull_request: &CanonicalPullRequest,
        now: i64,
    ) -> Result<PullRequestPersistence, SqliteAdapterError> {
        if pull_request.repository.as_str() != repository || pull_request.head_branch != branch {
            return Ok(PullRequestPersistence::IgnoredRepositoryOrBranch);
        }
        let state = match pull_request.state {
            PullRequestState::Merged => "completed",
            PullRequestState::Closed => "canceled",
            PullRequestState::Open => return Ok(PullRequestPersistence::Updated),
        };
        let _write_gate = self.write_gate.lock().await;
        let updated = sqlx::query("UPDATE work_items SET state = ?, active_run_id = NULL, updated_at = ? WHERE id = ? AND repository = ? AND pull_request_number = ?")
            .bind(state).bind(now).bind(work_item_id).bind(repository).bind(pull_request.number as i64).execute(&self.pool).await?;
        Ok(if updated.rows_affected() == 1 {
            PullRequestPersistence::Updated
        } else {
            PullRequestPersistence::StaleWorkItem
        })
    }

    /// Creates one AI-initiated correction using the already sticky maker
    /// selection. It never re-evaluates provider candidates after code exists.
    pub async fn schedule_ci_correction(
        &self,
        work_item_id: &str,
        run_id: &str,
        limits: CapacityLimits,
        now: i64,
    ) -> Result<CiCorrectionPersistence, SqliteAdapterError> {
        let _write_gate = self.write_gate.lock().await;
        let mut connection = self.pool.acquire().await?;
        begin_immediate(&mut connection).await?;
        let result = async {
            let item = sqlx::query("SELECT repository, ci_correction_cycles FROM work_items WHERE id = ? AND state = 'waiting_for_ci'")
                .bind(work_item_id).fetch_optional(&mut *connection).await?;
            let Some(item) = item else { return Ok::<_, sqlx::Error>(CiCorrectionPersistence::StaleWorkItem) };
            let completed_cycles = item.get::<i64, _>("ci_correction_cycles") as u8;
            if completed_cycles >= spire_application::MAX_INITIAL_CI_CORRECTION_CYCLES {
                sqlx::query("UPDATE work_items SET state = 'blocked', updated_at = ? WHERE id = ?").bind(now).bind(work_item_id).execute(&mut *connection).await?;
                return Ok(CiCorrectionPersistence::Exhausted);
            }
            let repository = item.get::<Option<String>, _>("repository").unwrap_or_default();
            let counts = CapacityCounts {
                total: sqlx::query_scalar::<_, i64>("SELECT count(*) FROM runs WHERE status IN ('starting', 'running')").fetch_one(&mut *connection).await? as u16,
                ai: sqlx::query_scalar::<_, i64>("SELECT count(*) FROM runs WHERE status IN ('starting', 'running') AND initiator = 'ai'").fetch_one(&mut *connection).await? as u16,
                repository: sqlx::query_scalar::<_, i64>("SELECT count(*) FROM runs r JOIN work_items w ON w.id = r.work_item_id WHERE r.status IN ('starting', 'running') AND w.repository = ?").bind(&repository).fetch_one(&mut *connection).await? as u16,
                ticket: sqlx::query_scalar::<_, i64>("SELECT count(*) FROM runs WHERE status IN ('starting', 'running') AND work_item_id = ?").bind(work_item_id).fetch_one(&mut *connection).await? as u16,
            };
            if capacity_allows(limits, counts, SchedulerInitiator::Ai).is_err() { return Ok(CiCorrectionPersistence::CapacityBlocked); }
            let maker = sqlx::query("SELECT id, root_run_id, harness, model, effort FROM runs WHERE work_item_id = ? AND role = 'implementation' ORDER BY created_at ASC LIMIT 1")
                .bind(work_item_id).fetch_optional(&mut *connection).await?;
            let Some(maker) = maker else { return Ok(CiCorrectionPersistence::NoStickyMaker) };
            let cycle = completed_cycles + 1;
            sqlx::query("INSERT INTO runs (id, work_item_id, parent_run_id, root_run_id, role, harness, model, effort, status, initiator, trigger_kind, created_at, updated_at) VALUES (?, ?, ?, ?, 'implementation', ?, ?, ?, 'starting', 'ai', 'ci_failed', ?, ?)")
                .bind(run_id).bind(work_item_id).bind(maker.get::<String, _>("id")).bind(maker.get::<String, _>("root_run_id")).bind(maker.get::<String, _>("harness")).bind(maker.get::<String, _>("model")).bind(maker.get::<String, _>("effort")).bind(now).bind(now).execute(&mut *connection).await?;
            sqlx::query("UPDATE work_items SET active_run_id = ?, ci_correction_cycles = ?, updated_at = ? WHERE id = ?")
                .bind(run_id).bind(i64::from(cycle)).bind(now).bind(work_item_id).execute(&mut *connection).await?;
            Ok(CiCorrectionPersistence::Scheduled { cycle })
        }.await;
        match result {
            Ok(value) => {
                sqlx::query("COMMIT").execute(&mut *connection).await?;
                Ok(value)
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                Err(error.into())
            }
        }
    }

    /// Creates exactly one fresh, different-harness review run for a CI-green
    /// current head. The whole admission decision is one short transaction.
    pub async fn schedule_review(
        &self,
        dispatch: ReviewDispatch<'_>,
        limits: CapacityLimits,
        now: i64,
    ) -> Result<ReviewDispatchPersistence, SqliteAdapterError> {
        let _write_gate = self.write_gate.lock().await;
        let mut connection = self.pool.acquire().await?;
        begin_immediate(&mut connection).await?;
        let result = async {
            let item = sqlx::query("SELECT repository, current_head_sha, base_sha, review_correction_cycles, review_candidates_json, review_policy_version, review_rule_id FROM work_items WHERE id = ? AND state = 'waiting_for_review'")
                .bind(dispatch.work_item_id).fetch_optional(&mut *connection).await?;
            let Some(item) = item else { return Ok::<_, sqlx::Error>(ReviewDispatchPersistence::StaleWorkItem) };
            let head_sha = item.get::<Option<String>, _>("current_head_sha");
            let base_sha = item.get::<Option<String>, _>("base_sha");
            let (Some(head_sha), Some(base_sha)) = (head_sha, base_sha) else { return Ok(ReviewDispatchPersistence::StaleWorkItem) };
            let existing = sqlx::query("SELECT review_state, review_run_id FROM review_cycles WHERE work_item_id = ? AND head_sha = ?")
                .bind(dispatch.work_item_id).bind(&head_sha).fetch_optional(&mut *connection).await?;
            if existing.is_some() {
                return Ok(ReviewDispatchPersistence::AlreadyScheduled);
            }
            let repository = item.get::<Option<String>, _>("repository").unwrap_or_default();
            let counts = capacity_counts_for(&mut connection, dispatch.work_item_id, &repository).await?;
            if capacity_allows(limits, counts, SchedulerInitiator::Ai).is_err() {
                return Ok(ReviewDispatchPersistence::CapacityBlocked);
            }
            let maker = sqlx::query("SELECT id, root_run_id, harness FROM runs WHERE work_item_id = ? AND role = 'implementation' ORDER BY created_at ASC LIMIT 1")
                .bind(dispatch.work_item_id).fetch_optional(&mut *connection).await?;
            let Some(maker) = maker else { return Ok(ReviewDispatchPersistence::MissingStickyMaker) };
            if maker.get::<String, _>("harness") == dispatch.harness {
                return Ok(ReviewDispatchPersistence::SameAsMaker);
            }
            let candidates_json = item.get::<String, _>("review_candidates_json");
            let candidates = serde_json::from_str::<Vec<spire_domain::DispatchCandidate>>(&candidates_json).unwrap_or_default();
            if !candidates.iter().any(|candidate| candidate.harness.as_str() == dispatch.harness && candidate.model.as_str() == dispatch.model && effort_name(candidate.effort) == dispatch.effort) {
                return Ok(ReviewDispatchPersistence::CandidateNotSnapshotted);
            }
            let round = item.get::<i64, _>("review_correction_cycles") as u8 + 1;
            sqlx::query("INSERT INTO review_cycles (id, work_item_id, round, implementation_run_id, base_sha, head_sha, ci_state, review_state, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, 'succeeded', 'running', ?, ?)")
                .bind(dispatch.review_cycle_id).bind(dispatch.work_item_id).bind(i64::from(round)).bind(maker.get::<String, _>("id")).bind(&base_sha).bind(&head_sha).bind(now).bind(now).execute(&mut *connection).await?;
            sqlx::query("INSERT INTO dispatch_decisions (id, work_item_id, run_id, policy_version, rule_id, role, complexity_estimate, complexity_class, candidate_schema_version, candidates_json, selected_candidate_index, candidate_evaluation_schema_version, candidate_evaluations_json, created_at) SELECT ?, w.id, ?, w.review_policy_version, w.review_rule_id, 'review', dd.complexity_estimate, dd.complexity_class, 1, w.review_candidates_json, ?, 1, ?, ? FROM work_items w JOIN dispatch_decisions dd ON dd.work_item_id = w.id AND dd.role = 'implementation' WHERE w.id = ? ORDER BY dd.created_at ASC LIMIT 1")
                .bind(dispatch.decision_id).bind(dispatch.run_id).bind(dispatch.selected_candidate_index as i64).bind(dispatch.evaluation_json).bind(now).bind(dispatch.work_item_id).execute(&mut *connection).await?;
            sqlx::query("INSERT INTO runs (id, work_item_id, parent_run_id, root_run_id, role, harness, model, effort, status, dispatch_decision_id, initiator, trigger_kind, created_at, updated_at) VALUES (?, ?, ?, ?, 'review', ?, ?, ?, 'starting', ?, 'ai', 'review_required', ?, ?)")
                .bind(dispatch.run_id).bind(dispatch.work_item_id).bind(maker.get::<String, _>("id")).bind(maker.get::<String, _>("root_run_id")).bind(dispatch.harness).bind(dispatch.model).bind(dispatch.effort).bind(dispatch.decision_id).bind(now).bind(now).execute(&mut *connection).await?;
            sqlx::query("UPDATE review_cycles SET review_run_id = ? WHERE id = ?")
                .bind(dispatch.run_id).bind(dispatch.review_cycle_id).execute(&mut *connection).await?;
            sqlx::query("UPDATE work_items SET active_run_id = ?, updated_at = ? WHERE id = ?")
                .bind(dispatch.run_id).bind(now).bind(dispatch.work_item_id).execute(&mut *connection).await?;
            Ok(ReviewDispatchPersistence::Scheduled { round })
        }.await;
        commit_result(&mut connection, result).await
    }

    /// Stores only a schema-validated review result. An approval cannot advance
    /// a changed head or a failed CI gate, even if the reviewer finishes later.
    pub async fn record_review_result(
        &self,
        work_item_id: &str,
        review_cycle_id: &str,
        review_run_id: &str,
        result: &ReviewResult,
        now: i64,
    ) -> Result<ReviewResultPersistence, SqliteAdapterError> {
        result
            .validate_for(&result.reviewed_head_sha)
            .map_err(|error| SqliteAdapterError::ReviewContract(error.to_string()))?;
        let _write_gate = self.write_gate.lock().await;
        let mut connection = self.pool.acquire().await?;
        begin_immediate(&mut connection).await?;
        let transaction = async {
            let row = sqlx::query("SELECT w.current_head_sha, w.linear_issue_id, c.ci_state, c.review_state FROM review_cycles c JOIN work_items w ON w.id = c.work_item_id WHERE c.id = ? AND c.work_item_id = ? AND c.review_run_id = ?")
                .bind(review_cycle_id).bind(work_item_id).bind(review_run_id).fetch_optional(&mut *connection).await?;
            let Some(row) = row else { return Ok::<_, sqlx::Error>(ReviewResultPersistence::StaleWorkItem) };
            if row.get::<Option<String>, _>("current_head_sha").as_deref() != Some(result.reviewed_head_sha.as_str()) {
                return Ok(ReviewResultPersistence::StaleHead);
            }
            if row.get::<String, _>("ci_state") != "succeeded" || row.get::<String, _>("review_state") != "running" {
                return Ok(ReviewResultPersistence::InvalidCiGate);
            }
            for finding in &result.findings {
                sqlx::query("INSERT INTO review_findings (id, review_cycle_id, stable_id, severity, file, line, title, rationale, requested_change, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(review_cycle_id, stable_id) DO NOTHING")
                    .bind(Uuid::new_v4().to_string()).bind(review_cycle_id).bind(&finding.stable_id).bind(match finding.severity { spire_application::FindingSeverity::Critical => "critical", spire_application::FindingSeverity::High => "high", spire_application::FindingSeverity::Medium => "medium", spire_application::FindingSeverity::Low => "low" }).bind(&finding.file).bind(finding.line.map(i64::from)).bind(&finding.title).bind(&finding.rationale).bind(&finding.requested_change).bind(now).execute(&mut *connection).await?;
            }
            let state = match result.verdict { ReviewVerdict::Approved => "human_ready", ReviewVerdict::ChangesRequired => "waiting_for_review", ReviewVerdict::Blocked => "blocked" };
            sqlx::query("UPDATE review_cycles SET review_state = ?, completed_at = ?, updated_at = ? WHERE id = ?")
                .bind(result.verdict.as_str()).bind(now).bind(now).bind(review_cycle_id).execute(&mut *connection).await?;
            sqlx::query("UPDATE runs SET status = 'succeeded', updated_at = ? WHERE id = ? AND status IN ('starting', 'running')")
                .bind(now).bind(review_run_id).execute(&mut *connection).await?;
            sqlx::query("UPDATE work_items SET state = ?, active_run_id = NULL, updated_at = ? WHERE id = ? AND current_head_sha = ?")
                .bind(state).bind(now).bind(work_item_id).bind(result.reviewed_head_sha.as_str()).execute(&mut *connection).await?;
            let payload = serde_json::json!({"review_cycle_id": review_cycle_id, "reviewed_head_sha": result.reviewed_head_sha.as_str(), "verdict": result.verdict.as_str(), "summary": result.summary});
            sqlx::query("INSERT INTO outbox (id, kind, aggregate_id, idempotency_key, payload_json, status, attempts, next_attempt_at, created_at, updated_at) VALUES (?, 'github_review_summary', ?, ?, ?, 'pending', 0, ?, ?, ?) ON CONFLICT(idempotency_key) DO NOTHING")
                .bind(Uuid::new_v4().to_string()).bind(work_item_id).bind(format!("review:{review_run_id}:{}", result.reviewed_head_sha.as_str())).bind(payload.to_string()).bind(now).bind(now).bind(now).execute(&mut *connection).await?;
            if let Some(issue_id) = row.get::<Option<String>, _>("linear_issue_id") {
                let key = format!("review-linear:{review_run_id}:{}", result.reviewed_head_sha.as_str());
                let marker = spire_application::comment_marker(&key);
                let body = format!(
                    "Spire independent review for SHA `{}`: `{}`. {}\n\n{}\n",
                    result.reviewed_head_sha,
                    result.verdict.as_str(),
                    spire_application::sanitize(&result.summary, 200),
                    marker,
                );
                let payload = serde_json::json!({"issue_id": issue_id, "marker": marker, "body": body});
                sqlx::query("INSERT INTO outbox (id, kind, aggregate_id, idempotency_key, payload_json, status, attempts, next_attempt_at, created_at, updated_at) VALUES (?, 'linear_comment', ?, ?, ?, 'pending', 0, ?, ?, ?) ON CONFLICT(idempotency_key) DO NOTHING")
                    .bind(Uuid::new_v4().to_string()).bind(work_item_id).bind(key).bind(payload.to_string()).bind(now).bind(now).bind(now).execute(&mut *connection).await?;
            }
            Ok(ReviewResultPersistence::Applied { verdict: result.verdict })
        }.await;
        commit_result(&mut connection, transaction).await
    }

    /// Schedules the maker correction after an actionable current-SHA review.
    /// Capacity waits return without incrementing the engineering cycle.
    pub async fn schedule_review_correction(
        &self,
        work_item_id: &str,
        run_id: &str,
        limits: CapacityLimits,
        now: i64,
    ) -> Result<ReviewCorrectionPersistence, SqliteAdapterError> {
        let _write_gate = self.write_gate.lock().await;
        let mut connection = self.pool.acquire().await?;
        begin_immediate(&mut connection).await?;
        let transaction = async {
            let item = sqlx::query("SELECT repository, current_head_sha, review_correction_cycles FROM work_items WHERE id = ? AND state = 'waiting_for_review'")
                .bind(work_item_id).fetch_optional(&mut *connection).await?;
            let Some(item) = item else { return Ok::<_, sqlx::Error>(ReviewCorrectionPersistence::StaleWorkItem) };
            let Some(head_sha) = item.get::<Option<String>, _>("current_head_sha") else { return Ok(ReviewCorrectionPersistence::StaleWorkItem) };
            let cycle = sqlx::query("SELECT review_run_id FROM review_cycles WHERE work_item_id = ? AND head_sha = ? AND review_state = 'changes_required'")
                .bind(work_item_id).bind(&head_sha).fetch_optional(&mut *connection).await?;
            let Some(cycle) = cycle else { return Ok(ReviewCorrectionPersistence::StaleWorkItem) };
            let completed = item.get::<i64, _>("review_correction_cycles") as u8;
            if completed >= spire_application::MAX_INITIAL_REVIEW_CORRECTION_CYCLES {
                sqlx::query("UPDATE work_items SET state = 'blocked', updated_at = ? WHERE id = ?").bind(now).bind(work_item_id).execute(&mut *connection).await?;
                return Ok(ReviewCorrectionPersistence::Exhausted);
            }
            let repository = item.get::<Option<String>, _>("repository").unwrap_or_default();
            let counts = capacity_counts_for(&mut connection, work_item_id, &repository).await?;
            if capacity_allows(limits, counts, SchedulerInitiator::Ai).is_err() { return Ok(ReviewCorrectionPersistence::CapacityBlocked); }
            let maker = sqlx::query("SELECT id, root_run_id, harness, model, effort FROM runs WHERE work_item_id = ? AND role = 'implementation' ORDER BY created_at ASC LIMIT 1")
                .bind(work_item_id).fetch_optional(&mut *connection).await?;
            let Some(maker) = maker else { return Ok(ReviewCorrectionPersistence::MissingStickyMaker) };
            let next_cycle = completed + 1;
            sqlx::query("INSERT INTO runs (id, work_item_id, parent_run_id, root_run_id, role, harness, model, effort, status, initiator, trigger_kind, created_at, updated_at) VALUES (?, ?, ?, ?, 'implementation', ?, ?, ?, 'starting', 'ai', 'review_changes_requested', ?, ?)")
                .bind(run_id).bind(work_item_id).bind(cycle.get::<Option<String>, _>("review_run_id")).bind(maker.get::<String, _>("root_run_id")).bind(maker.get::<String, _>("harness")).bind(maker.get::<String, _>("model")).bind(maker.get::<String, _>("effort")).bind(now).bind(now).execute(&mut *connection).await?;
            sqlx::query("UPDATE work_items SET state = 'implementing', active_run_id = ?, review_correction_cycles = ?, updated_at = ? WHERE id = ?")
                .bind(run_id).bind(i64::from(next_cycle)).bind(now).bind(work_item_id).execute(&mut *connection).await?;
            Ok(ReviewCorrectionPersistence::Scheduled { cycle: next_cycle })
        }.await;
        commit_result(&mut connection, transaction).await
    }

    /// An authenticated human may waive review findings only for a CI-green,
    /// unchanged SHA. A later push invalidates the row in the PR update path.
    pub async fn apply_review_waiver(
        &self,
        work_item_id: &str,
        head_sha: &str,
        actor: &str,
        reason: &str,
        authorized: bool,
        now: i64,
    ) -> Result<ReviewWaiverPersistence, SqliteAdapterError> {
        if !authorized || actor.trim().is_empty() || reason.trim().is_empty() {
            return Ok(ReviewWaiverPersistence::Unauthorized);
        }
        let _write_gate = self.write_gate.lock().await;
        let mut connection = self.pool.acquire().await?;
        begin_immediate(&mut connection).await?;
        let transaction = async {
            let cycle = sqlx::query("SELECT c.ci_state FROM review_cycles c JOIN work_items w ON w.id = c.work_item_id WHERE c.work_item_id = ? AND c.head_sha = ? AND w.current_head_sha = ? AND w.active_run_id IS NULL")
                .bind(work_item_id).bind(head_sha).bind(head_sha).fetch_optional(&mut *connection).await?;
            let Some(cycle) = cycle else { return Ok::<_, sqlx::Error>(ReviewWaiverPersistence::StaleHead) };
            if cycle.get::<String, _>("ci_state") != "succeeded" { return Ok(ReviewWaiverPersistence::InvalidCiGate); }
            sqlx::query("INSERT INTO review_waivers (id, work_item_id, head_sha, actor, reason, created_at) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(work_item_id, head_sha) DO NOTHING")
                .bind(Uuid::new_v4().to_string()).bind(work_item_id).bind(head_sha).bind(actor).bind(reason).bind(now).execute(&mut *connection).await?;
            sqlx::query("UPDATE work_items SET state = 'human_ready', active_run_id = NULL, updated_at = ? WHERE id = ? AND current_head_sha = ?")
                .bind(now).bind(work_item_id).bind(head_sha).execute(&mut *connection).await?;
            Ok(ReviewWaiverPersistence::Applied)
        }.await;
        commit_result(&mut connection, transaction).await
    }

    pub async fn record_review_publication(
        &self,
        review_cycle_id: &str,
        head_sha: &str,
        published_comment_id: &str,
        now: i64,
    ) -> Result<ReviewPublicationPersistence, SqliteAdapterError> {
        let _write_gate = self.write_gate.lock().await;
        let result = sqlx::query("UPDATE review_cycles SET published_comment_id = COALESCE(published_comment_id, ?), updated_at = ? WHERE id = ? AND head_sha = ? AND EXISTS (SELECT 1 FROM work_items w WHERE w.id = review_cycles.work_item_id AND w.current_head_sha = review_cycles.head_sha)")
            .bind(published_comment_id).bind(now).bind(review_cycle_id).bind(head_sha).execute(&self.pool).await?;
        Ok(if result.rows_affected() == 1 {
            ReviewPublicationPersistence::Applied
        } else {
            ReviewPublicationPersistence::StaleHead
        })
    }

    /// Observations only replace unclaimed work with an equal-or-newer canonical revision.
    pub async fn upsert_linear_observation(
        &self,
        observation: LinearObservation<'_>,
        now: i64,
    ) -> Result<bool, SqliteAdapterError> {
        let _write_gate = self.write_gate.lock().await;
        let result = sqlx::query(
            "INSERT INTO work_items (id, linear_issue_id, linear_identifier, team_id, workflow_state_id, revision, raw_estimate, complexity_class, eligibility_reason, state, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'observed', ?, ?) \
             ON CONFLICT(linear_issue_id) DO UPDATE SET \
                linear_identifier = excluded.linear_identifier, team_id = excluded.team_id, workflow_state_id = excluded.workflow_state_id, revision = excluded.revision, raw_estimate = excluded.raw_estimate, complexity_class = excluded.complexity_class, eligibility_reason = excluded.eligibility_reason, updated_at = excluded.updated_at \
             WHERE work_items.state NOT IN ('claiming', 'queued', 'implementing', 'waiting_for_ci', 'waiting_for_review') AND excluded.revision >= work_items.revision",
        )
        .bind(observation.work_item_id)
        .bind(observation.linear_issue_id)
        .bind(observation.linear_identifier)
        .bind(observation.team_id)
        .bind(observation.workflow_state_id)
        .bind(observation.revision)
        .bind(observation.raw_estimate.map(i64::from))
        .bind(observation.complexity_class)
        .bind(observation.eligibility_reason)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Makes the complete root claim durable before any provider or Linear work starts.
    pub async fn claim_root(
        &self,
        claim: RootClaim<'_>,
        limits: CapacityLimits,
        now: i64,
    ) -> Result<RootClaimResult, SqliteAdapterError> {
        let _write_gate = self.write_gate.lock().await;
        let mut connection = self.pool.acquire().await?;
        begin_immediate(&mut connection).await?;
        let result = async {
            let row = sqlx::query("SELECT revision, state FROM work_items WHERE id = ?")
                .bind(claim.work_item_id).fetch_optional(&mut *connection).await?;
            let Some(row) = row else { return Ok::<RootClaimResult, sqlx::Error>(RootClaimResult::RevisionChanged); };
            if row.get::<String, _>("revision") != claim.expected_revision { return Ok::<RootClaimResult, sqlx::Error>(RootClaimResult::RevisionChanged); }
            let state: String = row.get("state");
            if matches!(state.as_str(), "claiming" | "queued" | "implementing" | "waiting_for_ci" | "waiting_for_review") { return Ok::<RootClaimResult, sqlx::Error>(RootClaimResult::AlreadyOwned); }
            let counts = CapacityCounts {
                total: sqlx::query_scalar::<_, i64>("SELECT count(*) FROM runs WHERE status IN ('starting', 'running')").fetch_one(&mut *connection).await? as u16,
                ai: sqlx::query_scalar::<_, i64>("SELECT count(*) FROM runs WHERE status IN ('starting', 'running') AND initiator = 'ai'").fetch_one(&mut *connection).await? as u16,
                repository: sqlx::query_scalar::<_, i64>("SELECT count(*) FROM runs r JOIN work_items w ON w.id = r.work_item_id WHERE r.status IN ('starting', 'running') AND w.repository = ?").bind(claim.repository).fetch_one(&mut *connection).await? as u16,
                ticket: sqlx::query_scalar::<_, i64>("SELECT count(*) FROM runs WHERE status IN ('starting', 'running') AND work_item_id = ?").bind(claim.work_item_id).fetch_one(&mut *connection).await? as u16,
            };
            if capacity_allows(limits, counts, claim.initiator).is_err() { return Ok::<RootClaimResult, sqlx::Error>(RootClaimResult::CapacityBlocked); }
            sqlx::query("INSERT INTO dispatch_decisions (id, work_item_id, run_id, policy_version, rule_id, role, complexity_estimate, complexity_class, candidate_schema_version, candidates_json, selected_candidate_index, candidate_evaluation_schema_version, candidate_evaluations_json, created_at) VALUES (?, ?, ?, ?, ?, 'implementation', ?, ?, 1, ?, 0, 1, ?, ?)")
                .bind(claim.decision_id).bind(claim.work_item_id).bind(claim.run_id).bind(i64::from(claim.policy_version)).bind(claim.implementation_rule_id).bind(i64::from(claim.raw_estimate)).bind(claim.complexity_class).bind(claim.candidate_json).bind(claim.evaluation_json).bind(now).execute(&mut *connection).await?;
            sqlx::query("INSERT INTO runs (id, work_item_id, root_run_id, role, harness, model, effort, status, dispatch_decision_id, initiator, trigger_kind, created_at, updated_at) VALUES (?, ?, ?, 'implementation', ?, ?, ?, 'starting', ?, ?, ?, ?, ?)")
                .bind(claim.run_id).bind(claim.work_item_id).bind(claim.run_id).bind(claim.harness).bind(claim.model).bind(claim.effort).bind(claim.decision_id).bind(initiator_name(claim.initiator)).bind(claim.trigger_kind).bind(now).bind(now).execute(&mut *connection).await?;
            sqlx::query("UPDATE work_items SET state = 'claiming', repository = ?, active_run_id = ?, updated_at = ? WHERE id = ?")
                .bind(claim.repository).bind(claim.run_id).bind(now).bind(claim.work_item_id).execute(&mut *connection).await?;
            sqlx::query("UPDATE work_items SET review_candidates_json = ?, review_policy_version = ?, review_rule_id = ? WHERE id = ?")
                .bind(claim.review_candidate_json).bind(i64::from(claim.policy_version)).bind(claim.review_rule_id).bind(claim.work_item_id).execute(&mut *connection).await?;
            Ok(RootClaimResult::Claimed)
        }.await;
        match result {
            Ok(value) => {
                sqlx::query("COMMIT").execute(&mut *connection).await?;
                Ok(value)
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                Err(error.into())
            }
        }
    }

    pub async fn claim_inbox(
        &self,
        owner: &str,
        now: i64,
        lease_until: i64,
    ) -> Result<Option<String>, SqliteAdapterError> {
        let _write_gate = self.write_gate.lock().await;
        let mut connection = self.pool.acquire().await?;
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *connection)
            .await?;
        let result = async {
            let claim = sqlx::query(
                "UPDATE webhook_inbox SET status = 'processing', lease_owner = ?, lease_expires_at = ?, attempts = attempts + 1, updated_at = ? \
                 WHERE id = (SELECT id FROM webhook_inbox WHERE status = 'pending' ORDER BY received_at LIMIT 1) AND status = 'pending'",
            )
            .bind(owner)
            .bind(lease_until)
            .bind(now)
            .execute(&mut *connection)
            .await?;
            if claim.rows_affected() == 0 {
                return Ok(None);
            }
            sqlx::query_scalar::<_, String>(
                "SELECT id FROM webhook_inbox WHERE status = 'processing' AND lease_owner = ? AND lease_expires_at = ? ORDER BY updated_at DESC LIMIT 1",
            )
            .bind(owner)
            .bind(lease_until)
            .fetch_optional(&mut *connection)
            .await
        }
        .await;
        match result {
            Ok(id) => {
                sqlx::query("COMMIT").execute(&mut *connection).await?;
                Ok(id)
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                Err(error.into())
            }
        }
    }

    pub async fn acknowledge_outbox(
        &self,
        id: &str,
        owner: &str,
        external_reference: &str,
        now: i64,
    ) -> Result<bool, SqliteAdapterError> {
        let _write_gate = self.write_gate.lock().await;
        let result = sqlx::query(
            "UPDATE outbox SET status = 'delivered', external_reference = ?, lease_owner = NULL, lease_expires_at = NULL, updated_at = ? \
             WHERE id = ? AND status = 'leased' AND lease_owner = ?",
        )
        .bind(external_reference)
        .bind(now)
        .bind(id)
        .bind(owner)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn renew_outbox_lease(
        &self,
        id: &str,
        owner: &str,
        now: i64,
        lease_until: i64,
    ) -> Result<bool, SqliteAdapterError> {
        let _write_gate = self.write_gate.lock().await;
        let result = sqlx::query(
            "UPDATE outbox SET lease_expires_at = ?, updated_at = ? \
             WHERE id = ? AND status = 'leased' AND lease_owner = ? AND lease_expires_at >= ?",
        )
        .bind(lease_until)
        .bind(now)
        .bind(id)
        .bind(owner)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn release_outbox_lease(
        &self,
        id: &str,
        owner: &str,
        next_attempt_at: i64,
        error_class: &str,
        error: &str,
        now: i64,
    ) -> Result<bool, SqliteAdapterError> {
        let _write_gate = self.write_gate.lock().await;
        let result = sqlx::query(
            "UPDATE outbox SET status = 'pending', lease_owner = NULL, lease_expires_at = NULL, next_attempt_at = ?, error_class = ?, last_error = ?, updated_at = ? \
             WHERE id = ? AND status = 'leased' AND lease_owner = ?",
        )
        .bind(next_attempt_at)
        .bind(error_class)
        .bind(error)
        .bind(now)
        .bind(id)
        .bind(owner)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn release_run_lease(
        &self,
        run_id: &str,
        owner: &str,
        now: i64,
    ) -> Result<bool, SqliteAdapterError> {
        let _write_gate = self.write_gate.lock().await;
        let result = sqlx::query(
            "UPDATE runs SET lease_owner = NULL, lease_expires_at = NULL, updated_at = ? \
             WHERE id = ? AND lease_owner = ?",
        )
        .bind(now)
        .bind(run_id)
        .bind(owner)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn renew_run_lease(
        &self,
        run_id: &str,
        owner: &str,
        now: i64,
        lease_until: i64,
    ) -> Result<bool, SqliteAdapterError> {
        let _write_gate = self.write_gate.lock().await;
        let result = sqlx::query(
            "UPDATE runs SET lease_expires_at = ?, updated_at = ? \
             WHERE id = ? AND status IN ('queued', 'starting', 'running', 'cancel_requested') \
             AND lease_owner = ? AND lease_expires_at >= ?",
        )
        .bind(lease_until)
        .bind(now)
        .bind(run_id)
        .bind(owner)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn acquire_run_lease(
        &self,
        run_id: &str,
        owner: &str,
        now: i64,
        lease_until: i64,
    ) -> Result<bool, SqliteAdapterError> {
        let _write_gate = self.write_gate.lock().await;
        let result = sqlx::query(
            "UPDATE runs SET lease_owner = ?, lease_expires_at = ?, recovery_pending = 0, updated_at = ? \
             WHERE id = ? AND status IN ('queued', 'starting', 'running', 'cancel_requested') \
             AND (lease_expires_at IS NULL OR lease_expires_at < ? OR lease_owner = ?)",
        )
        .bind(owner)
        .bind(lease_until)
        .bind(now)
        .bind(run_id)
        .bind(now)
        .bind(owner)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn mark_expired_runs_for_recovery(
        &self,
        now: i64,
    ) -> Result<u64, SqliteAdapterError> {
        let _write_gate = self.write_gate.lock().await;
        let result = sqlx::query(
            "UPDATE runs SET recovery_pending = 1, updated_at = ? \
             WHERE status IN ('queued', 'starting', 'running', 'cancel_requested') \
             AND lease_expires_at < ? AND recovery_pending = 0",
        )
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// SQLite's `VACUUM INTO` is an online consistent backup and leaves the live database untouched.
    pub async fn backup_to(&self, destination: impl AsRef<Path>) -> Result<(), SqliteAdapterError> {
        let destination = destination.as_ref();
        if destination == self.database_path {
            return Err(SqliteAdapterError::BackupWouldOverwriteLiveDatabase);
        }
        let escaped = destination.display().to_string().replace('\'', "''");
        sqlx::query(&format!("VACUUM INTO '{escaped}'"))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub fn new_id() -> String {
        Uuid::new_v4().to_string()
    }
}

#[allow(async_fn_in_trait)]
impl ProjectMappingPort for SqliteDatabase {
    type Error = SqliteAdapterError;

    async fn resolve(
        &self,
        linear_organization_id: &str,
        linear_project_id: &LinearProjectId,
    ) -> Result<ProjectRoutingDecision, Self::Error> {
        let rows = sqlx::query(
            "SELECT * FROM project_repository_mappings WHERE linear_organization_id = ? AND linear_project_id = ?",
        )
        .bind(linear_organization_id)
        .bind(linear_project_id.as_str())
        .fetch_all(&self.pool)
        .await?;
        let mappings = rows
            .iter()
            .map(project_mapping_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(spire_application::resolve_project_routing(
            &mappings,
            spire_application::MappingRepositoryHealth::Healthy,
        ))
    }

    async fn list(&self) -> Result<Vec<ProjectRepositoryMapping>, Self::Error> {
        sqlx::query(
            "SELECT * FROM project_repository_mappings ORDER BY linear_organization_id, linear_project_id",
        )
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(project_mapping_from_row)
        .collect()
    }

    async fn create(
        &self,
        mapping: NewProjectRepositoryMapping,
        actor: &str,
        reason: Option<&str>,
    ) -> Result<ProjectRepositoryMapping, Self::Error> {
        let now = mapping_now();
        let mapping = ProjectRepositoryMapping {
            id: ProjectRepositoryMappingId::new(),
            linear_organization_id: mapping.linear_organization_id,
            linear_team_id: mapping.linear_team_id,
            linear_project_id: mapping.linear_project_id,
            linear_project_name_snapshot: mapping.linear_project_name_snapshot,
            github_repository: mapping.github_repository,
            repository_source_path: mapping.repository_source_path,
            git_common_directory: mapping.git_common_directory,
            git_remote_url: mapping.git_remote_url,
            default_branch: mapping.default_branch,
            status: ProjectMappingStatus::Enabled,
            revision: ProjectMappingRevision::new(1).expect("a literal mapping revision is valid"),
            created_at: now,
            updated_at: now,
        };
        self.persist_project_mapping(mapping, None, actor, "created", reason)
            .await
    }

    async fn revise(
        &self,
        mapping: ProjectRepositoryMapping,
        expected_revision: ProjectMappingRevision,
        actor: &str,
        reason: &str,
    ) -> Result<ProjectRepositoryMapping, Self::Error> {
        self.persist_project_mapping(
            mapping,
            Some(expected_revision),
            actor,
            "revised",
            Some(reason),
        )
        .await
    }

    async fn disable(
        &self,
        id: ProjectRepositoryMappingId,
        expected_revision: ProjectMappingRevision,
        actor: &str,
        reason: &str,
    ) -> Result<ProjectRepositoryMapping, Self::Error> {
        self.change_project_mapping_status(
            id,
            expected_revision,
            ProjectMappingStatus::Disabled,
            actor,
            "disabled",
            reason,
        )
        .await
    }

    async fn tombstone(
        &self,
        id: ProjectRepositoryMappingId,
        expected_revision: ProjectMappingRevision,
        actor: &str,
        reason: &str,
    ) -> Result<ProjectRepositoryMapping, Self::Error> {
        self.change_project_mapping_status(
            id,
            expected_revision,
            ProjectMappingStatus::Removed,
            actor,
            "removed",
            reason,
        )
        .await
    }
}

impl SqliteDatabase {
    async fn change_project_mapping_status(
        &self,
        id: ProjectRepositoryMappingId,
        expected_revision: ProjectMappingRevision,
        status: ProjectMappingStatus,
        actor: &str,
        operation: &str,
        reason: &str,
    ) -> Result<ProjectRepositoryMapping, SqliteAdapterError> {
        let row = sqlx::query("SELECT * FROM project_repository_mappings WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?
            .ok_or(sqlx::Error::RowNotFound)?;
        let mut mapping = project_mapping_from_row(&row)?;
        mapping.status = status;
        self.persist_project_mapping(
            mapping,
            Some(expected_revision),
            actor,
            operation,
            Some(reason),
        )
        .await
    }

    async fn persist_project_mapping(
        &self,
        mut mapping: ProjectRepositoryMapping,
        expected_revision: Option<ProjectMappingRevision>,
        actor: &str,
        operation: &str,
        reason: Option<&str>,
    ) -> Result<ProjectRepositoryMapping, SqliteAdapterError> {
        let _write_gate = self.write_gate.lock().await;
        let mut connection = self.pool.acquire().await?;
        begin_immediate(&mut connection).await?;
        let result = async {
            let previous_revision = expected_revision.map(ProjectMappingRevision::value);
            if let Some(revision) = expected_revision {
                mapping.revision = revision.next();
                mapping.updated_at = mapping_now();
                let updated = sqlx::query(
                    "UPDATE project_repository_mappings SET linear_team_id = ?, linear_project_name_snapshot = ?, github_repository = ?, repository_source_path = ?, git_common_directory = ?, git_remote_url = ?, default_branch = ?, status = ?, revision = ?, updated_at = ? WHERE id = ? AND revision = ?",
                )
                .bind(&mapping.linear_team_id)
                .bind(&mapping.linear_project_name_snapshot)
                .bind(mapping.github_repository.as_str())
                .bind(&mapping.repository_source_path)
                .bind(&mapping.git_common_directory)
                .bind(&mapping.git_remote_url)
                .bind(&mapping.default_branch)
                .bind(mapping_status_name(mapping.status))
                .bind(mapping.revision.value() as i64)
                .bind(mapping.updated_at)
                .bind(mapping.id.to_string())
                .bind(revision.value() as i64)
                .execute(&mut *connection)
                .await?;
                if updated.rows_affected() != 1 {
                    return Err(sqlx::Error::RowNotFound);
                }
            } else {
                sqlx::query(
                    "INSERT INTO project_repository_mappings (id, linear_organization_id, linear_team_id, linear_project_id, linear_project_name_snapshot, github_repository, repository_source_path, git_common_directory, git_remote_url, default_branch, status, revision, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(mapping.id.to_string())
                .bind(&mapping.linear_organization_id)
                .bind(&mapping.linear_team_id)
                .bind(mapping.linear_project_id.as_str())
                .bind(&mapping.linear_project_name_snapshot)
                .bind(mapping.github_repository.as_str())
                .bind(&mapping.repository_source_path)
                .bind(&mapping.git_common_directory)
                .bind(&mapping.git_remote_url)
                .bind(&mapping.default_branch)
                .bind(mapping_status_name(mapping.status))
                .bind(mapping.revision.value() as i64)
                .bind(mapping.created_at)
                .bind(mapping.updated_at)
                .execute(&mut *connection)
                .await?;
            }
            let snapshot = serde_json::to_string(&mapping)
                .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
            sqlx::query(
                "INSERT INTO project_repository_mapping_history (mapping_id, actor, operation, previous_revision, new_revision, reason, mapping_snapshot_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(mapping.id.to_string())
            .bind(actor)
            .bind(operation)
            .bind(previous_revision.map(|value| value as i64))
            .bind(mapping.revision.value() as i64)
            .bind(reason)
            .bind(snapshot)
            .bind(mapping.updated_at)
            .execute(&mut *connection)
            .await?;
            Ok::<ProjectRepositoryMapping, sqlx::Error>(mapping)
        }
        .await;
        commit_result(&mut connection, result).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    static NEXT_TEST_DATABASE: AtomicUsize = AtomicUsize::new(0);

    async fn database() -> SqliteDatabase {
        let directory = std::env::temp_dir().join(format!(
            "spire-s02-{}-{}",
            std::process::id(),
            NEXT_TEST_DATABASE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).unwrap();
        SqliteDatabase::initialize(directory.join("spire.db"), 4)
            .await
            .unwrap()
    }

    async fn insert_work_item(database: &SqliteDatabase, id: &str) {
        sqlx::query("INSERT INTO work_items (id, state, revision, created_at, updated_at) VALUES (?, 'observed', 'initial', 1, 1)")
            .bind(id)
            .execute(database.pool())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn applies_required_connection_pragmas_and_migrations() {
        let database = database().await;
        assert_eq!(database.pragma("foreign_keys").await.unwrap(), "1");
        assert_eq!(database.pragma("busy_timeout").await.unwrap(), "5000");
        assert_eq!(
            database
                .pragma("journal_mode")
                .await
                .unwrap()
                .to_lowercase(),
            "wal"
        );
        database.check_integrity().await.unwrap();
    }

    #[tokio::test]
    async fn wal_keeps_readers_available_during_a_short_write_transaction() {
        let database = database().await;
        insert_work_item(&database, "work-1").await;
        let mut writer = database.pool().acquire().await.unwrap();
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *writer)
            .await
            .unwrap();
        sqlx::query("UPDATE work_items SET revision = 'next' WHERE id = 'work-1'")
            .execute(&mut *writer)
            .await
            .unwrap();
        let revision: String =
            sqlx::query_scalar("SELECT revision FROM work_items WHERE id = 'work-1'")
                .fetch_one(database.pool())
                .await
                .unwrap();
        assert_eq!(revision, "initial");
        sqlx::query("ROLLBACK").execute(&mut *writer).await.unwrap();
    }

    #[tokio::test]
    async fn duplicate_inbox_delivery_creates_one_row() {
        let database = database().await;
        let event = InboxEvent {
            id: "inbox-1",
            source: "linear",
            delivery_id: "delivery-1",
            event_type: "Issue",
            raw_headers: "{}",
            raw_body: b"{}",
        };
        assert!(database.insert_inbox(event.clone(), 1).await.unwrap());
        assert!(!database.insert_inbox(event, 2).await.unwrap());
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM webhook_inbox")
            .fetch_one(database.pool())
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn state_and_outbox_action_commit_atomically() {
        let database = database().await;
        database
            .insert_inbox(
                InboxEvent {
                    id: "inbox-1",
                    source: "linear",
                    delivery_id: "delivery-1",
                    event_type: "Issue",
                    raw_headers: "{}",
                    raw_body: b"{}",
                },
                1,
            )
            .await
            .unwrap();
        database
            .process_inbox_and_enqueue(
                "inbox-1",
                "work-1",
                OutboxAction {
                    id: "outbox-1",
                    kind: "post_comment",
                    aggregate_id: "work-1",
                    idempotency_key: "comment:work-1",
                    payload_json: "{}",
                },
                2,
            )
            .await
            .unwrap();
        let processed: String =
            sqlx::query_scalar("SELECT status FROM webhook_inbox WHERE id = 'inbox-1'")
                .fetch_one(database.pool())
                .await
                .unwrap();
        let outbox: i64 = sqlx::query_scalar("SELECT count(*) FROM outbox WHERE id = 'outbox-1'")
            .fetch_one(database.pool())
            .await
            .unwrap();
        assert_eq!(processed, "processed");
        assert_eq!(outbox, 1);
    }

    #[tokio::test]
    async fn active_run_and_lineage_constraints_fail_at_database_boundary() {
        let database = database().await;
        insert_work_item(&database, "work-1").await;
        insert_work_item(&database, "work-2").await;
        database
            .create_run(
                RunRecord {
                    id: "run-1",
                    work_item_id: "work-1",
                    parent_run_id: None,
                    root_run_id: "run-1",
                    role: "implementation",
                    harness: "codex",
                    model: "model",
                    effort: "medium",
                    status: "running",
                },
                1,
            )
            .await
            .unwrap();
        assert!(
            database
                .create_run(
                    RunRecord {
                        id: "run-2",
                        work_item_id: "work-1",
                        parent_run_id: None,
                        root_run_id: "run-2",
                        role: "implementation",
                        harness: "codex",
                        model: "model",
                        effort: "medium",
                        status: "queued"
                    },
                    2
                )
                .await
                .is_err()
        );
        assert!(
            database
                .create_run(
                    RunRecord {
                        id: "run-3",
                        work_item_id: "work-2",
                        parent_run_id: Some("run-1"),
                        root_run_id: "run-1",
                        role: "review",
                        harness: "claude-code",
                        model: "model",
                        effort: "medium",
                        status: "queued"
                    },
                    2
                )
                .await
                .is_err()
        );
        assert!(
            database
                .create_run(
                    RunRecord {
                        id: "run-4",
                        work_item_id: "work-2",
                        parent_run_id: None,
                        root_run_id: "run-1",
                        role: "implementation",
                        harness: "codex",
                        model: "model",
                        effort: "medium",
                        status: "succeeded"
                    },
                    2
                )
                .await
                .is_err()
        );
        assert!(
            sqlx::query("DELETE FROM work_items WHERE id = 'work-1'")
                .execute(database.pool())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn audit_constraints_reject_duplicates_and_mutation() {
        let database = database().await;
        insert_work_item(&database, "work-1").await;
        sqlx::query(
            "INSERT INTO provider_health (harness, model, credential_profile, state, updated_at) VALUES ('codex', 'model', 'default', 'healthy', 1)",
        )
        .execute(database.pool())
        .await
        .unwrap();
        assert!(sqlx::query(
            "INSERT INTO provider_health (harness, model, credential_profile, state, updated_at) VALUES ('codex', 'model', 'default', 'healthy', 2)",
        )
        .execute(database.pool())
        .await
        .is_err());
        sqlx::query(
            "INSERT INTO review_cycles (id, work_item_id, head_sha, ci_state, review_state, created_at, updated_at) VALUES ('review-1', 'work-1', 'sha', 'green', 'pending', 1, 1)",
        )
        .execute(database.pool())
        .await
        .unwrap();
        assert!(sqlx::query(
            "INSERT INTO review_cycles (id, work_item_id, head_sha, ci_state, review_state, created_at, updated_at) VALUES ('review-2', 'work-1', 'sha', 'green', 'pending', 2, 2)",
        )
        .execute(database.pool())
        .await
        .is_err());
        sqlx::query(
            "INSERT INTO dispatch_decisions (id, work_item_id, run_id, policy_version, rule_id, role, complexity_estimate, complexity_class, candidate_schema_version, candidates_json, candidate_evaluation_schema_version, candidate_evaluations_json, created_at) VALUES ('decision-1', 'work-1', 'run-1', 1, 'rule', 'implementation', 1, 'small', 1, '[]', 1, '[]', 1)",
        )
        .execute(database.pool())
        .await
        .unwrap();
        assert!(
            sqlx::query(
                "UPDATE dispatch_decisions SET rule_id = 'changed' WHERE id = 'decision-1'"
            )
            .execute(database.pool())
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn outbox_lease_is_exclusive_and_recoverable() {
        let database = database().await;
        database
            .insert_inbox(
                InboxEvent {
                    id: "inbox-1",
                    source: "linear",
                    delivery_id: "delivery-1",
                    event_type: "Issue",
                    raw_headers: "{}",
                    raw_body: b"{}",
                },
                1,
            )
            .await
            .unwrap();
        database
            .process_inbox_and_enqueue(
                "inbox-1",
                "work-1",
                OutboxAction {
                    id: "outbox-1",
                    kind: "post_comment",
                    aggregate_id: "work-1",
                    idempotency_key: "comment:work-1",
                    payload_json: "{}",
                },
                2,
            )
            .await
            .unwrap();
        assert!(
            database
                .claim_outbox("worker-1", 3, 10)
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            database
                .claim_outbox("worker-2", 3, 10)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            database
                .renew_outbox_lease("outbox-1", "worker-1", 3, 12)
                .await
                .unwrap()
        );
        assert_eq!(database.recover_expired_leases(11).await.unwrap(), 0);
        assert_eq!(database.recover_expired_leases(13).await.unwrap(), 1);
        assert!(
            database
                .claim_outbox("worker-2", 13, 20)
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            database
                .claim_outbox("worker-2", 11, 20)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            database
                .release_outbox_lease(
                    "outbox-1",
                    "worker-2",
                    21,
                    "transient",
                    "destination unavailable",
                    14,
                )
                .await
                .unwrap()
        );
        assert!(
            database
                .claim_outbox("worker-1", 21, 30)
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn inbox_lease_recovers_after_crash_and_quarantine_remains_inspectable() {
        let database = database().await;
        database
            .insert_inbox(
                InboxEvent {
                    id: "inbox-1",
                    source: "linear",
                    delivery_id: "delivery-1",
                    event_type: "Issue",
                    raw_headers: "{}",
                    raw_body: b"invalid",
                },
                1,
            )
            .await
            .unwrap();
        assert_eq!(
            database.claim_inbox("worker-1", 2, 10).await.unwrap(),
            Some("inbox-1".to_owned())
        );
        assert!(
            database
                .claim_inbox("worker-1", 3, 10)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(database.recover_expired_inbox_leases(11).await.unwrap(), 1);
        assert_eq!(
            database.claim_inbox("worker-2", 11, 20).await.unwrap(),
            Some("inbox-1".to_owned())
        );
        database
            .quarantine_inbox("inbox-1", "malformed payload", 12)
            .await
            .unwrap();
        let row: (String, String, Vec<u8>) = sqlx::query_as(
            "SELECT status, last_error, raw_body FROM webhook_inbox WHERE id = 'inbox-1'",
        )
        .fetch_one(database.pool())
        .await
        .unwrap();
        assert_eq!(row.0, "quarantined");
        assert_eq!(row.1, "malformed payload");
        assert_eq!(row.2, b"invalid");
    }

    #[tokio::test]
    async fn expired_run_lease_requires_recovery_instead_of_declaring_the_run_dead() {
        let database = database().await;
        insert_work_item(&database, "work-1").await;
        database
            .create_run(
                RunRecord {
                    id: "run-1",
                    work_item_id: "work-1",
                    parent_run_id: None,
                    root_run_id: "run-1",
                    role: "implementation",
                    harness: "codex",
                    model: "model",
                    effort: "medium",
                    status: "running",
                },
                1,
            )
            .await
            .unwrap();
        assert!(
            database
                .acquire_run_lease("run-1", "worker-1", 2, 10)
                .await
                .unwrap()
        );
        assert!(
            database
                .renew_run_lease("run-1", "worker-1", 3, 12)
                .await
                .unwrap()
        );
        assert_eq!(
            database.mark_expired_runs_for_recovery(11).await.unwrap(),
            0
        );
        assert_eq!(
            database.mark_expired_runs_for_recovery(13).await.unwrap(),
            1
        );
        let state: (String, i64) =
            sqlx::query_as("SELECT status, recovery_pending FROM runs WHERE id = 'run-1'")
                .fetch_one(database.pool())
                .await
                .unwrap();
        assert_eq!(state, ("running".to_owned(), 1));
        assert!(
            database
                .acquire_run_lease("run-1", "worker-2", 13, 20)
                .await
                .unwrap()
        );
        assert!(
            database
                .release_run_lease("run-1", "worker-2", 14)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn online_backup_restores_integrity_and_state() {
        let database = database().await;
        database
            .insert_inbox(
                InboxEvent {
                    id: "inbox-1",
                    source: "linear",
                    delivery_id: "delivery-1",
                    event_type: "Issue",
                    raw_headers: "{}",
                    raw_body: b"{}",
                },
                1,
            )
            .await
            .unwrap();
        let backup = database.path().parent().unwrap().join("backup.db");
        database.backup_to(&backup).await.unwrap();
        let restored = SqliteDatabase::initialize(&backup, 1).await.unwrap();
        restored.check_integrity().await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM webhook_inbox")
            .fetch_one(restored.pool())
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn older_linear_observation_cannot_overwrite_newer_or_active_work() {
        let database = database().await;
        let newer = LinearObservation {
            work_item_id: "work-1",
            linear_issue_id: "linear-1",
            linear_identifier: "SPI-1",
            team_id: "team",
            workflow_state_id: "ready",
            revision: "2026-01-02:hash",
            raw_estimate: Some(2),
            complexity_class: Some("medium"),
            eligibility_reason: None,
        };
        assert!(
            database
                .upsert_linear_observation(newer.clone(), 2)
                .await
                .unwrap()
        );
        let older = LinearObservation {
            revision: "2026-01-01:hash",
            raw_estimate: Some(1),
            ..newer.clone()
        };
        assert!(!database.upsert_linear_observation(older, 3).await.unwrap());
        let revision: String =
            sqlx::query_scalar("SELECT revision FROM work_items WHERE id = 'work-1'")
                .fetch_one(database.pool())
                .await
                .unwrap();
        assert_eq!(revision, "2026-01-02:hash");
        sqlx::query("UPDATE work_items SET state = 'implementing' WHERE id = 'work-1'")
            .execute(database.pool())
            .await
            .unwrap();
        let active = LinearObservation {
            revision: "2026-01-03:hash",
            ..newer
        };
        assert!(!database.upsert_linear_observation(active, 4).await.unwrap());
    }

    #[tokio::test]
    async fn root_claim_is_exactly_once_at_the_database_boundary() {
        let database = database().await;
        database
            .upsert_linear_observation(
                LinearObservation {
                    work_item_id: "work-1",
                    linear_issue_id: "linear-1",
                    linear_identifier: "SPI-1",
                    team_id: "team",
                    workflow_state_id: "ready",
                    revision: "revision",
                    raw_estimate: Some(2),
                    complexity_class: Some("medium"),
                    eligibility_reason: None,
                },
                1,
            )
            .await
            .unwrap();
        let limits = CapacityLimits {
            total: 1,
            ai: 1,
            per_repository: 1,
            per_ticket: 1,
        };
        let first = RootClaim {
            work_item_id: "work-1",
            expected_revision: "revision",
            repository: "owner/repo",
            run_id: "run-1",
            decision_id: "decision-1",
            raw_estimate: 2,
            complexity_class: "medium",
            implementation_rule_id: "implementation",
            review_rule_id: "review",
            policy_version: 1,
            candidate_json: "[]",
            evaluation_json: "[]",
            review_candidate_json: "[]",
            harness: "codex",
            model: "model",
            effort: "medium",
            initiator: SchedulerInitiator::Human,
            trigger_kind: "linear_ready",
        };
        let second = RootClaim {
            run_id: "run-2",
            decision_id: "decision-2",
            ..first.clone()
        };
        let left = database.clone();
        let right = database.clone();
        let (one, two) = tokio::join!(
            left.claim_root(first, limits, 2),
            right.claim_root(second, limits, 2)
        );
        assert!(
            matches!(one.unwrap(), RootClaimResult::Claimed)
                ^ matches!(two.unwrap(), RootClaimResult::Claimed)
        );
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM runs WHERE work_item_id = 'work-1'")
                .fetch_one(database.pool())
                .await
                .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn new_head_invalidates_old_ci_and_review_evidence() {
        use spire_application::{CanonicalPullRequest, PullRequestState, RequiredCheckGate};
        use spire_domain::{CommitSha, RepositoryName};

        let database = database().await;
        insert_work_item(&database, "work-1").await;
        sqlx::query("UPDATE work_items SET repository = 'owner/repo', current_head_sha = 'old', state = 'waiting_for_review' WHERE id = 'work-1'")
            .execute(database.pool()).await.unwrap();
        sqlx::query("INSERT INTO review_cycles (id, work_item_id, head_sha, ci_state, review_state, created_at, updated_at) VALUES ('cycle-old', 'work-1', 'old', 'succeeded', 'approved', 0, 0)")
            .execute(database.pool()).await.unwrap();
        let pr = CanonicalPullRequest {
            repository: RepositoryName::new("owner/repo").unwrap(),
            number: 7,
            url: "https://example.test/pr/7".into(),
            state: PullRequestState::Open,
            is_draft: true,
            base_branch: "main".into(),
            base_sha: CommitSha::new("base").unwrap(),
            head_branch: "spire/SPI-1".into(),
            head_sha: CommitSha::new("new").unwrap(),
            mergeable: Some(true),
            author: "maker".into(),
            updated_at_unix_seconds: 1,
        };
        assert_eq!(
            database
                .persist_canonical_pull_request("work-1", "owner/repo", "spire/SPI-1", &pr, 2)
                .await
                .unwrap(),
            PullRequestPersistence::Updated
        );
        let cycle: (String, String) = sqlx::query_as(
            "SELECT ci_state, review_state FROM review_cycles WHERE id = 'cycle-old'",
        )
        .fetch_one(database.pool())
        .await
        .unwrap();
        assert_eq!(cycle, ("invalidated".into(), "invalidated".into()));
        assert_eq!(
            database
                .persist_required_check_gate(
                    "work-1",
                    "old",
                    &RequiredCheckGate::Pending {
                        missing: vec!["test".into()]
                    },
                    3
                )
                .await
                .unwrap(),
            CiGatePersistence::StaleHead
        );
    }

    #[tokio::test]
    async fn ci_correction_reuses_the_original_maker_and_is_bounded() {
        let database = database().await;
        insert_work_item(&database, "work-1").await;
        sqlx::query("UPDATE work_items SET repository = 'owner/repo', state = 'waiting_for_ci' WHERE id = 'work-1'").execute(database.pool()).await.unwrap();
        database
            .create_run(
                RunRecord {
                    id: "maker",
                    work_item_id: "work-1",
                    parent_run_id: None,
                    root_run_id: "maker",
                    role: "implementation",
                    harness: "codex",
                    model: "model",
                    effort: "medium",
                    status: "succeeded",
                },
                1,
            )
            .await
            .unwrap();
        let limits = CapacityLimits {
            total: 3,
            ai: 1,
            per_repository: 1,
            per_ticket: 1,
        };
        assert_eq!(
            database
                .schedule_ci_correction("work-1", "ci-1", limits, 2)
                .await
                .unwrap(),
            CiCorrectionPersistence::Scheduled { cycle: 1 }
        );
        let run: (String, String, String, String) =
            sqlx::query_as("SELECT harness, model, effort, initiator FROM runs WHERE id = 'ci-1'")
                .fetch_one(database.pool())
                .await
                .unwrap();
        assert_eq!(
            run,
            ("codex".into(), "model".into(), "medium".into(), "ai".into())
        );
    }

    #[tokio::test]
    async fn review_is_deduplicated_sha_bound_and_routes_to_the_sticky_maker() {
        use spire_application::{FindingSeverity, ReviewFinding, ReviewResult, ReviewVerdict};
        use spire_domain::CommitSha;

        let database = database().await;
        insert_work_item(&database, "work-1").await;
        sqlx::query("UPDATE work_items SET linear_issue_id = 'issue-1', repository = 'owner/repo', base_sha = 'base', current_head_sha = 'head', state = 'waiting_for_review', review_candidates_json = ?, review_policy_version = 7, review_rule_id = 'review-snapshot' WHERE id = 'work-1'")
            .bind(r#"[{"harness":"claude-code","model":"review-model","effort":"high"}]"#)
            .execute(database.pool()).await.unwrap();
        database
            .create_run(
                RunRecord {
                    id: "maker",
                    work_item_id: "work-1",
                    parent_run_id: None,
                    root_run_id: "maker",
                    role: "implementation",
                    harness: "codex",
                    model: "maker-model",
                    effort: "medium",
                    status: "succeeded",
                },
                1,
            )
            .await
            .unwrap();
        sqlx::query("INSERT INTO dispatch_decisions (id, work_item_id, run_id, policy_version, rule_id, role, complexity_estimate, complexity_class, candidate_schema_version, candidates_json, selected_candidate_index, candidate_evaluation_schema_version, candidate_evaluations_json, created_at) VALUES ('maker-decision', 'work-1', 'maker', 7, 'implementation-snapshot', 'implementation', 2, 'medium', 1, '[]', 0, 1, '[]', 1)")
            .execute(database.pool()).await.unwrap();
        let limits = CapacityLimits {
            total: 3,
            ai: 1,
            per_repository: 1,
            per_ticket: 1,
        };
        let dispatch = ReviewDispatch {
            work_item_id: "work-1",
            run_id: "review-1",
            review_cycle_id: "cycle-1",
            decision_id: "review-decision",
            evaluation_json: "[]",
            selected_candidate_index: 0,
            harness: "claude-code",
            model: "review-model",
            effort: "high",
        };
        assert_eq!(
            database
                .schedule_review(dispatch.clone(), limits, 2)
                .await
                .unwrap(),
            ReviewDispatchPersistence::Scheduled { round: 1 }
        );
        assert_eq!(
            database.schedule_review(dispatch, limits, 3).await.unwrap(),
            ReviewDispatchPersistence::AlreadyScheduled
        );
        let review_decision: (i64, String, String) = sqlx::query_as("SELECT policy_version, rule_id, candidates_json FROM dispatch_decisions WHERE id = 'review-decision'").fetch_one(database.pool()).await.unwrap();
        assert_eq!(review_decision.0, 7);
        assert_eq!(review_decision.1, "review-snapshot");
        assert!(review_decision.2.contains("claude-code"));

        let changes = ReviewResult {
            schema_version: 1,
            verdict: ReviewVerdict::ChangesRequired,
            reviewed_head_sha: CommitSha::new("head").unwrap(),
            summary: "Fix this".into(),
            findings: vec![ReviewFinding {
                stable_id: "SPI-1".into(),
                severity: FindingSeverity::High,
                file: "src/lib.rs".into(),
                line: Some(1),
                title: "Incorrect result".into(),
                rationale: "The result is wrong".into(),
                requested_change: "Return the correct result".into(),
            }],
        };
        assert_eq!(
            database
                .record_review_result("work-1", "cycle-1", "review-1", &changes, 4)
                .await
                .unwrap(),
            ReviewResultPersistence::Applied {
                verdict: ReviewVerdict::ChangesRequired
            }
        );
        let review_actions: i64 =
            sqlx::query_scalar("SELECT count(*) FROM outbox WHERE aggregate_id = 'work-1'")
                .fetch_one(database.pool())
                .await
                .unwrap();
        assert_eq!(review_actions, 2);
        assert_eq!(
            database
                .schedule_review_correction("work-1", "fix-1", limits, 5)
                .await
                .unwrap(),
            ReviewCorrectionPersistence::Scheduled { cycle: 1 }
        );
        let correction: (String, String, String) =
            sqlx::query_as("SELECT harness, initiator, trigger_kind FROM runs WHERE id = 'fix-1'")
                .fetch_one(database.pool())
                .await
                .unwrap();
        assert_eq!(
            correction,
            (
                "codex".into(),
                "ai".into(),
                "review_changes_requested".into()
            )
        );
    }

    #[tokio::test]
    async fn stale_review_and_unauthorized_waiver_cannot_make_a_pr_human_ready() {
        use spire_application::{ReviewResult, ReviewVerdict};
        use spire_domain::CommitSha;

        let database = database().await;
        insert_work_item(&database, "work-1").await;
        sqlx::query("UPDATE work_items SET current_head_sha = 'new', state = 'waiting_for_review' WHERE id = 'work-1'").execute(database.pool()).await.unwrap();
        sqlx::query("INSERT INTO review_cycles (id, work_item_id, head_sha, ci_state, review_state, created_at, updated_at) VALUES ('cycle-old', 'work-1', 'old', 'succeeded', 'running', 1, 1)").execute(database.pool()).await.unwrap();
        database
            .create_run(
                RunRecord {
                    id: "review-old",
                    work_item_id: "work-1",
                    parent_run_id: None,
                    root_run_id: "review-old",
                    role: "review",
                    harness: "claude-code",
                    model: "model",
                    effort: "high",
                    status: "running",
                },
                1,
            )
            .await
            .unwrap();
        sqlx::query("UPDATE review_cycles SET review_run_id = 'review-old' WHERE id = 'cycle-old'")
            .execute(database.pool())
            .await
            .unwrap();
        let approval = ReviewResult {
            schema_version: 1,
            verdict: ReviewVerdict::Approved,
            reviewed_head_sha: CommitSha::new("old").unwrap(),
            summary: "Approved".into(),
            findings: vec![],
        };
        assert_eq!(
            database
                .record_review_result("work-1", "cycle-old", "review-old", &approval, 2)
                .await
                .unwrap(),
            ReviewResultPersistence::StaleHead
        );
        assert_eq!(
            database
                .apply_review_waiver("work-1", "new", "", "no", false, 3)
                .await
                .unwrap(),
            ReviewWaiverPersistence::Unauthorized
        );
        let state: String = sqlx::query_scalar("SELECT state FROM work_items WHERE id = 'work-1'")
            .fetch_one(database.pool())
            .await
            .unwrap();
        assert_eq!(state, "waiting_for_review");
    }

    #[tokio::test]
    async fn project_mappings_are_revisioned_auditable_and_fail_closed() {
        let database = database().await;
        let mapping = database
            .create(
                NewProjectRepositoryMapping {
                    linear_organization_id: "organization".into(),
                    linear_team_id: "team".into(),
                    linear_project_id: LinearProjectId::new("project").unwrap(),
                    linear_project_name_snapshot: "Project".into(),
                    github_repository: RepositoryName::new("owner/repository").unwrap(),
                    repository_source_path: "/repositories/source".into(),
                    git_common_directory: "/repositories/source/.git".into(),
                    git_remote_url: "git@github.com:owner/repository.git".into(),
                    default_branch: "main".into(),
                },
                "operator",
                Some("initial mapping"),
            )
            .await
            .unwrap();
        assert!(matches!(
            database
                .resolve("organization", &LinearProjectId::new("project").unwrap())
                .await
                .unwrap(),
            ProjectRoutingDecision::Mapped { .. }
        ));

        let disabled = database
            .disable(mapping.id, mapping.revision, "operator", "maintenance")
            .await
            .unwrap();
        assert_eq!(disabled.revision, ProjectMappingRevision::new(2).unwrap());
        assert_eq!(
            database
                .resolve("organization", &LinearProjectId::new("project").unwrap())
                .await
                .unwrap(),
            ProjectRoutingDecision::MappingDisabled
        );
        assert!(
            database
                .disable(mapping.id, mapping.revision, "operator", "stale update")
                .await
                .is_err()
        );
        let history: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM project_repository_mapping_history WHERE mapping_id = ?",
        )
        .bind(mapping.id.to_string())
        .fetch_one(database.pool())
        .await
        .unwrap();
        assert_eq!(history, 2);
    }
}
