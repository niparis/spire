use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
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

fn is_busy(error: &sqlx::Error) -> bool {
    error.as_database_error().is_some_and(|database| {
        matches!(database.code().as_deref(), Some("5") | Some("SQLITE_BUSY"))
    })
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
    #[error("migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
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
}
