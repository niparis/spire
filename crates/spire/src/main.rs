#![forbid(unsafe_code)]

use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use anyhow::{Context, Result};
use axum::{
    Router,
    extract::{Request, State},
    http::{HeaderName, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::get,
};
use clap::{Parser, Subcommand};
use spire_adapters::{
    linear::LinearReadAdapter,
    sqlite::{LinearObservation, SqliteDatabase},
};
use spire_application::{
    CanonicalLinearIssue, Config, EligibilityInput, ExternalResult, LinearReadPort,
    RelevantIssueQuery, ValidatedConfig, dispatch_is_covered, evaluate_eligibility,
};
use spire_domain::{ComplexityClass, HarnessId, LinearIssueId, RunRole};
use tokio::net::TcpListener;
use tracing::info;
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(name = "spire", about = "Single-node Code Harness orchestrator")]
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
    Linear {
        #[command(subcommand)]
        command: LinearCommand,
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

#[derive(Clone)]
struct Readiness {
    configuration_valid: bool,
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
    let readiness = Readiness {
        configuration_valid: true,
    };
    let api = TcpListener::bind(config.config.server.api_bind)
        .await
        .context("failed to bind API listener")?;
    let admin = TcpListener::bind(config.config.server.admin_bind)
        .await
        .context("failed to bind loopback admin listener")?;
    info!(api = %config.config.server.api_bind, admin = %config.config.server.admin_bind, "starting Spire foundation service");

    let api_server = axum::serve(api, health_router(readiness.clone()))
        .with_graceful_shutdown(shutdown_signal());
    let admin_server =
        axum::serve(admin, health_router(readiness)).with_graceful_shutdown(shutdown_signal());
    tokio::try_join!(api_server, admin_server).context("health server stopped unexpectedly")?;
    Ok(())
}

fn health_router(readiness: Readiness) -> Router {
    Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
        .layer(middleware::from_fn(request_id))
        .with_state(readiness)
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
        })
        .oneshot(Request::get("/health/ready").body(Body::empty()).unwrap())
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
