use std::{collections::BTreeMap, fs, net::SocketAddr, path::PathBuf};

use crate::{LinearStateKind, RepositoryMapping, RolloutGate, WebhookLimits};
use serde::Deserialize;
use spire_domain::{
    ComplexityClass, ComplexityEstimate, DispatchCandidate, DispatchPolicy, DispatchPolicyError,
    DispatchPolicyVersion, DispatchRule, DispatchRuleId, Effort, HarnessCapabilityRegistry,
    HarnessId, ModelId, RunRole,
};
use thiserror::Error;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub linear: LinearConfig,
    pub github: GitHubConfig,
    pub cloudflare: CloudflareConfig,
    pub harnesses: BTreeMap<HarnessId, HarnessConfig>,
    pub dispatch: DispatchConfig,
    pub concurrency: ConcurrencyConfig,
    pub security: SecurityConfig,
    pub runtime: RuntimeConfig,
    pub server: ServerConfig,
    pub webhook: WebhookConfig,
    pub rollout: RolloutConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinearConfig {
    pub organization_id: String,
    pub team_id: String,
    pub ready_state_id: String,
    pub in_progress_state_id: String,
    pub in_review_state_id: String,
    pub specs_needed_state_id: String,
    pub blocked_state_id: String,
    pub done_state_id: String,
    pub canceled_state_id: String,
    pub bot_actor_id: String,
    pub credential_ref: String,
    pub complexity_mapping: BTreeMap<ComplexityEstimate, ComplexityClass>,
    pub supported_type_labels: Vec<String>,
    pub repository_mappings: Vec<RepositoryMapping>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubConfig {
    pub installation_id: String,
    pub credential_ref: String,
    pub webhook_secret_ref: String,
    pub request_timeout_seconds: u64,
    pub repositories: Vec<GitHubRepositoryConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubRepositoryConfig {
    pub repository: String,
    pub base_branch: String,
    pub required_checks: Vec<String>,
    pub workspace_root: PathBuf,
}

impl GitHubConfig {
    /// Resolves only explicitly configured repositories. Unknown repositories
    /// fail closed before an adapter can make a request.
    pub fn repository(&self, name: &str) -> Option<&GitHubRepositoryConfig> {
        self.repositories
            .iter()
            .find(|entry| entry.repository == name)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloudflareConfig {
    pub account_ref: String,
    pub zone_ref: String,
    pub webhook_hostname: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessConfig {
    pub executable: String,
    pub credential_ref: String,
    pub models: Vec<ModelId>,
    pub efforts: Vec<Effort>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DispatchConfig {
    pub policy_version: u32,
    pub rules: Vec<DispatchRuleConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DispatchRuleConfig {
    pub id: String,
    pub when: DispatchWhen,
    pub candidates: Vec<DispatchCandidate>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DispatchWhen {
    pub role: RunRole,
    pub complexity: Vec<ComplexityClass>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConcurrencyConfig {
    pub total_active_harness_runs: u16,
    pub ai_initiated_active_harness_runs: u16,
    pub mutating_runs_per_repository: u16,
    pub active_runs_per_ticket: u16,
    pub cleanup_global: u16,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityConfig {
    pub admin_access: String,
    pub maker_push_mode: String,
    pub reviewer_can_push: bool,
    pub credential_can_merge: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    pub database_path: PathBuf,
    pub database_max_connections: u32,
    pub data_root: PathBuf,
    pub backup_root: PathBuf,
    pub workspace_root: PathBuf,
    pub evidence_root: PathBuf,
    pub implementation_timeout_seconds: u64,
    pub review_timeout_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    pub api_bind: SocketAddr,
    pub admin_bind: SocketAddr,
}

/// Public signed-ingress boundary. Every limit here is enforced before the raw
/// body is parsed, so an unauthenticated caller cannot make the process work.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebhookConfig {
    pub path: String,
    pub signing_secret_ref: String,
    pub webhook_id: String,
    pub replay_window_seconds: u64,
    pub max_body_bytes: usize,
}

/// Restricted-rollout gates. Automation stays inert until an operator sets
/// `linear_writes_enabled` and names every allowlisted team, repository, and
/// work type.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RolloutConfig {
    pub linear_writes_enabled: bool,
    pub allowed_team_ids: Vec<String>,
    pub allowed_repositories: Vec<String>,
    pub allowed_type_labels: Vec<String>,
    pub max_active_harness_runs: u16,
}

#[derive(Debug, Clone)]
pub struct ValidatedConfig {
    pub config: Config,
    pub policy: DispatchPolicy,
    pub capabilities: HarnessCapabilityRegistry,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("unable to read configuration {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid configuration YAML: {0}")]
    Parse(#[from] serde_yaml::Error),
    #[error("configuration schema_version must be {SCHEMA_VERSION}")]
    UnsupportedSchemaVersion,
    #[error("configuration value {path} is missing or is still a placeholder")]
    MissingValue { path: String },
    #[error("credential reference {path} must use env:NAME or systemd:credentials/NAME")]
    InvalidCredentialReference { path: String },
    #[error("configuration value {path} must be greater than zero")]
    MustBePositive { path: String },
    #[error("configuration value {path} is outside the supported range: {detail}")]
    OutOfRange { path: String, detail: String },
    #[error("webhook.path must be an absolute path that does not shadow /health or /admin")]
    WebhookPathInvalid,
    #[error("rollout gate {path} is inconsistent: {detail}")]
    RolloutInconsistent { path: String, detail: String },
    #[error("runtime path {path} must be absolute")]
    RelativePath { path: String },
    #[error("runtime paths must be distinct: {first} and {second}")]
    DuplicatePath { first: String, second: String },
    #[error("server.admin_bind must be loopback")]
    AdminMustBeLoopback,
    #[error("reviewer credentials must not have push access")]
    ReviewerCanPush,
    #[error("credentials must not be able to merge")]
    CredentialCanMerge,
    #[error(transparent)]
    Dispatch(#[from] DispatchPolicyError),
    #[error("invalid domain value at {path}: {message}")]
    Domain { path: String, message: String },
}

/// The only configuration schema this build accepts. Sprint 06 added the signed
/// ingress and restricted-rollout sections, so a Sprint 05 file must be edited
/// rather than silently reinterpreted.
pub const SCHEMA_VERSION: u32 = 2;
const MAX_WEBHOOK_BODY_BYTES: usize = 1_048_576;
const MAX_REPLAY_WINDOW_SECONDS: u64 = 600;

impl LinearConfig {
    /// Resolves a projected lifecycle status to its configured workflow-state ID.
    /// Linear writes always address states by ID, never by human-visible name.
    pub fn state_id(&self, kind: LinearStateKind) -> &str {
        match kind {
            LinearStateKind::Ready => &self.ready_state_id,
            LinearStateKind::InProgress => &self.in_progress_state_id,
            LinearStateKind::InReview => &self.in_review_state_id,
            LinearStateKind::SpecsNeeded => &self.specs_needed_state_id,
            LinearStateKind::Blocked => &self.blocked_state_id,
            LinearStateKind::Done => &self.done_state_id,
            LinearStateKind::Canceled => &self.canceled_state_id,
        }
    }

    /// Names the state an ID belongs to, so operator output never has to guess.
    pub fn state_kind(&self, state_id: &str) -> Option<LinearStateKind> {
        LinearStateKind::ALL
            .into_iter()
            .find(|kind| self.state_id(*kind) == state_id)
    }
}

impl ValidatedConfig {
    pub fn webhook_limits(&self) -> WebhookLimits {
        WebhookLimits {
            max_body_bytes: self.config.webhook.max_body_bytes,
            replay_window_seconds: self.config.webhook.replay_window_seconds,
        }
    }

    /// Builds the rollout gate from configuration. The runtime kill switch lives
    /// in the database, so it is supplied separately by the caller.
    pub fn rollout_gate(&self, kill_switch_engaged: bool) -> RolloutGate {
        RolloutGate {
            linear_writes_enabled: self.config.rollout.linear_writes_enabled,
            kill_switch_engaged,
            allowed_team_ids: self
                .config
                .rollout
                .allowed_team_ids
                .iter()
                .cloned()
                .collect(),
            allowed_repositories: self
                .config
                .rollout
                .allowed_repositories
                .iter()
                .cloned()
                .collect(),
            allowed_type_labels: self
                .config
                .rollout
                .allowed_type_labels
                .iter()
                .cloned()
                .collect(),
            max_active_harness_runs: self.config.rollout.max_active_harness_runs,
        }
    }
}

impl Config {
    pub fn from_yaml(input: &str) -> Result<Self, ConfigError> {
        Ok(serde_yaml::from_str(input)?)
    }

    pub fn from_path(path: impl Into<PathBuf>) -> Result<Self, ConfigError> {
        let path = path.into();
        let input = fs::read_to_string(&path).map_err(|source| ConfigError::Read {
            path: path.clone(),
            source,
        })?;
        Self::from_yaml(&input)
    }

    pub fn validate(self) -> Result<ValidatedConfig, ConfigError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(ConfigError::UnsupportedSchemaVersion);
        }
        for (path, value) in [
            (
                "linear.organization_id",
                self.linear.organization_id.as_str(),
            ),
            ("linear.team_id", self.linear.team_id.as_str()),
            ("linear.ready_state_id", self.linear.ready_state_id.as_str()),
            (
                "linear.in_progress_state_id",
                self.linear.in_progress_state_id.as_str(),
            ),
            (
                "linear.in_review_state_id",
                self.linear.in_review_state_id.as_str(),
            ),
            (
                "linear.specs_needed_state_id",
                self.linear.specs_needed_state_id.as_str(),
            ),
            (
                "linear.blocked_state_id",
                self.linear.blocked_state_id.as_str(),
            ),
            ("linear.done_state_id", self.linear.done_state_id.as_str()),
            (
                "linear.canceled_state_id",
                self.linear.canceled_state_id.as_str(),
            ),
            ("linear.bot_actor_id", self.linear.bot_actor_id.as_str()),
            (
                "github.installation_id",
                self.github.installation_id.as_str(),
            ),
            (
                "cloudflare.account_ref",
                self.cloudflare.account_ref.as_str(),
            ),
            ("cloudflare.zone_ref", self.cloudflare.zone_ref.as_str()),
            (
                "cloudflare.webhook_hostname",
                self.cloudflare.webhook_hostname.as_str(),
            ),
            ("security.admin_access", self.security.admin_access.as_str()),
            (
                "security.maker_push_mode",
                self.security.maker_push_mode.as_str(),
            ),
            ("webhook.webhook_id", self.webhook.webhook_id.as_str()),
        ] {
            ensure_value(path, value)?;
        }
        for (path, reference) in [
            ("linear.credential_ref", self.linear.credential_ref.as_str()),
            ("github.credential_ref", self.github.credential_ref.as_str()),
            (
                "webhook.signing_secret_ref",
                self.webhook.signing_secret_ref.as_str(),
            ),
            (
                "github.webhook_secret_ref",
                self.github.webhook_secret_ref.as_str(),
            ),
        ] {
            ensure_credential_reference(path, reference)?;
        }
        validate_webhook(&self.webhook)?;
        if self.github.repositories.is_empty() {
            return Err(ConfigError::MissingValue {
                path: "github.repositories".to_owned(),
            });
        }
        let mut github_repositories = std::collections::BTreeSet::new();
        for repository in &self.github.repositories {
            ensure_value("github.repositories[].repository", &repository.repository)?;
            ensure_value("github.repositories[].base_branch", &repository.base_branch)?;
            if !repository.workspace_root.is_absolute() {
                return Err(ConfigError::RelativePath {
                    path: format!(
                        "github.repositories[{}].workspace_root",
                        repository.repository
                    ),
                });
            }
            if repository.required_checks.is_empty() {
                return Err(ConfigError::MissingValue {
                    path: format!(
                        "github.repositories[{}].required_checks",
                        repository.repository
                    ),
                });
            }
            if !github_repositories.insert(&repository.repository) {
                return Err(ConfigError::Domain {
                    path: "github.repositories".to_owned(),
                    message: format!("duplicate repository {}", repository.repository),
                });
            }
        }
        if self.github.request_timeout_seconds == 0 {
            return Err(ConfigError::MustBePositive {
                path: "github.request_timeout_seconds".to_owned(),
            });
        }
        if self.linear.complexity_mapping.is_empty() {
            return Err(ConfigError::MissingValue {
                path: "linear.complexity_mapping".to_owned(),
            });
        }
        if self.linear.supported_type_labels.is_empty() {
            return Err(ConfigError::MissingValue {
                path: "linear.supported_type_labels".to_owned(),
            });
        }
        if self.linear.repository_mappings.is_empty() {
            return Err(ConfigError::MissingValue {
                path: "linear.repository_mappings".to_owned(),
            });
        }
        for (path, value) in [
            (
                "concurrency.total_active_harness_runs",
                self.concurrency.total_active_harness_runs,
            ),
            (
                "concurrency.ai_initiated_active_harness_runs",
                self.concurrency.ai_initiated_active_harness_runs,
            ),
            (
                "concurrency.mutating_runs_per_repository",
                self.concurrency.mutating_runs_per_repository,
            ),
            (
                "concurrency.active_runs_per_ticket",
                self.concurrency.active_runs_per_ticket,
            ),
            (
                "concurrency.cleanup_global",
                self.concurrency.cleanup_global,
            ),
        ] {
            if value == 0 {
                return Err(ConfigError::MustBePositive {
                    path: path.to_owned(),
                });
            }
        }
        if self.concurrency.ai_initiated_active_harness_runs
            > self.concurrency.total_active_harness_runs
        {
            return Err(ConfigError::MustBePositive {
                path:
                    "concurrency.ai_initiated_active_harness_runs exceeds total_active_harness_runs"
                        .to_owned(),
            });
        }
        validate_rollout(&self.rollout, &self.linear, &self.concurrency)?;
        if self.security.reviewer_can_push {
            return Err(ConfigError::ReviewerCanPush);
        }
        if self.security.credential_can_merge {
            return Err(ConfigError::CredentialCanMerge);
        }
        validate_runtime_paths(&self.runtime)?;
        if self.runtime.database_max_connections == 0 {
            return Err(ConfigError::MustBePositive {
                path: "runtime.database_max_connections".to_owned(),
            });
        }
        if !self.server.admin_bind.ip().is_loopback() {
            return Err(ConfigError::AdminMustBeLoopback);
        }

        let mut capabilities = HarnessCapabilityRegistry::default();
        for (harness, config) in &self.harnesses {
            ensure_value(
                &format!("harnesses.{harness}.executable"),
                &config.executable,
            )?;
            ensure_credential_reference(
                &format!("harnesses.{harness}.credential_ref"),
                &config.credential_ref,
            )?;
            if config.models.is_empty() || config.efforts.is_empty() {
                return Err(ConfigError::MissingValue {
                    path: format!("harnesses.{harness}.models/efforts"),
                });
            }
            capabilities.register(
                harness.clone(),
                config.models.clone(),
                config.efforts.clone(),
            );
        }

        let policy_version =
            DispatchPolicyVersion::new(self.dispatch.policy_version).map_err(|error| {
                ConfigError::Domain {
                    path: "dispatch.policy_version".to_owned(),
                    message: error.to_string(),
                }
            })?;
        let policy = DispatchPolicy {
            policy_version,
            rules: self
                .dispatch
                .rules
                .iter()
                .map(|rule| {
                    Ok(DispatchRule {
                        id: DispatchRuleId::new(&rule.id).map_err(|error| ConfigError::Domain {
                            path: format!("dispatch.rules.{}.id", rule.id),
                            message: error.to_string(),
                        })?,
                        role: rule.when.role,
                        complexity: rule.when.complexity.clone(),
                        candidates: rule.candidates.clone(),
                    })
                })
                .collect::<Result<Vec<_>, ConfigError>>()?,
        };
        policy.validate(&capabilities)?;

        Ok(ValidatedConfig {
            config: self,
            policy,
            capabilities,
        })
    }
}

fn ensure_value(path: &str, value: &str) -> Result<(), ConfigError> {
    if value.trim().is_empty() || value.starts_with("REPLACE_ME_") {
        return Err(ConfigError::MissingValue {
            path: path.to_owned(),
        });
    }
    Ok(())
}

fn ensure_credential_reference(path: &str, reference: &str) -> Result<(), ConfigError> {
    if reference
        .strip_prefix("env:")
        .is_some_and(|name| !name.is_empty())
        || reference
            .strip_prefix("systemd:credentials/")
            .is_some_and(|name| !name.is_empty())
    {
        return Ok(());
    }
    Err(ConfigError::InvalidCredentialReference {
        path: path.to_owned(),
    })
}

fn validate_webhook(webhook: &WebhookConfig) -> Result<(), ConfigError> {
    let path = webhook.path.as_str();
    if !path.starts_with('/')
        || path.len() < 2
        || path.contains(['?', '#', ' '])
        || path.starts_with("/health")
        || path.starts_with("/admin")
    {
        return Err(ConfigError::WebhookPathInvalid);
    }
    if webhook.replay_window_seconds == 0 || webhook.max_body_bytes == 0 {
        return Err(ConfigError::MustBePositive {
            path: "webhook.replay_window_seconds/max_body_bytes".to_owned(),
        });
    }
    if webhook.replay_window_seconds > MAX_REPLAY_WINDOW_SECONDS {
        return Err(ConfigError::OutOfRange {
            path: "webhook.replay_window_seconds".to_owned(),
            detail: format!("must not exceed {MAX_REPLAY_WINDOW_SECONDS} seconds"),
        });
    }
    if webhook.max_body_bytes > MAX_WEBHOOK_BODY_BYTES {
        return Err(ConfigError::OutOfRange {
            path: "webhook.max_body_bytes".to_owned(),
            detail: format!("must not exceed {MAX_WEBHOOK_BODY_BYTES} bytes"),
        });
    }
    Ok(())
}

/// The rollout gate fails closed: enabling Linear writes requires an explicit
/// allowlist for every dimension the pilot restricts.
fn validate_rollout(
    rollout: &RolloutConfig,
    linear: &LinearConfig,
    concurrency: &ConcurrencyConfig,
) -> Result<(), ConfigError> {
    if rollout.max_active_harness_runs == 0 {
        return Err(ConfigError::MustBePositive {
            path: "rollout.max_active_harness_runs".to_owned(),
        });
    }
    if rollout.max_active_harness_runs > concurrency.total_active_harness_runs {
        return Err(ConfigError::RolloutInconsistent {
            path: "rollout.max_active_harness_runs".to_owned(),
            detail: "must not exceed concurrency.total_active_harness_runs".to_owned(),
        });
    }
    if !rollout.linear_writes_enabled {
        return Ok(());
    }
    for (path, values) in [
        ("rollout.allowed_team_ids", &rollout.allowed_team_ids),
        (
            "rollout.allowed_repositories",
            &rollout.allowed_repositories,
        ),
        ("rollout.allowed_type_labels", &rollout.allowed_type_labels),
    ] {
        if values.is_empty() {
            return Err(ConfigError::RolloutInconsistent {
                path: path.to_owned(),
                detail: "must not be empty while linear_writes_enabled is true".to_owned(),
            });
        }
    }
    if !rollout.allowed_team_ids.contains(&linear.team_id) {
        return Err(ConfigError::RolloutInconsistent {
            path: "rollout.allowed_team_ids".to_owned(),
            detail: "must contain linear.team_id".to_owned(),
        });
    }
    for repository in &rollout.allowed_repositories {
        if !linear
            .repository_mappings
            .iter()
            .any(|mapping| mapping.repository.as_str() == repository.as_str() && mapping.enabled)
        {
            return Err(ConfigError::RolloutInconsistent {
                path: "rollout.allowed_repositories".to_owned(),
                detail: format!("{repository} is not an enabled linear.repository_mappings entry"),
            });
        }
    }
    for label in &rollout.allowed_type_labels {
        if !linear.supported_type_labels.contains(label) {
            return Err(ConfigError::RolloutInconsistent {
                path: "rollout.allowed_type_labels".to_owned(),
                detail: format!("{label} is not in linear.supported_type_labels"),
            });
        }
    }
    Ok(())
}

fn validate_runtime_paths(runtime: &RuntimeConfig) -> Result<(), ConfigError> {
    let paths = [
        ("runtime.database_path", &runtime.database_path),
        ("runtime.data_root", &runtime.data_root),
        ("runtime.backup_root", &runtime.backup_root),
        ("runtime.workspace_root", &runtime.workspace_root),
        ("runtime.evidence_root", &runtime.evidence_root),
    ];
    for (name, path) in paths {
        if !path.is_absolute() {
            return Err(ConfigError::RelativePath {
                path: name.to_owned(),
            });
        }
    }
    for (index, (first_name, first)) in paths.iter().enumerate() {
        for (second_name, second) in paths.iter().skip(index + 1) {
            if first == second {
                return Err(ConfigError::DuplicatePath {
                    first: (*first_name).to_owned(),
                    second: (*second_name).to_owned(),
                });
            }
        }
    }
    for (name, timeout) in [
        (
            "runtime.implementation_timeout_seconds",
            runtime.implementation_timeout_seconds,
        ),
        (
            "runtime.review_timeout_seconds",
            runtime.review_timeout_seconds,
        ),
    ] {
        if timeout == 0 {
            return Err(ConfigError::MustBePositive {
                path: name.to_owned(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_CONFIG: &str = r#"
schema_version: 2
linear:
  organization_id: org
  team_id: team
  ready_state_id: ready
  in_progress_state_id: progress
  in_review_state_id: review
  specs_needed_state_id: specs
  blocked_state_id: blocked
  done_state_id: done
  canceled_state_id: canceled
  bot_actor_id: bot
  credential_ref: env:LINEAR_TOKEN
  complexity_mapping: {1: small, 2: medium, 3: large, 5: xlarge}
  supported_type_labels: [type:bug]
  repository_mappings: [{label: repo:spire, repository: owner/spire, enabled: true}]
github:
  installation_id: installation
  credential_ref: systemd:credentials/github-key
  webhook_secret_ref: env:GITHUB_WEBHOOK_SECRET
  request_timeout_seconds: 10
  repositories:
    - {repository: owner/repository, base_branch: main, required_checks: [test], workspace_root: /var/lib/spire/workspaces/owner-repository}
cloudflare: {account_ref: account, zone_ref: zone, webhook_hostname: spire.example.test}
harnesses:
  codex: {executable: codex, credential_ref: env:CODEX_TOKEN, models: [codex-model], efforts: [medium]}
  claude-code: {executable: claude, credential_ref: env:CLAUDE_TOKEN, models: [claude-model], efforts: [medium]}
dispatch:
  policy_version: 1
  rules:
    - id: implementation
      when: {role: implementation, complexity: [small, medium, large, xlarge]}
      candidates:
        - {harness: codex, model: codex-model, effort: medium}
        - {harness: claude-code, model: claude-model, effort: medium}
    - id: review
      when: {role: review, complexity: [small, medium, large, xlarge]}
      candidates:
        - {harness: claude-code, model: claude-model, effort: medium}
        - {harness: codex, model: codex-model, effort: medium}
concurrency: {total_active_harness_runs: 3, ai_initiated_active_harness_runs: 1, mutating_runs_per_repository: 1, active_runs_per_ticket: 1, cleanup_global: 1}
security: {admin_access: loopback, maker_push_mode: mechanical_publisher, reviewer_can_push: false, credential_can_merge: false}
runtime: {database_path: /var/lib/spire/data/spire.db, database_max_connections: 4, data_root: /var/lib/spire/data, backup_root: /var/lib/spire/backups, workspace_root: /var/lib/spire/workspaces, evidence_root: /var/lib/spire/evidence, implementation_timeout_seconds: 7200, review_timeout_seconds: 1800}
server: {api_bind: 127.0.0.1:8080, admin_bind: 127.0.0.1:8081}
webhook: {path: /webhooks/linear, signing_secret_ref: systemd:credentials/linear-webhook-secret, webhook_id: webhook, replay_window_seconds: 60, max_body_bytes: 262144}
rollout: {linear_writes_enabled: false, allowed_team_ids: [], allowed_repositories: [], allowed_type_labels: [], max_active_harness_runs: 1}
"#;

    fn enabled_rollout() -> String {
        VALID_CONFIG.replace(
            "rollout: {linear_writes_enabled: false, allowed_team_ids: [], allowed_repositories: [], allowed_type_labels: [], max_active_harness_runs: 1}",
            "rollout: {linear_writes_enabled: true, allowed_team_ids: [team], allowed_repositories: [owner/spire], allowed_type_labels: [type:bug], max_active_harness_runs: 1}",
        )
    }

    #[test]
    fn validates_complete_provider_neutral_config() {
        let config = Config::from_yaml(VALID_CONFIG).unwrap().validate().unwrap();
        assert_eq!(config.webhook_limits().replay_window_seconds, 60);
        assert!(!config.rollout_gate(false).linear_writes_enabled);
        assert_eq!(
            config.config.linear.state_id(LinearStateKind::InProgress),
            "progress"
        );
        assert_eq!(
            config.config.linear.state_kind("review"),
            Some(LinearStateKind::InReview)
        );
    }

    #[test]
    fn rejects_unknown_keys_and_empty_policy() {
        assert!(Config::from_yaml("schema_version: 2\nunknown: value").is_err());
        let invalid = VALID_CONFIG.replace("policy_version: 1", "policy_version: 0");
        assert!(matches!(
            Config::from_yaml(&invalid).unwrap().validate(),
            Err(ConfigError::Domain { .. })
        ));
        let invalid = VALID_CONFIG.replace("schema_version: 2", "schema_version: 1");
        assert!(matches!(
            Config::from_yaml(&invalid).unwrap().validate(),
            Err(ConfigError::UnsupportedSchemaVersion)
        ));
    }

    #[test]
    fn webhook_ingress_limits_fail_closed() {
        for (from, to) in [
            ("path: /webhooks/linear", "path: /admin/webhooks"),
            ("path: /webhooks/linear", "path: webhooks/linear"),
            ("replay_window_seconds: 60", "replay_window_seconds: 0"),
            ("replay_window_seconds: 60", "replay_window_seconds: 3600"),
            ("max_body_bytes: 262144", "max_body_bytes: 8388608"),
            (
                "signing_secret_ref: systemd:credentials/linear-webhook-secret",
                "signing_secret_ref: /etc/spire/secret",
            ),
        ] {
            let invalid = VALID_CONFIG.replace(from, to);
            assert!(
                Config::from_yaml(&invalid).unwrap().validate().is_err(),
                "{to} must be rejected"
            );
        }
    }

    #[test]
    fn enabling_linear_writes_requires_a_consistent_allowlist() {
        assert!(
            Config::from_yaml(&enabled_rollout())
                .unwrap()
                .validate()
                .is_ok()
        );
        for (from, to) in [
            ("allowed_team_ids: [team]", "allowed_team_ids: [other-team]"),
            (
                "allowed_repositories: [owner/spire]",
                "allowed_repositories: []",
            ),
            (
                "allowed_repositories: [owner/spire]",
                "allowed_repositories: [owner/other]",
            ),
            (
                "allowed_type_labels: [type:bug]",
                "allowed_type_labels: [type:chore]",
            ),
            ("max_active_harness_runs: 1}", "max_active_harness_runs: 9}"),
        ] {
            let invalid = enabled_rollout().replace(from, to);
            assert!(
                matches!(
                    Config::from_yaml(&invalid).unwrap().validate(),
                    Err(ConfigError::RolloutInconsistent { .. })
                ),
                "{to} must be rejected"
            );
        }
    }
}
