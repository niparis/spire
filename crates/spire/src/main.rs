#![forbid(unsafe_code)]

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use axum::{
    Router,
    body::{Bytes, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
};
use clap::{Parser, Subcommand};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use spire_adapters::{
    github::{GitHubHttpAdapter, GitHubReconciler},
    linear::{LinearReadAdapter, load_credential},
    sqlite::{InboxEvent, LinearObservation, SqliteDatabase},
};
use spire_application::{
    CanonicalLinearIssue, Config, EligibilityInput, ExternalResult, LinearReadPort,
    RelevantIssueQuery, ValidatedConfig, WebhookAllowlist, WebhookRequest, accept_delivery,
    dispatch_is_covered, evaluate_eligibility,
};
use spire_domain::{ComplexityClass, HarnessId, LinearIssueId, RunRole};
use tokio::net::TcpListener;
use tracing::info;
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(
    name = "spire",
    about = "Single-node Code Harness orchestrator",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Dispatch {
        #[command(subcommand)]
        command: DispatchCommand,
    },
    Db {
        #[command(subcommand)]
        command: DbCommand,
    },
    Ops {
        #[command(subcommand)]
        command: OpsCommand,
    },
    Linear {
        #[command(subcommand)]
        command: LinearCommand,
    },
    GitHub {
        #[command(subcommand)]
        command: GitHubCommand,
    },
    Scheduler {
        #[command(subcommand)]
        command: SchedulerCommand,
    },
    Runs {
        #[command(subcommand)]
        command: RunsCommand,
    },
    Serve {
        #[arg(long)]
        config: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    Validate {
        #[arg(long)]
        config: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum DispatchCommand {
    DryRun {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        maker_harness: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum DbCommand {
    Backup {
        #[arg(long)]
        database: PathBuf,
        #[arg(long)]
        destination: PathBuf,
    },
    Check {
        #[arg(long)]
        database: PathBuf,
    },
    BackupDaily {
        #[arg(long)]
        config: PathBuf,
    },
    RestoreCheck {
        #[arg(long)]
        backup: PathBuf,
        #[arg(long)]
        destination: PathBuf,
    },
    RestoreLatest {
        #[arg(long)]
        config: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum OpsCommand {
    Status {
        #[arg(long)]
        config: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum LinearCommand {
    Get {
        #[arg(long)]
        config: PathBuf,
        issue: String,
    },
    Reconcile {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        dry_run: bool,
    },
    Explain {
        #[arg(long)]
        config: PathBuf,
        issue: String,
    },
}

#[derive(Debug, Subcommand)]
enum GitHubCommand {
    Reconcile {
        #[arg(long)]
        config: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum SchedulerCommand {
    Once {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        dry_run: bool,
    },
    Explain {
        #[arg(long)]
        config: PathBuf,
        issue: String,
    },
    CapacityShow {
        #[arg(long)]
        config: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum RunsCommand {
    StartManual {
        fixture_ticket: String,
        #[arg(long)]
        dry_linear: bool,
        #[arg(long)]
        dry_github: bool,
    },
}

#[derive(Clone)]
struct Readiness {
    configuration_valid: bool,
    database: Option<SqliteDatabase>,
    github: Option<GitHubHttpAdapter>,
    github_webhook_secret: Option<Vec<u8>>,
    github_repositories: BTreeSet<String>,
}

#[derive(Clone)]
struct WebhookState {
    database: SqliteDatabase,
    path: String,
    organization_id: String,
    webhook_id: String,
    limits: spire_application::WebhookLimits,
    signing_secret: Arc<[u8]>,
}

#[derive(Clone)]
struct PublicState {
    readiness: Readiness,
    webhook: WebhookState,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();

    match Cli::parse().command {
        Command::Config {
            command: ConfigCommand::Validate { config },
        } => {
            load_config(config)?;
            println!("configuration is valid");
        }
        Command::Dispatch {
            command:
                DispatchCommand::DryRun {
                    config,
                    maker_harness,
                },
        } => dispatch_dry_run(load_config(config)?, maker_harness)?,
        Command::Db {
            command:
                DbCommand::Backup {
                    database,
                    destination,
                },
        } => {
            let database = SqliteDatabase::initialize(database, 4).await?;
            database.backup_to(destination).await?;
            println!("database backup completed");
        }
        Command::Db {
            command: DbCommand::Check { database },
        } => {
            SqliteDatabase::initialize(database, 4)
                .await?
                .check_integrity()
                .await?;
            println!("database integrity check passed");
        }
        Command::Db {
            command: DbCommand::BackupDaily { config },
        } => backup_daily(load_config(config)?).await?,
        Command::Db {
            command:
                DbCommand::RestoreCheck {
                    backup,
                    destination,
                },
        } => restore_check(backup, destination).await?,
        Command::Db {
            command: DbCommand::RestoreLatest { config },
        } => restore_latest(load_config(config)?).await?,
        Command::Ops {
            command: OpsCommand::Status { config },
        } => operations_status(load_config(config)?).await?,
        Command::Linear {
            command: LinearCommand::Get { config, issue },
        } => {
            let config = load_config(config)?;
            let issue = LinearIssueId::new(issue).context("invalid Linear issue ID")?;
            let adapter = linear_adapter(&config)?;
            match adapter.get_canonical_issue(&issue).await? {
                ExternalResult::Confirmed(issue) => print_json(&issue)?,
                ExternalResult::NotFound => anyhow::bail!("Linear issue was not found"),
                ExternalResult::Ambiguous { detail } => {
                    anyhow::bail!("ambiguous Linear response: {detail}")
                }
            }
        }
        Command::Linear {
            command: LinearCommand::Explain { config, issue },
        } => {
            let config = load_config(config)?;
            let issue_id = LinearIssueId::new(issue).context("invalid Linear issue ID")?;
            let adapter = linear_adapter(&config)?;
            match adapter.get_canonical_issue(&issue_id).await? {
                ExternalResult::Confirmed(issue) => print_json(&explain_issue(&config, &issue))?,
                ExternalResult::NotFound => anyhow::bail!("Linear issue was not found"),
                ExternalResult::Ambiguous { detail } => {
                    anyhow::bail!("ambiguous Linear response: {detail}")
                }
            }
        }
        Command::Linear {
            command: LinearCommand::Reconcile { config, dry_run },
        } => {
            if !dry_run {
                anyhow::bail!("Linear reconciliation is read-only in Sprint 03; pass --dry-run");
            }
            linear_reconcile(load_config(config)?).await?;
        }
        Command::GitHub {
            command: GitHubCommand::Reconcile { config },
        } => github_reconcile(load_config(config)?).await?,
        Command::Scheduler {
            command: SchedulerCommand::Once { config, dry_run },
        } => {
            if !dry_run {
                anyhow::bail!(
                    "scheduler dispatch remains dry-run-only in Sprint 04; pass --dry-run"
                );
            }
            let config = load_config(config)?;
            let database = SqliteDatabase::initialize(
                &config.config.runtime.database_path,
                config.config.runtime.database_max_connections,
            )
            .await?;
            let (total, ai) = database.capacity_counts().await?;
            print_json(
                &serde_json::json!({"dry_run": true, "claim": "not_started", "active_total": total, "active_ai": ai}),
            )?;
        }
        Command::Scheduler {
            command: SchedulerCommand::CapacityShow { config },
        } => {
            let config = load_config(config)?;
            let database = SqliteDatabase::initialize(
                &config.config.runtime.database_path,
                config.config.runtime.database_max_connections,
            )
            .await?;
            let (total, ai) = database.capacity_counts().await?;
            print_json(
                &serde_json::json!({"active_total": total, "active_ai": ai, "limits": {"total": config.config.concurrency.total_active_harness_runs, "ai": config.config.concurrency.ai_initiated_active_harness_runs}}),
            )?;
        }
        Command::Scheduler {
            command: SchedulerCommand::Explain { config, issue },
        } => {
            let config = load_config(config)?;
            let database = SqliteDatabase::initialize(
                &config.config.runtime.database_path,
                config.config.runtime.database_max_connections,
            )
            .await?;
            let (total, ai) = database.capacity_counts().await?;
            print_json(
                &serde_json::json!({"issue": issue, "active_total": total, "active_ai": ai, "claim": "requires canonical reconciliation and the atomic claim path", "linear_writes_enabled": false}),
            )?;
        }
        Command::Runs {
            command:
                RunsCommand::StartManual {
                    fixture_ticket,
                    dry_linear,
                    dry_github,
                },
        } => {
            if !dry_linear || !dry_github {
                anyhow::bail!(
                    "manual harness starts require --dry-linear and --dry-github until write authority is approved"
                );
            }
            print_json(
                &serde_json::json!({"fixture_ticket": fixture_ticket, "dry_linear": true, "dry_github": true, "runner": "disabled_pending_captured_provider_fixtures", "started": false}),
            )?;
        }
        Command::Serve { config } => serve(load_config(config)?).await?,
    }
    Ok(())
}

fn linear_adapter(config: &ValidatedConfig) -> Result<LinearReadAdapter> {
    LinearReadAdapter::from_credential_reference(&config.config.linear.credential_ref)
        .context("failed to construct read-only Linear adapter")
}

fn explain_issue(config: &ValidatedConfig, issue: &CanonicalLinearIssue) -> serde_json::Value {
    let supported_types = config
        .config
        .linear
        .supported_type_labels
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let complexity_mapping = config
        .config
        .linear
        .complexity_mapping
        .iter()
        .map(|(estimate, class)| (estimate.value(), *class))
        .collect::<BTreeMap<_, _>>();
    let eligibility = evaluate_eligibility(EligibilityInput {
        issue,
        ready_state_id: &config.config.linear.ready_state_id,
        supported_type_labels: &supported_types,
        repository_mappings: &config.config.linear.repository_mappings,
        complexity_mapping: &complexity_mapping,
        incomplete_blockers: &BTreeSet::new(),
        locally_active: false,
        locally_terminal: false,
        dispatch_covers_implementation_and_review: issue
            .estimate
            .and_then(|value| complexity_mapping.get(&value).copied())
            .is_some_and(|class| dispatch_is_covered(&config.policy, &config.capabilities, class)),
    });
    serde_json::json!({"issue": issue, "eligibility": eligibility, "linear_writes_enabled": false})
}

async fn linear_reconcile(config: ValidatedConfig) -> Result<()> {
    let adapter = linear_adapter(&config)?;
    let database = SqliteDatabase::initialize(
        &config.config.runtime.database_path,
        config.config.runtime.database_max_connections,
    )
    .await?;
    let mut cursor = None;
    let mut reports = Vec::new();
    loop {
        let page = match adapter
            .find_canonical_issues(&RelevantIssueQuery {
                team_id: config.config.linear.team_id.clone(),
                cursor: cursor.clone(),
                workflow_state_ids: vec![config.config.linear.ready_state_id.clone()],
            })
            .await?
        {
            ExternalResult::Confirmed(page) => page,
            ExternalResult::NotFound => break,
            ExternalResult::Ambiguous { detail } => {
                anyhow::bail!("ambiguous Linear response: {detail}")
            }
        };
        for issue in page.issues {
            let report = explain_issue(&config, &issue);
            let eligibility = report.get("eligibility").cloned().unwrap_or_default();
            let complexity = eligibility
                .get("complexity")
                .and_then(serde_json::Value::as_str);
            let reason = eligibility
                .get("reason")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            let work_item_id = format!("linear:{}", issue.id);
            database
                .upsert_linear_observation(
                    LinearObservation {
                        work_item_id: &work_item_id,
                        linear_issue_id: issue.id.as_str(),
                        linear_identifier: &issue.identifier,
                        team_id: &issue.team_id,
                        workflow_state_id: &issue.workflow_state_id,
                        revision: &issue.revision,
                        raw_estimate: issue.estimate,
                        complexity_class: complexity,
                        eligibility_reason: reason.as_deref(),
                    },
                    0,
                )
                .await?;
            reports.push(report);
        }
        let Some(next) = page.next_cursor else { break };
        cursor = Some(next);
    }
    print_json(
        &serde_json::json!({"dry_run": true, "linear_writes_enabled": false, "reports": reports, "health": adapter.health()}),
    )
}

async fn github_reconcile(config: ValidatedConfig) -> Result<()> {
    let database = SqliteDatabase::initialize(
        &config.config.runtime.database_path,
        config.config.runtime.database_max_connections,
    )
    .await?;
    let github = github_adapter(&config)?;
    let report = GitHubReconciler::new(&database, &github)
        .reconcile_active_pull_requests(unix_now())
        .await?;
    print_json(&report)
}

async fn operations_status(config: ValidatedConfig) -> Result<()> {
    let database = SqliteDatabase::initialize(
        &config.config.runtime.database_path,
        config.config.runtime.database_max_connections,
    )
    .await?;
    let integrity = database.check_integrity().await.is_ok();
    let workspace_root_healthy = config.config.runtime.workspace_root.is_dir();
    print_json(&serde_json::json!({
        "snapshot": database.operations_snapshot().await?,
        "database_integrity": integrity,
        "workspace_root_healthy": workspace_root_healthy,
        "guard_thresholds": {
            "minimum_free_disk_bytes": config.config.operations.minimum_free_disk_bytes,
            "minimum_free_inodes": config.config.operations.minimum_free_inodes,
        },
        "host_disk_probe": "requires the systemd/host probe; admission remains fail-closed when unavailable",
    }))
}

async fn backup_daily(config: ValidatedConfig) -> Result<()> {
    let root = &config.config.runtime.backup_root;
    fs::create_dir_all(root)
        .with_context(|| format!("failed to create backup root {}", root.display()))?;
    let destination = root.join(format!("spire-{}.db", unix_now()));
    let database = SqliteDatabase::initialize(
        &config.config.runtime.database_path,
        config.config.runtime.database_max_connections,
    )
    .await?;
    database.backup_to(&destination).await?;
    prune_dated_backups(root, config.config.operations.backup_retention_count)?;
    print_json(
        &serde_json::json!({"backup": destination, "retained": config.config.operations.backup_retention_count}),
    )
}

async fn restore_check(backup: PathBuf, destination: PathBuf) -> Result<()> {
    if destination.exists() {
        anyhow::bail!(
            "restore destination already exists: {}",
            destination.display()
        );
    }
    let parent = destination
        .parent()
        .context("restore destination must have a parent directory")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create restore directory {}", parent.display()))?;
    fs::copy(&backup, &destination).with_context(|| {
        format!(
            "failed to copy backup {} to {}",
            backup.display(),
            destination.display()
        )
    })?;
    let restored = SqliteDatabase::initialize(&destination, 1).await?;
    restored.check_integrity().await?;
    print_json(
        &serde_json::json!({"restore_check": "passed", "destination": destination, "snapshot": restored.operations_snapshot().await?}),
    )
}

async fn restore_latest(config: ValidatedConfig) -> Result<()> {
    let backup = dated_backups(&config.config.runtime.backup_root)?
        .pop()
        .context("no dated Spire backup is available for a restore drill")?;
    let destination = config
        .config
        .runtime
        .data_root
        .join("restore-drills")
        .join(format!("spire-{}.db", unix_now()));
    restore_check(backup.path(), destination).await
}

fn prune_dated_backups(root: &std::path::Path, retain: u16) -> Result<()> {
    let mut backups = dated_backups(root)?;
    let expired = backups.len().saturating_sub(usize::from(retain));
    for entry in backups.drain(..expired) {
        fs::remove_file(entry.path()).with_context(|| {
            format!("failed to remove expired backup {}", entry.path().display())
        })?;
    }
    Ok(())
}

fn dated_backups(root: &std::path::Path) -> Result<Vec<fs::DirEntry>> {
    let mut backups = fs::read_dir(root)
        .with_context(|| format!("failed to read backup root {}", root.display()))?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            let name = entry.file_name();
            let name = name.to_str()?;
            (file_type.is_file() && name.starts_with("spire-") && name.ends_with(".db"))
                .then_some(entry)
        })
        .collect::<Vec<_>>();
    backups.sort_by_key(|entry| entry.file_name());
    Ok(backups)
}

fn github_adapter(config: &ValidatedConfig) -> Result<GitHubHttpAdapter> {
    let repositories = config
        .config
        .github
        .repositories
        .iter()
        .map(|entry| spire_domain::RepositoryName::new(entry.repository.clone()))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let token = load_credential(&config.config.github.credential_ref)
        .context("failed to load GitHub credential")?;
    Ok(GitHubHttpAdapter::new(
        token,
        repositories,
        Duration::from_secs(config.config.github.request_timeout_seconds),
    )?)
}

fn print_json(value: &impl serde::Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn load_config(path: PathBuf) -> Result<ValidatedConfig> {
    Config::from_path(&path)
        .with_context(|| format!("failed to load configuration {}", path.display()))?
        .validate()
        .context("configuration validation failed")
}

fn dispatch_dry_run(config: ValidatedConfig, maker_harness: Option<String>) -> Result<()> {
    let maker = maker_harness
        .as_deref()
        .map(HarnessId::new)
        .transpose()
        .context("invalid --maker-harness")?;
    let mut evaluations = Vec::new();
    for complexity in ComplexityClass::ALL {
        for role in [RunRole::Implementation, RunRole::Review] {
            evaluations.push(
                config
                    .policy
                    .evaluate(&config.capabilities, role, complexity, &[], maker.as_ref())
                    .context("dispatch evaluation failed")?,
            );
        }
    }
    println!("{}", serde_json::to_string_pretty(&evaluations)?);
    Ok(())
}

async fn serve(config: ValidatedConfig) -> Result<()> {
    let signing_secret = load_credential(&config.config.webhook.signing_secret_ref)
        .context("failed to load Linear webhook signing secret")?;
    let database = SqliteDatabase::initialize(
        &config.config.runtime.database_path,
        config.config.runtime.database_max_connections,
    )
    .await?;
    let webhook_state = WebhookState {
        database: database.clone(),
        path: config.config.webhook.path.clone(),
        organization_id: config.config.linear.organization_id.clone(),
        webhook_id: config.config.webhook.webhook_id.clone(),
        limits: config.webhook_limits(),
        signing_secret: Arc::from(signing_secret.into_bytes()),
    };
    let readiness = Readiness {
        configuration_valid: true,
        database: Some(database.clone()),
        github: Some(github_adapter(&config)?),
        github_webhook_secret: Some(
            load_credential(&config.config.github.webhook_secret_ref)
                .context("failed to load GitHub webhook signing secret")?
                .into_bytes(),
        ),
        github_repositories: config
            .config
            .github
            .repositories
            .iter()
            .map(|entry| entry.repository.clone())
            .collect(),
    };
    spawn_github_reconciliation(database, readiness.github.clone());
    let api = TcpListener::bind(config.config.server.api_bind)
        .await
        .context("failed to bind API listener")?;
    let admin = TcpListener::bind(config.config.server.admin_bind)
        .await
        .context("failed to bind loopback admin listener")?;
    info!(api = %config.config.server.api_bind, admin = %config.config.server.admin_bind, "starting Spire foundation service");

    let api_server = axum::serve(api, public_router(readiness.clone(), webhook_state))
        .with_graceful_shutdown(shutdown_signal());
    let admin_server =
        axum::serve(admin, health_router(readiness)).with_graceful_shutdown(shutdown_signal());
    tokio::try_join!(api_server, admin_server).context("health server stopped unexpectedly")?;
    Ok(())
}

fn public_router(readiness: Readiness, webhook_state: WebhookState) -> Router {
    Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(public_ready))
        .route(&webhook_state.path, post(linear_webhook))
        .route("/webhooks/github", post(github_webhook))
        .layer(middleware::from_fn(request_id))
        .with_state(PublicState {
            readiness,
            webhook: webhook_state,
        })
}

fn spawn_github_reconciliation(database: SqliteDatabase, github: Option<GitHubHttpAdapter>) {
    let Some(github) = github else { return };
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(300));
        loop {
            ticker.tick().await;
            if let Err(error) = GitHubReconciler::new(&database, &github)
                .reconcile_active_pull_requests(unix_now())
                .await
            {
                tracing::warn!(error = %error, "GitHub active-PR reconciliation failed");
            }
        }
    });
}

fn health_router(readiness: Readiness) -> Router {
    Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
        .route("/admin/operations", get(operations))
        .layer(middleware::from_fn(request_id))
        .with_state(readiness)
}

async fn operations(
    State(readiness): State<Readiness>,
) -> std::result::Result<axum::Json<spire_application::OperationsSnapshot>, StatusCode> {
    let database = readiness.database.ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    database
        .operations_snapshot()
        .await
        .map(axum::Json)
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)
}

/// The public handler is intentionally limited to raw-body verification and
/// durable inbox insertion. Canonical Linear reads, lifecycle decisions, and
/// all harness work happen asynchronously after this `200` response.
async fn public_ready(State(state): State<PublicState>) -> StatusCode {
    if state.readiness.configuration_valid {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

async fn linear_webhook(State(public): State<PublicState>, request: Request) -> StatusCode {
    let state = public.webhook;
    let method = request.method().as_str().to_owned();
    let path = request.uri().path().to_owned();
    let content_type = request
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let signature = request
        .headers()
        .get(spire_application::SIGNATURE_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let delivery_id = request
        .headers()
        .get(spire_application::DELIVERY_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let declared_length = request
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok());
    let body = match to_bytes(request.into_body(), state.limits.max_body_bytes).await {
        Ok(body) => body,
        Err(_) => return StatusCode::PAYLOAD_TOO_LARGE,
    };
    let accepted = match accept_delivery(
        &WebhookRequest {
            method: &method,
            path: &path,
            content_type: content_type.as_deref(),
            signature: signature.as_deref(),
            delivery_id: delivery_id.as_deref(),
            declared_length,
            body: &body,
        },
        &state.path,
        state.limits,
        &state.signing_secret,
        WebhookAllowlist {
            organization_id: &state.organization_id,
            webhook_id: &state.webhook_id,
        },
        unix_now(),
    ) {
        Ok(accepted) => accepted,
        Err(rejection) if rejection.is_authentication_failure() => {
            tracing::warn!(reason = rejection.reason(), "Linear webhook rejected");
            return StatusCode::UNAUTHORIZED;
        }
        Err(rejection) => {
            tracing::warn!(reason = rejection.reason(), "Linear webhook rejected");
            return match rejection {
                spire_application::WebhookRejection::MethodNotAllowed => {
                    StatusCode::METHOD_NOT_ALLOWED
                }
                spire_application::WebhookRejection::PathNotFound => StatusCode::NOT_FOUND,
                spire_application::WebhookRejection::UnsupportedMediaType => {
                    StatusCode::UNSUPPORTED_MEDIA_TYPE
                }
                spire_application::WebhookRejection::BodyTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
                _ => StatusCode::BAD_REQUEST,
            };
        }
    };
    let headers = match serde_json::to_string(&accepted.redacted_headers) {
        Ok(headers) => headers,
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE,
    };
    let inbox_id = Uuid::new_v4().to_string();
    let inserted = state
        .database
        .insert_inbox(
            InboxEvent {
                id: &inbox_id,
                source: "linear",
                delivery_id: &accepted.envelope.delivery_id,
                event_type: &accepted.envelope.event_type,
                raw_headers: &headers,
                raw_body: &body,
            },
            unix_now(),
        )
        .await;
    match inserted {
        Ok(_) => StatusCode::OK,
        Err(error) => {
            tracing::error!(error = %error, "Linear webhook inbox insert failed");
            StatusCode::SERVICE_UNAVAILABLE
        }
    }
}

/// Verifies GitHub's HMAC over the unmodified body before durable, idempotent
/// receipt. No provider request or lifecycle transition occurs in this path.
async fn github_webhook(
    State(public): State<PublicState>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    let readiness = public.readiness;
    let Some(secret) = readiness.github_webhook_secret.as_deref() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let Some(signature) = headers
        .get("x-hub-signature-256")
        .and_then(|value| value.to_str().ok())
    else {
        return StatusCode::UNAUTHORIZED;
    };
    if !verify_github_signature(secret, signature, &body) {
        return StatusCode::UNAUTHORIZED;
    }
    let Some(delivery_id) = headers
        .get("x-github-delivery")
        .and_then(|value| value.to_str().ok())
    else {
        return StatusCode::BAD_REQUEST;
    };
    let Some(event_type) = headers
        .get("x-github-event")
        .and_then(|value| value.to_str().ok())
    else {
        return StatusCode::BAD_REQUEST;
    };
    if !is_allowlisted_github_repository(&body, &readiness.github_repositories) {
        return StatusCode::OK;
    }
    let Some(database) = readiness.database else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let selected_headers =
        serde_json::json!({"delivery": delivery_id, "event": event_type}).to_string();
    let inbox_id = Uuid::new_v4().to_string();
    let inserted = match database
        .insert_inbox(
            InboxEvent {
                id: &inbox_id,
                source: "github",
                delivery_id,
                event_type,
                raw_headers: &selected_headers,
                raw_body: &body,
            },
            unix_now(),
        )
        .await
    {
        Ok(inserted) => inserted,
        Err(error) => {
            tracing::warn!(error = %error, "unable to persist GitHub webhook delivery");
            return StatusCode::SERVICE_UNAVAILABLE;
        }
    };
    if inserted && let Some(github) = readiness.github {
        tokio::spawn(async move {
            if let Err(error) = GitHubReconciler::new(&database, &github)
                .reconcile_active_pull_requests(unix_now())
                .await
            {
                tracing::warn!(error = %error, "GitHub webhook reconciliation failed");
            }
        });
    }
    StatusCode::OK
}

fn verify_github_signature(secret: &[u8], signature: &str, body: &[u8]) -> bool {
    let Some(encoded) = signature.strip_prefix("sha256=") else {
        return false;
    };
    let Ok(expected) = hex::decode(encoded) else {
        return false;
    };
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(&expected).is_ok()
}

fn is_allowlisted_github_repository(body: &[u8], allowlist: &BTreeSet<String>) -> bool {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .pointer("/repository/full_name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .is_some_and(|repository| allowlist.contains(&repository))
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

async fn request_id(mut request: Request, next: Next) -> Response {
    let request_id = HeaderValue::from_str(&Uuid::new_v4().to_string())
        .expect("UUID is a valid HTTP header value");
    request
        .headers_mut()
        .insert(HeaderName::from_static("x-request-id"), request_id.clone());
    let mut response = next.run(request).await;
    info!(request_id = %request_id.to_str().unwrap_or("invalid"), "request completed");
    response
        .headers_mut()
        .insert(HeaderName::from_static("x-request-id"), request_id);
    response
}

async fn live() -> StatusCode {
    StatusCode::OK
}

async fn ready(State(readiness): State<Readiness>) -> StatusCode {
    if readiness.configuration_valid {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    info!("shutdown signal received");
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    #[tokio::test]
    async fn liveness_never_depends_on_readiness() {
        let response = health_router(Readiness {
            configuration_valid: false,
            database: None,
            github: None,
            github_webhook_secret: None,
            github_repositories: BTreeSet::new(),
        })
        .oneshot(Request::get("/health/live").body(Body::empty()).unwrap())
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("x-request-id"));
    }

    #[tokio::test]
    async fn readiness_reports_invalid_configuration() {
        let response = health_router(Readiness {
            configuration_valid: false,
            database: None,
            github: None,
            github_webhook_secret: None,
            github_repositories: BTreeSet::new(),
        })
        .oneshot(Request::get("/health/ready").body(Body::empty()).unwrap())
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn github_signatures_require_the_original_body() {
        let secret = b"secret";
        let body = br#"{"repository":{"full_name":"owner/repo"}}"#;
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).unwrap();
        mac.update(body);
        let signature = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
        assert!(verify_github_signature(secret, &signature, body));
        assert!(!verify_github_signature(secret, &signature, b"{}"));
    }
}
