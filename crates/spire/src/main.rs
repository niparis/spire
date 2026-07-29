#![forbid(unsafe_code)]

mod runtime_paths;
mod user_service;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use axum::{
    Router,
    body::{Bytes, to_bytes},
    extract::{Query, Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use clap::{Parser, Subcommand};
use hmac::{Hmac, Mac};
use nix::{
    sys::termios::{self, LocalFlags, SetArg},
    unistd::Uid,
};
use sha2::Sha256;
use spire_adapters::{
    diagnostics::{
        GitCliProbe, HarnessKind, HarnessProbeSpec, ProcessHarnessProbe, SystemCommandExecutor,
        SystemdServiceContextProbe,
    },
    github::{GitHubHttpAdapter, GitHubReconciler},
    github_app::{
        GitHubAppApi, GitHubAppHttpApi, GitHubAppManifest, GitHubAppServiceProbe,
        GitHubAppTokenProvider, SystemClock, approved_installation_permissions,
    },
    linear::{LinearReadAdapter, load_credential},
    secrets::{UserAuthenticationMetadataStore, UserSecretStore},
    sqlite::{InboxEvent, LinearObservation, SqliteDatabase},
};
use spire_application::{
    AuthenticationMetadataStorePort, AuthenticationState, CanonicalLinearIssue, Config,
    DiagnosticFinding, DiagnosticReport, DiagnosticSeverity, EligibilityInput, ExternalResult,
    GitHubAuthenticationMetadata, GitTransportProbePort, HarnessProbePort, InstallationProfile,
    LinearAuthenticationMetadata, LinearReadPort, ManagedSecret, RelevantIssueQuery,
    SecretPromptPort, SecretStorePort, ServiceAuthenticationProbePort, ServiceContextProbePort,
    ValidatedConfig, WebhookAllowlist, WebhookRequest, accept_delivery, dispatch_is_covered,
    evaluate_eligibility,
};
use spire_domain::{ComplexityClass, Effort, HarnessId, LinearIssueId, RunRole};
use tokio::{net::TcpListener, sync::oneshot, time::timeout};
use tracing::info;
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(
    name = "spire",
    about = "Single-node Code Harness orchestrator",
    version
)]
struct Cli {
    /// An absolute configuration file path. When omitted, Spire applies the
    /// documented user-XDG then system-profile precedence.
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    /// Select the explicit machine-wide installation profile.
    #[arg(long, global = true)]
    system: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Paths {
        #[arg(long, default_value = "text")]
        format: OutputFormat,
    },
    Service {
        #[command(subcommand)]
        command: ServiceCommand,
    },
    Start,
    Stop,
    Status,
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
    Doctor {
        #[arg(long, default_value = "text")]
        format: OutputFormat,
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
    Serve,
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    Path,
    Validate,
    Show {
        #[arg(long)]
        effective: bool,
        #[arg(long)]
        redacted: bool,
    },
    Migrate {
        #[arg(long)]
        from: PathBuf,
        #[arg(long)]
        write: bool,
    },
}

#[derive(Debug, Subcommand)]
enum AuthCommand {
    Status {
        #[arg(long, default_value = "text")]
        format: OutputFormat,
    },
    Login {
        service: AuthService,
        /// Read the credential from a regular 0600 file owned by the runtime
        /// user. The credential itself is never accepted as an argument.
        #[arg(long)]
        credential_file: Option<PathBuf>,
        /// Register the GitHub App in this organization; omit for the user account.
        #[arg(long)]
        github_owner: Option<String>,
    },
    Rotate {
        service: AuthService,
        /// Read the replacement credential from a regular 0600 file owned by
        /// the runtime user. The credential itself is never accepted as an
        /// argument.
        #[arg(long)]
        credential_file: Option<PathBuf>,
    },
    Remove {
        service: AuthService,
    },
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum AuthService {
    Linear,
    #[value(name = "github")]
    GitHub,
}

#[derive(Debug, Subcommand)]
enum ServiceCommand {
    Install {
        /// Write the rendered unit after previewing it.
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

#[derive(Debug, Subcommand)]
enum DispatchCommand {
    DryRun {
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
    BackupDaily {},
    RestoreCheck {
        #[arg(long)]
        backup: PathBuf,
        #[arg(long)]
        destination: PathBuf,
    },
    RestoreLatest,
}

#[derive(Debug, Subcommand)]
enum OpsCommand {
    Status,
}

#[derive(Debug, Subcommand)]
enum LinearCommand {
    Get {
        issue: String,
    },
    Reconcile {
        #[arg(long)]
        dry_run: bool,
    },
    Explain {
        issue: String,
    },
}

#[derive(Debug, Subcommand)]
enum GitHubCommand {
    Reconcile,
}

#[derive(Debug, Subcommand)]
enum SchedulerCommand {
    Once {
        #[arg(long)]
        dry_run: bool,
    },
    Explain {
        issue: String,
    },
    CapacityShow,
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

    let Cli {
        config: config_override,
        system,
        command,
    } = Cli::parse();
    match command {
        Command::Paths { format } => print_paths(
            resolve_runtime_paths(config_override.as_deref(), system)?,
            format,
        )?,
        Command::Service {
            command: ServiceCommand::Install { yes },
        } => user_service::install(
            &resolve_runtime_paths(config_override.as_deref(), system)?,
            &std::env::current_exe().context("unable to resolve installed Spire binary")?,
            yes,
        )?,
        Command::Start => user_service::systemctl("start", system)?,
        Command::Stop => user_service::systemctl("stop", system)?,
        Command::Status => user_service::systemctl("status", system)?,
        Command::Config {
            command: ConfigCommand::Path,
        } => print_paths(
            resolve_runtime_paths(config_override.as_deref(), system)?,
            OutputFormat::Text,
        )?,
        Command::Config {
            command: ConfigCommand::Validate,
        } => {
            load_config(config_override.as_deref(), system)?;
            println!("configuration is valid");
        }
        Command::Config {
            command:
                ConfigCommand::Show {
                    effective,
                    redacted,
                },
        } => config_show(config_override.as_deref(), system, effective, redacted)?,
        Command::Config {
            command: ConfigCommand::Migrate { from, write },
        } => config_migrate(&from, write)?,
        Command::Auth { command } => {
            let paths = resolve_runtime_paths(config_override.as_deref(), system)?;
            match command {
                AuthCommand::Status { format } => {
                    auth_status(paths, config_override.as_deref(), system, format).await?
                }
                AuthCommand::Login {
                    service,
                    credential_file,
                    github_owner,
                } => {
                    auth_login(
                        paths,
                        config_override.as_deref(),
                        system,
                        service,
                        credential_file,
                        github_owner,
                    )
                    .await?
                }
                AuthCommand::Rotate {
                    service,
                    credential_file,
                } => auth_rotate(paths, service, credential_file).await?,
                AuthCommand::Remove { service } => auth_remove(paths, service)?,
            }
        }
        Command::Doctor { format } => {
            doctor(
                resolve_runtime_paths(config_override.as_deref(), system)?,
                system,
                format,
            )
            .await?
        }
        Command::Dispatch {
            command: DispatchCommand::DryRun { maker_harness },
        } => dispatch_dry_run(
            load_config(config_override.as_deref(), system)?,
            maker_harness,
        )?,
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
            command: DbCommand::BackupDaily {},
        } => backup_daily(load_config(config_override.as_deref(), system)?).await?,
        Command::Db {
            command:
                DbCommand::RestoreCheck {
                    backup,
                    destination,
                },
        } => restore_check(backup, destination).await?,
        Command::Db {
            command: DbCommand::RestoreLatest,
        } => restore_latest(load_config(config_override.as_deref(), system)?).await?,
        Command::Ops {
            command: OpsCommand::Status,
        } => operations_status(load_config(config_override.as_deref(), system)?).await?,
        Command::Linear {
            command: LinearCommand::Get { issue },
        } => {
            let config = load_config(config_override.as_deref(), system)?;
            let paths = resolve_runtime_paths(config_override.as_deref(), system)?;
            let issue = LinearIssueId::new(issue).context("invalid Linear issue ID")?;
            let adapter = linear_adapter(&config, &paths)?;
            match adapter.get_canonical_issue(&issue).await? {
                ExternalResult::Confirmed(issue) => print_json(&issue)?,
                ExternalResult::NotFound => anyhow::bail!("Linear issue was not found"),
                ExternalResult::Ambiguous { detail } => {
                    anyhow::bail!("ambiguous Linear response: {detail}")
                }
            }
        }
        Command::Linear {
            command: LinearCommand::Explain { issue },
        } => {
            let config = load_config(config_override.as_deref(), system)?;
            let paths = resolve_runtime_paths(config_override.as_deref(), system)?;
            let issue_id = LinearIssueId::new(issue).context("invalid Linear issue ID")?;
            let adapter = linear_adapter(&config, &paths)?;
            match adapter.get_canonical_issue(&issue_id).await? {
                ExternalResult::Confirmed(issue) => print_json(&explain_issue(&config, &issue))?,
                ExternalResult::NotFound => anyhow::bail!("Linear issue was not found"),
                ExternalResult::Ambiguous { detail } => {
                    anyhow::bail!("ambiguous Linear response: {detail}")
                }
            }
        }
        Command::Linear {
            command: LinearCommand::Reconcile { dry_run },
        } => {
            if !dry_run {
                anyhow::bail!("Linear reconciliation is read-only in Sprint 03; pass --dry-run");
            }
            let config = load_config(config_override.as_deref(), system)?;
            let paths = resolve_runtime_paths(config_override.as_deref(), system)?;
            linear_reconcile(config, paths).await?;
        }
        Command::GitHub {
            command: GitHubCommand::Reconcile,
        } => {
            github_reconcile(
                load_config(config_override.as_deref(), system)?,
                resolve_runtime_paths(config_override.as_deref(), system)?,
            )
            .await?
        }
        Command::Scheduler {
            command: SchedulerCommand::Once { dry_run },
        } => {
            if !dry_run {
                anyhow::bail!(
                    "scheduler dispatch remains dry-run-only in Sprint 04; pass --dry-run"
                );
            }
            let config = load_config(config_override.as_deref(), system)?;
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
            command: SchedulerCommand::CapacityShow,
        } => {
            let config = load_config(config_override.as_deref(), system)?;
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
            command: SchedulerCommand::Explain { issue },
        } => {
            let config = load_config(config_override.as_deref(), system)?;
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
        Command::Serve => {
            serve(
                load_config(config_override.as_deref(), system)?,
                resolve_runtime_paths(config_override.as_deref(), system)?,
            )
            .await?
        }
    }
    Ok(())
}

fn linear_adapter(
    config: &ValidatedConfig,
    paths: &spire_application::ResolvedPaths,
) -> Result<LinearReadAdapter> {
    if paths.profile == spire_application::InstallationProfile::User {
        let token = UserSecretStore::below_config_root(&paths.config_root)
            .read_for_service(ManagedSecret::LinearApiKey)
            .context("failed to load the Linear API key from the user secret store")?;
        return LinearReadAdapter::from_token(token.as_str().to_owned())
            .context("failed to construct read-only Linear adapter");
    }

    let reference = config
        .config
        .linear
        .credential_ref
        .as_deref()
        .context("system-profile Linear authentication requires linear.credential_ref")?;
    LinearReadAdapter::from_credential_reference(reference)
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

async fn linear_reconcile(
    config: ValidatedConfig,
    paths: spire_application::ResolvedPaths,
) -> Result<()> {
    let adapter = linear_adapter(&config, &paths)?;
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

async fn github_reconcile(
    config: ValidatedConfig,
    paths: spire_application::ResolvedPaths,
) -> Result<()> {
    let database = SqliteDatabase::initialize(
        &config.config.runtime.database_path,
        config.config.runtime.database_max_connections,
    )
    .await?;
    let github = github_adapter(&config, &paths).await?;
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

async fn github_adapter(
    config: &ValidatedConfig,
    paths: &spire_application::ResolvedPaths,
) -> Result<GitHubHttpAdapter> {
    let repositories = config
        .config
        .github
        .repositories
        .iter()
        .map(|entry| spire_domain::RepositoryName::new(entry.repository.clone()))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let token = github_token_provider(config, paths)
        .context("failed to construct GitHub App authentication")?
        .installation_token()
        .await
        .context("failed to mint a GitHub App installation token")?;
    Ok(GitHubHttpAdapter::new(
        token.expose_to_github_adapter().to_owned(),
        repositories,
        Duration::from_secs(config.config.github.request_timeout_seconds),
    )?)
}

fn print_json(value: &impl serde::Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn resolve_runtime_paths(
    config: Option<&std::path::Path>,
    system: bool,
) -> Result<spire_application::ResolvedPaths> {
    runtime_paths::resolve_paths(config, system, &runtime_paths::process_environment())
}

fn print_paths(paths: spire_application::ResolvedPaths, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Text => {
            println!("profile: {:?}", paths.profile);
            println!("config: {}", paths.config_file.display());
            println!("data: {}", paths.data_root.display());
            println!("state: {}", paths.state_root.display());
            println!("cache: {}", paths.cache_root.display());
        }
        OutputFormat::Json => print_json(&paths)?,
    }
    Ok(())
}

async fn auth_status(
    paths: spire_application::ResolvedPaths,
    config_override: Option<&std::path::Path>,
    system: bool,
    format: OutputFormat,
) -> Result<()> {
    if paths.profile != spire_application::InstallationProfile::User {
        anyhow::bail!(
            "system-profile authentication requires its separate credential-store adapter; no user secret store was consulted"
        );
    }
    let store = UserSecretStore::below_config_root(&paths.config_root);
    let mut findings = secret_configuration_findings(&store);
    match load_config(config_override, system) {
        Ok(config) => {
            match linear_adapter(&config, &paths) {
                Ok(linear) => match linear.probe_service("linear").await {
                    Ok(probe) => findings.push(service_probe_finding("SPIRE-AUTH-010", probe)),
                    Err(_) => findings.push(DiagnosticFinding::required_authentication(
                        "SPIRE-AUTH-010",
                        AuthenticationState::Ambiguous,
                        "Linear authentication probe failed without trusted evidence",
                        Some("run spire auth rotate linear".into()),
                    )),
                },
                Err(_) => findings.push(DiagnosticFinding::required_authentication(
                    "SPIRE-AUTH-010",
                    AuthenticationState::Unavailable,
                    "Linear authentication is not configured",
                    Some("run spire auth login linear".into()),
                )),
            }
            match github_service_probe(&config, &paths).await {
                Ok(probe) => findings.push(service_probe_finding("SPIRE-AUTH-020", probe)),
                Err(error) => findings.push(DiagnosticFinding::required_authentication(
                    "SPIRE-AUTH-020",
                    AuthenticationState::Ambiguous,
                    "GitHub App authentication could not be verified",
                    Some(format!(
                        "run spire auth login github or inspect the installation: {error}"
                    )),
                )),
            }
        }
        Err(_) => findings.push(DiagnosticFinding::required(
            "SPIRE-AUTH-000",
            AuthenticationState::Unavailable,
            "configuration is required for provider authentication probes",
            Some("run spire config validate".into()),
        )),
    }
    let report = DiagnosticReport::from_findings(findings);
    print_diagnostic_report(&report, format)?;
    Ok(())
}

async fn auth_login(
    paths: spire_application::ResolvedPaths,
    config_override: Option<&std::path::Path>,
    system: bool,
    service: AuthService,
    credential_file: Option<PathBuf>,
    github_owner: Option<String>,
) -> Result<()> {
    match service {
        AuthService::Linear => install_linear_credential(paths, credential_file, false).await,
        AuthService::GitHub => {
            if credential_file.is_some() {
                anyhow::bail!(
                    "GitHub App registration never accepts a private key or webhook secret file"
                )
            }
            let config = load_config(config_override, system)?;
            register_github_app(paths, &config, github_owner).await
        }
    }
}

async fn auth_rotate(
    paths: spire_application::ResolvedPaths,
    service: AuthService,
    credential_file: Option<PathBuf>,
) -> Result<()> {
    match service {
        AuthService::Linear => install_linear_credential(paths, credential_file, true).await,
        AuthService::GitHub => anyhow::bail!(
            "GitHub App private-key rotation requires a GitHub-generated replacement; no key was accepted through process arguments"
        ),
    }
}

fn auth_remove(paths: spire_application::ResolvedPaths, service: AuthService) -> Result<()> {
    ensure_user_auth_profile(&paths)?;
    match service {
        AuthService::Linear => {
            let store = UserSecretStore::below_config_root(&paths.config_root);
            store
                .remove(ManagedSecret::LinearApiKey)
                .context("failed to remove the Linear API key")?;
            if store.status(ManagedSecret::LinearApiKey)? != AuthenticationState::Unavailable {
                anyhow::bail!("Linear credential removal did not leave the installation non-ready")
            }
            println!("Linear authentication removed; Spire is now non-ready until login succeeds");
            Ok(())
        }
        AuthService::GitHub => {
            let store = UserSecretStore::below_config_root(&paths.config_root);
            store
                .remove_many(&[
                    ManagedSecret::GitHubAppPrivateKey,
                    ManagedSecret::GitHubWebhookSecret,
                ])
                .context("failed to remove the GitHub App credential bundle")?;
            let metadata_store =
                UserAuthenticationMetadataStore::below_config_root(&paths.config_root);
            let mut metadata = metadata_store.load()?;
            metadata.github = None;
            metadata_store.store(&metadata)?;
            println!("GitHub App authentication removed; Spire is now non-ready");
            Ok(())
        }
    }
}

async fn install_linear_credential(
    paths: spire_application::ResolvedPaths,
    credential_file: Option<PathBuf>,
    rotation: bool,
) -> Result<()> {
    ensure_user_auth_profile(&paths)?;
    let store = UserSecretStore::below_config_root(&paths.config_root);
    if rotation && store.status(ManagedSecret::LinearApiKey)? != AuthenticationState::Configured {
        anyhow::bail!("cannot rotate Linear authentication before a successful login")
    }
    let credential = read_secret_input(credential_file)?;
    let adapter = LinearReadAdapter::from_token(credential.as_str().to_owned())
        .context("failed to construct the Linear authentication probe")?;
    let identity = adapter
        .verify_viewer()
        .await
        .context("Linear credential verification failed; the prior credential remains active")?;
    store
        .replace(ManagedSecret::LinearApiKey, credential)
        .context("failed to activate the verified Linear credential")?;
    let metadata_store = UserAuthenticationMetadataStore::below_config_root(&paths.config_root);
    let mut metadata = metadata_store.load()?;
    metadata.linear = Some(LinearAuthenticationMetadata {
        viewer_id: identity.viewer_id.clone(),
        organization_id: identity.organization_id.clone(),
        verified_at_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    });
    metadata_store.store(&metadata)?;
    println!(
        "Linear authentication {} for viewer {} in organization {}",
        if rotation { "rotated" } else { "configured" },
        identity.viewer_id,
        identity.organization_id
    );
    Ok(())
}

#[derive(Clone)]
struct ManifestFlowState {
    form_action: Arc<str>,
    manifest: Arc<str>,
    expected_state: Arc<str>,
    code_sender: Arc<Mutex<Option<oneshot::Sender<String>>>>,
}

#[derive(serde::Deserialize)]
struct ManifestCallback {
    code: Option<String>,
    state: Option<String>,
}

async fn manifest_start(State(state): State<ManifestFlowState>) -> Html<String> {
    Html(format!(
        "<!doctype html><meta charset=\"utf-8\"><title>Register Spire GitHub App</title>\
         <p>Continue to GitHub to review and create the preconfigured Spire App.</p>\
         <form method=\"post\" action=\"{}\">\
         <input type=\"hidden\" name=\"manifest\" value=\"{}\">\
         <button type=\"submit\">Continue to GitHub</button></form>",
        html_attribute(&state.form_action),
        html_attribute(&state.manifest),
    ))
}

async fn manifest_callback(
    State(flow): State<ManifestFlowState>,
    Query(callback): Query<ManifestCallback>,
) -> impl IntoResponse {
    let Some(code) = callback
        .code
        .filter(|value| !value.is_empty() && value.len() <= 256)
    else {
        return (
            StatusCode::BAD_REQUEST,
            "GitHub did not return a usable one-time manifest code",
        );
    };
    if callback.state.as_deref() != Some(flow.expected_state.as_ref()) {
        return (
            StatusCode::UNAUTHORIZED,
            "GitHub App registration state did not match",
        );
    }
    let Some(sender) = flow
        .code_sender
        .lock()
        .ok()
        .and_then(|mut value| value.take())
    else {
        return (
            StatusCode::CONFLICT,
            "GitHub App registration was already completed",
        );
    };
    if sender.send(code).is_err() {
        return (
            StatusCode::GONE,
            "Spire is no longer waiting for this registration",
        );
    }
    (
        StatusCode::OK,
        "GitHub App registration received. Return to the Spire terminal.",
    )
}

async fn register_github_app(
    paths: spire_application::ResolvedPaths,
    config: &ValidatedConfig,
    github_owner: Option<String>,
) -> Result<()> {
    ensure_user_auth_profile(&paths)?;
    if let Some(owner) = github_owner.as_deref()
        && !owner
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        anyhow::bail!("--github-owner contains an unsafe character")
    }
    let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .await
        .context("unable to bind the loopback GitHub App callback")?;
    let address = listener.local_addr()?;
    let redirect_url = format!("http://{address}/callback");
    let webhook_hostname = &config.config.cloudflare.webhook_hostname;
    let manifest = GitHubAppManifest::spire(
        format!("Spire {}", &Uuid::new_v4().simple().to_string()[..8]),
        format!("https://{webhook_hostname}"),
        redirect_url,
        format!("https://{webhook_hostname}/webhooks/github"),
        false,
    );
    let state_token = Uuid::new_v4().simple().to_string();
    let registration_url = match github_owner {
        Some(owner) => format!(
            "https://github.com/organizations/{}/settings/apps/new?state={state_token}",
            urlencoding::encode(&owner)
        ),
        None => format!("https://github.com/settings/apps/new?state={state_token}"),
    };
    let (code_sender, code_receiver) = oneshot::channel();
    let (shutdown_sender, shutdown_receiver) = oneshot::channel();
    let flow = ManifestFlowState {
        form_action: registration_url.into(),
        manifest: serde_json::to_string(&manifest)?.into(),
        expected_state: state_token.into(),
        code_sender: Arc::new(Mutex::new(Some(code_sender))),
    };
    let router = Router::new()
        .route("/", get(manifest_start))
        .route("/callback", get(manifest_callback))
        .with_state(flow);
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = shutdown_receiver.await;
            })
            .await
    });
    println!("Open http://{address}/ to register the preconfigured GitHub App.");
    println!("Spire will wait up to ten minutes for GitHub's loopback callback.");
    let code = timeout(Duration::from_secs(600), code_receiver)
        .await
        .context("GitHub App registration timed out without changing credentials")?
        .context("GitHub App callback closed without a code")?;
    let _ = shutdown_sender.send(());
    server
        .await
        .context("GitHub App callback task failed")?
        .context("GitHub App callback server failed")?;

    let conversion = GitHubAppHttpApi::new(Duration::from_secs(
        config.config.github.request_timeout_seconds,
    ))?
    .exchange_manifest_code(&code)
    .await
    .context("GitHub rejected the one-time manifest conversion code")?;
    let store = UserSecretStore::below_config_root(&paths.config_root);
    store
        .replace_many(vec![
            (
                ManagedSecret::GitHubAppPrivateKey,
                spire_application::SecretInput::new(conversion.private_key_pem),
            ),
            (
                ManagedSecret::GitHubWebhookSecret,
                spire_application::SecretInput::new(conversion.webhook_secret),
            ),
        ])
        .context("failed to activate the GitHub App credential bundle")?;
    let metadata_store = UserAuthenticationMetadataStore::below_config_root(&paths.config_root);
    let mut metadata = metadata_store.load()?;
    metadata.github = Some(GitHubAuthenticationMetadata {
        app_id: conversion.app_id,
        app_slug: conversion.app_slug,
        client_id: conversion.client_id,
        html_url: conversion.html_url.clone(),
        verified_at_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    });
    metadata_store.store(&metadata)?;
    println!("GitHub App registered: {}", conversion.html_url);
    println!(
        "Install the App on the configured repositories, then set the non-secret github.installation_id and run spire doctor."
    );
    Ok(())
}

fn html_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn ensure_user_auth_profile(paths: &spire_application::ResolvedPaths) -> Result<()> {
    if paths.profile != spire_application::InstallationProfile::User {
        anyhow::bail!(
            "system-profile authentication requires its separate credential-store adapter; no user secret store was consulted"
        )
    }
    Ok(())
}

fn read_secret_input(credential_file: Option<PathBuf>) -> Result<spire_application::SecretInput> {
    match credential_file {
        Some(path) => read_protected_secret_file(&path),
        None => TtySecretPrompt.prompt_secret("Linear API key: "),
    }
}

fn read_protected_secret_file(path: &std::path::Path) -> Result<spire_application::SecretInput> {
    const MAX_SECRET_FILE_BYTES: u64 = 1024 * 1024;

    let file = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)
        .with_context(|| format!("unable to open credential file {}", path.display()))?;
    let metadata = file
        .metadata()
        .with_context(|| format!("unable to inspect credential file {}", path.display()))?;
    if !metadata.file_type().is_file()
        || metadata.uid() != Uid::current().as_raw()
        || metadata.mode() & 0o777 != 0o600
    {
        anyhow::bail!("credential file must be an owner-only regular 0600 file")
    }
    let mut bytes = Vec::new();
    file.take(MAX_SECRET_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("unable to read credential file {}", path.display()))?;
    if bytes.len() as u64 > MAX_SECRET_FILE_BYTES {
        anyhow::bail!("credential file exceeds the one MiB input limit")
    }
    let value = String::from_utf8(bytes).context("credential file must contain UTF-8 text")?;
    Ok(spire_application::SecretInput::new(
        value.trim_end_matches(['\r', '\n']).to_owned(),
    ))
}

struct TtySecretPrompt;

impl SecretPromptPort for TtySecretPrompt {
    type Error = anyhow::Error;

    fn prompt_secret(&self, prompt: &str) -> Result<spire_application::SecretInput> {
        let mut terminal = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .context("a TTY or --credential-file is required to provide a credential")?;
        terminal.write_all(prompt.as_bytes())?;
        terminal.flush()?;
        let original = termios::tcgetattr(&terminal).context("unable to read TTY settings")?;
        let mut hidden = original.clone();
        hidden.local_flags.remove(LocalFlags::ECHO);
        termios::tcsetattr(&terminal, SetArg::TCSANOW, &hidden)
            .context("unable to disable terminal echo")?;
        let mut value = String::new();
        let read_result = {
            let mut reader = BufReader::new(&terminal);
            reader.read_line(&mut value)
        };
        let restore_result = termios::tcsetattr(&terminal, SetArg::TCSANOW, &original);
        terminal.write_all(b"\n")?;
        restore_result.context("unable to restore terminal echo")?;
        read_result.context("unable to read credential from the TTY")?;
        Ok(spire_application::SecretInput::new(
            value.trim_end_matches(['\r', '\n']).to_owned(),
        ))
    }
}

async fn doctor(
    paths: spire_application::ResolvedPaths,
    system: bool,
    format: OutputFormat,
) -> Result<()> {
    let mut findings = Vec::new();
    findings.push(DiagnosticFinding::required(
        "SPIRE-DIAG-001",
        AuthenticationState::Configured,
        "Spire paths resolved for the selected installation profile",
        None,
    ));

    let config = load_config(Some(&paths.config_file), system);
    let configuration_state = if config.is_ok() {
        AuthenticationState::Configured
    } else {
        AuthenticationState::Unavailable
    };
    findings.push(DiagnosticFinding::required(
        "SPIRE-DIAG-002",
        configuration_state,
        "configuration validation",
        (configuration_state == AuthenticationState::Unavailable).then_some(format!(
            "spire config validate --config {}",
            paths.config_file.display()
        )),
    ));

    let Some(config) = config.ok() else {
        let report = DiagnosticReport::from_findings(findings);
        print_diagnostic_report(&report, format)?;
        anyhow::bail!("Spire diagnostics are not ready")
    };

    match SqliteDatabase::initialize(
        &config.config.runtime.database_path,
        config.config.runtime.database_max_connections,
    )
    .await
    {
        Ok(database) if database.check_integrity().await.is_ok() => {
            findings.push(DiagnosticFinding::required(
                "SPIRE-DIAG-003",
                AuthenticationState::Configured,
                "SQLite migrations are current and integrity_check passed",
                None,
            ));
        }
        _ => findings.push(DiagnosticFinding::required(
            "SPIRE-DIAG-003",
            AuthenticationState::Unavailable,
            "SQLite migrations or integrity check failed",
            Some(format!(
                "spire db check --database {}",
                config.config.runtime.database_path.display()
            )),
        )),
    }

    if paths.profile == InstallationProfile::User {
        findings.extend(secret_configuration_findings(
            &UserSecretStore::below_config_root(&paths.config_root),
        ));
    }
    match linear_adapter(&config, &paths) {
        Ok(linear) => match linear.probe_service("linear").await {
            Ok(probe) => findings.push(service_probe_finding("SPIRE-AUTH-010", probe)),
            Err(_) => findings.push(DiagnosticFinding::required_authentication(
                "SPIRE-AUTH-010",
                AuthenticationState::Ambiguous,
                "Linear authentication response was not understood",
                Some("run spire auth rotate linear".into()),
            )),
        },
        Err(_) => findings.push(DiagnosticFinding::required_authentication(
            "SPIRE-AUTH-010",
            AuthenticationState::Unavailable,
            "Linear authentication is unavailable",
            Some("run spire auth login linear".into()),
        )),
    }
    match github_service_probe(&config, &paths).await {
        Ok(probe) => findings.push(service_probe_finding("SPIRE-AUTH-020", probe)),
        Err(error) => findings.push(DiagnosticFinding::required_authentication(
            "SPIRE-AUTH-020",
            AuthenticationState::Ambiguous,
            "GitHub App installation identity or permissions could not be verified",
            Some(format!(
                "repair GitHub App authentication, then retry: {error}"
            )),
        )),
    }

    for (index, role) in [
        &config.config.harnesses.maker,
        &config.config.harnesses.reviewer,
    ]
    .into_iter()
    .enumerate()
    {
        let harness = role.provider.as_str();
        let kind = match harness {
            "codex" => HarnessKind::Codex,
            "claude-code" => HarnessKind::ClaudeCode,
            _ => {
                findings.push(DiagnosticFinding::required_authentication(
                    format!("SPIRE-HARNESS-{}", index + 1),
                    AuthenticationState::Unsupported,
                    format!("unsupported harness provider {harness}"),
                    None,
                ));
                continue;
            }
        };
        let probe = ProcessHarnessProbe::new(
            SystemCommandExecutor,
            HarnessProbeSpec {
                kind,
                executable: PathBuf::from(if kind == HarnessKind::ClaudeCode {
                    "claude"
                } else {
                    "codex"
                }),
                configured_models: vec![role.model.as_str().to_owned()],
                configured_efforts: vec![effort_name(role.effort).into()],
            },
        );
        match probe.probe_harness(harness) {
            Ok(probe) => findings.push(DiagnosticFinding::required_authentication(
                format!("SPIRE-HARNESS-{}", index + 1),
                probe.state,
                format!(
                    "{} {} authentication ({:?} confidence)",
                    probe.harness,
                    probe.version.as_deref().unwrap_or("unknown version"),
                    probe.confidence
                ),
                probe.remediation,
            )),
            Err(error) => findings.push(DiagnosticFinding::required_authentication(
                format!("SPIRE-HARNESS-{}", index + 1),
                AuthenticationState::Unavailable,
                format!("{harness} diagnostic failed"),
                Some(error.to_string()),
            )),
        }
    }

    if let Some(repository) = config.config.github.repositories.first() {
        match GitCliProbe::new(SystemCommandExecutor, "git", &repository.workspace_root)
            .probe_git_transport()
        {
            Ok(probe) => {
                findings.push(DiagnosticFinding::required_authentication(
                    "SPIRE-GIT-001",
                    probe.fetch_state,
                    format!(
                        "Git fetch access for {} ({})",
                        probe
                            .canonical_repository
                            .as_deref()
                            .unwrap_or("unrecognized remote"),
                        probe.remote_url.as_deref().unwrap_or("missing origin")
                    ),
                    probe.remediation,
                ));
                findings.push(DiagnosticFinding::optional(
                    "SPIRE-GIT-002",
                    probe.push_state,
                    DiagnosticSeverity::Warning,
                    "Git push authority remains unverified by a non-mutating probe",
                    Some(
                        "verify branch publication authority before enabling production writes"
                            .into(),
                    ),
                ));
                if probe.ephemeral_agent_risk {
                    findings.push(DiagnosticFinding::required(
                        "SPIRE-GIT-003",
                        AuthenticationState::Unavailable,
                        "SSH agent state may not survive logout or reboot",
                        Some(
                            "use a runtime-user SSH identity available to the user service".into(),
                        ),
                    ));
                }
            }
            Err(error) => findings.push(DiagnosticFinding::required_authentication(
                "SPIRE-GIT-001",
                AuthenticationState::Unavailable,
                "Git/SSH fetch access could not be verified",
                Some(error.to_string()),
            )),
        }
    }

    let runtime_user = std::env::var("USER").unwrap_or_else(|_| "CURRENT_USER".into());
    match SystemdServiceContextProbe::new(SystemCommandExecutor, runtime_user)
        .probe_service_context()
    {
        Ok(probe) => findings.push(DiagnosticFinding::required_authentication(
            "SPIRE-SERVICE-001",
            probe.state,
            format!(
                "user service installed={}, active={}, lingering={}",
                probe.unit_installed, probe.unit_active, probe.lingering_enabled
            ),
            probe.remediation,
        )),
        Err(error) => findings.push(DiagnosticFinding::required_authentication(
            "SPIRE-SERVICE-001",
            AuthenticationState::Unavailable,
            "service runtime context could not be inspected",
            Some(error.to_string()),
        )),
    }
    findings.push(DiagnosticFinding::required(
        "SPIRE-ROLLOUT-001",
        if !config.config.rollout.linear_writes_enabled {
            AuthenticationState::Configured
        } else {
            AuthenticationState::Unavailable
        },
        "production automation remains disabled during authentication diagnostics",
        config
            .config
            .rollout
            .linear_writes_enabled
            .then(|| "set rollout.linear_writes_enabled=false until onboarding completes".into()),
    ));
    findings.push(DiagnosticFinding::required(
        "SPIRE-AUTHORITY-001",
        if !config.config.security.credential_can_merge {
            AuthenticationState::Configured
        } else {
            AuthenticationState::PermissionDenied
        },
        "GitHub credential merge authority is disabled",
        config
            .config
            .security
            .credential_can_merge
            .then(|| "remove merge authority before enabling Spire".into()),
    ));
    let report = DiagnosticReport::from_findings(findings);
    print_diagnostic_report(&report, format)?;
    if !report.ready {
        anyhow::bail!("Spire diagnostics are not ready")
    }
    Ok(())
}

fn secret_configuration_findings(store: &UserSecretStore) -> Vec<DiagnosticFinding> {
    let finding = |code: &str, secret: ManagedSecret, provider: &str, remediation: &str| {
        let state = store
            .status(secret)
            .unwrap_or(AuthenticationState::Ambiguous);
        DiagnosticFinding::optional(
            code,
            state,
            if state == AuthenticationState::Configured {
                DiagnosticSeverity::Info
            } else {
                DiagnosticSeverity::Warning
            },
            format!("{provider} service credential bundle status"),
            (!state.is_ready()).then_some(remediation.to_owned()),
        )
    };
    vec![
        finding(
            "SPIRE-AUTH-001",
            ManagedSecret::LinearApiKey,
            "Linear",
            "install or inspect the Linear service credential with spire auth",
        ),
        finding(
            "SPIRE-AUTH-002",
            ManagedSecret::GitHubAppPrivateKey,
            "GitHub App private key",
            "register the GitHub App with spire auth login github",
        ),
        finding(
            "SPIRE-AUTH-003",
            ManagedSecret::GitHubWebhookSecret,
            "GitHub App webhook secret",
            "register the GitHub App with spire auth login github",
        ),
    ]
}

fn service_probe_finding(
    code: &str,
    probe: spire_application::ServiceAuthenticationProbe,
) -> DiagnosticFinding {
    DiagnosticFinding::required_authentication(
        code,
        probe.state,
        format!(
            "{} authentication{}; permissions=[{}]; missing=[{}]; confidence={:?}",
            probe.service,
            probe
                .identity
                .as_deref()
                .map(|identity| format!(" ({identity})"))
                .unwrap_or_default(),
            probe.permissions.join(","),
            probe.missing_permissions.join(","),
            probe.confidence
        ),
        probe.remediation,
    )
}

fn github_token_provider(
    config: &ValidatedConfig,
    paths: &spire_application::ResolvedPaths,
) -> Result<GitHubAppTokenProvider<GitHubAppHttpApi, SystemClock>> {
    let installation_id = config
        .config
        .github
        .installation_id
        .parse::<u64>()
        .context("github.installation_id must be the numeric GitHub App installation ID")?;
    let (app_id, private_key) = match paths.profile {
        InstallationProfile::User => {
            let metadata =
                UserAuthenticationMetadataStore::below_config_root(&paths.config_root).load()?;
            let github = metadata
                .github
                .context("GitHub App metadata is missing; run spire auth login github")?;
            let private_key = UserSecretStore::below_config_root(&paths.config_root)
                .read_for_service(ManagedSecret::GitHubAppPrivateKey)?
                .as_str()
                .to_owned();
            (github.app_id, private_key)
        }
        InstallationProfile::System => {
            let app_id = config
                .config
                .github
                .app_id
                .context("system-profile GitHub authentication requires github.app_id")?;
            let reference =
                config.config.github.credential_ref.as_deref().context(
                    "system-profile GitHub authentication requires github.credential_ref",
                )?;
            (app_id, load_credential(reference)?)
        }
    };
    GitHubAppTokenProvider::new(
        GitHubAppHttpApi::new(Duration::from_secs(
            config.config.github.request_timeout_seconds,
        ))?,
        SystemClock,
        app_id,
        installation_id,
        private_key,
        approved_installation_permissions(false),
    )
    .map_err(Into::into)
}

async fn github_service_probe(
    config: &ValidatedConfig,
    paths: &spire_application::ResolvedPaths,
) -> Result<spire_application::ServiceAuthenticationProbe> {
    GitHubAppServiceProbe::new(
        github_token_provider(config, paths)?,
        approved_installation_permissions(false),
    )
    .probe_service("github")
    .await
    .map_err(Into::into)
}

fn effort_name(effort: Effort) -> &'static str {
    match effort {
        Effort::Low => "low",
        Effort::Medium => "medium",
        Effort::High => "high",
    }
}

fn print_diagnostic_report(report: &DiagnosticReport, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Json => print_json(report),
        OutputFormat::Text => {
            println!("ready: {}", report.ready);
            for finding in &report.findings {
                println!(
                    "{}: {:?} — {}",
                    finding.code, finding.state, finding.summary
                );
                if let Some(remediation) = &finding.remediation {
                    println!("  remediation: {remediation}");
                }
            }
            Ok(())
        }
    }
}

fn config_show(
    config: Option<&std::path::Path>,
    system: bool,
    effective: bool,
    _redacted: bool,
) -> Result<()> {
    let paths = resolve_runtime_paths(config, system)?;
    let input = fs::read_to_string(&paths.config_file).with_context(|| {
        format!(
            "failed to read configuration {}",
            paths.config_file.display()
        )
    })?;
    let mut value = serde_yaml::from_str::<serde_yaml::Value>(&input)
        .context("failed to parse configuration YAML for redacted display")?;
    redact_yaml(&mut value);
    if effective {
        let mut output = serde_yaml::Mapping::new();
        output.insert(
            serde_yaml::Value::String("configuration_path".to_owned()),
            serde_yaml::Value::String(paths.config_file.display().to_string()),
        );
        output.insert(serde_yaml::Value::String("configuration".to_owned()), value);
        println!("{}", serde_yaml::to_string(&output)?);
    } else {
        println!("{}", serde_yaml::to_string(&value)?);
    }
    Ok(())
}

fn config_migrate(from: &std::path::Path, write: bool) -> Result<()> {
    let input = fs::read_to_string(from)
        .with_context(|| format!("failed to read schema 3 configuration {}", from.display()))?;
    let preview = spire_application::preview_schema3_migration(&input)
        .context("configuration migration stopped without changing any file")?;
    let migrated = serde_yaml::to_string(&preview.configuration)?;
    if write {
        atomic_replace_with_backup(from, migrated.as_bytes())?;
        println!("configuration migrated in place: {}", from.display());
    } else {
        let mut redacted = preview.configuration.clone();
        redact_yaml(&mut redacted);
        print_json(&serde_json::json!({
            "write": false,
            "from_schema_version": preview.from_schema_version,
            "to_schema_version": preview.to_schema_version,
            "deferred_fields": preview.deferred_fields,
            "configuration": serde_yaml::to_string(&redacted)?,
        }))?;
    }
    Ok(())
}

fn atomic_replace_with_backup(path: &std::path::Path, content: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .context("configuration file must have a parent directory")?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("configuration path must have a UTF-8 file name")?;
    let timestamp = unix_now();
    let backup = parent.join(format!("{file_name}.before-schema4-{timestamp}.bak"));
    fs::copy(path, &backup)
        .with_context(|| format!("failed to create migration backup {}", backup.display()))?;
    let temporary = parent.join(format!(".{file_name}.schema4-{timestamp}.tmp"));
    let write_result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .with_context(|| {
                format!(
                    "failed to create temporary configuration {}",
                    temporary.display()
                )
            })?;
        file.write_all(content)?;
        file.sync_all()?;
        fs::rename(&temporary, path)
            .with_context(|| format!("failed to atomically replace {}", path.display()))?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write_result
}

fn redact_yaml(value: &mut serde_yaml::Value) {
    match value {
        serde_yaml::Value::Mapping(mapping) => {
            for (key, value) in mapping {
                let sensitive = key.as_str().is_some_and(|name| {
                    let name = name.to_ascii_lowercase();
                    name.contains("secret")
                        || name.contains("credential")
                        || name.contains("token")
                        || name.contains("password")
                });
                if sensitive {
                    *value = serde_yaml::Value::String("REDACTED".to_owned());
                } else {
                    redact_yaml(value);
                }
            }
        }
        serde_yaml::Value::Sequence(values) => values.iter_mut().for_each(redact_yaml),
        _ => {}
    }
}

fn load_config(config: Option<&std::path::Path>, system: bool) -> Result<ValidatedConfig> {
    let paths = resolve_runtime_paths(config, system)?;
    Config::from_path(&paths.config_file)
        .with_context(|| {
            format!(
                "failed to load configuration {}",
                paths.config_file.display()
            )
        })?
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

async fn serve(config: ValidatedConfig, paths: spire_application::ResolvedPaths) -> Result<()> {
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
    let github_webhook_secret = match paths.profile {
        InstallationProfile::User => UserSecretStore::below_config_root(&paths.config_root)
            .read_for_service(ManagedSecret::GitHubWebhookSecret)?
            .as_str()
            .as_bytes()
            .to_vec(),
        InstallationProfile::System => {
            let reference = config.config.github.webhook_secret_ref.as_deref().context(
                "system-profile GitHub authentication requires github.webhook_secret_ref",
            )?;
            load_credential(reference)
                .context("failed to load GitHub webhook signing secret")?
                .into_bytes()
        }
    };
    let readiness = Readiness {
        configuration_valid: true,
        database: Some(database.clone()),
        github: Some(github_adapter(&config, &paths).await?),
        github_webhook_secret: Some(github_webhook_secret),
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
    use std::os::unix::fs::{PermissionsExt, symlink};
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

    #[test]
    fn protected_file_input_requires_user_only_permissions() {
        let root = std::env::temp_dir().join(format!("spire-cli-secret-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let credential_file = root.join("linear-key");
        fs::write(&credential_file, "SPIRE_SECRET_SENTINEL\n").unwrap();
        fs::set_permissions(&credential_file, fs::Permissions::from_mode(0o600)).unwrap();

        let credential = read_protected_secret_file(&credential_file).unwrap();

        assert_eq!(credential.as_str(), "SPIRE_SECRET_SENTINEL");
        assert!(!format!("{credential:?}").contains("SPIRE_SECRET_SENTINEL"));
        fs::set_permissions(&credential_file, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_protected_secret_file(&credential_file).is_err());
        fs::set_permissions(&credential_file, fs::Permissions::from_mode(0o600)).unwrap();
        let credential_link = root.join("linear-key-link");
        symlink(&credential_file, &credential_link).unwrap();
        assert!(read_protected_secret_file(&credential_link).is_err());

        let oversized = root.join("oversized-key");
        fs::write(&oversized, vec![b'x'; 1024 * 1024 + 1]).unwrap();
        fs::set_permissions(&oversized, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(read_protected_secret_file(&oversized).is_err());
        let _ = fs::remove_dir_all(root);
    }
}
