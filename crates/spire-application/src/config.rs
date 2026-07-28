use std::{collections::BTreeMap, fs, net::SocketAddr, path::PathBuf};

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
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubConfig {
    pub repository: String,
    pub base_branch: String,
    pub required_checks: Vec<String>,
    pub installation_id: String,
    pub credential_ref: String,
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
    #[error("configuration schema_version must be 1")]
    UnsupportedSchemaVersion,
    #[error("configuration value {path} is missing or is still a placeholder")]
    MissingValue { path: String },
    #[error("credential reference {path} must use env:NAME or systemd:credentials/NAME")]
    InvalidCredentialReference { path: String },
    #[error("configuration value {path} must be greater than zero")]
    MustBePositive { path: String },
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
        if self.schema_version != 1 {
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
            ("github.repository", self.github.repository.as_str()),
            ("github.base_branch", self.github.base_branch.as_str()),
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
        ] {
            ensure_value(path, value)?;
        }
        for (path, reference) in [
            ("linear.credential_ref", self.linear.credential_ref.as_str()),
            ("github.credential_ref", self.github.credential_ref.as_str()),
        ] {
            ensure_credential_reference(path, reference)?;
        }
        if self.linear.complexity_mapping.is_empty() {
            return Err(ConfigError::MissingValue {
                path: "linear.complexity_mapping".to_owned(),
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
        if self.security.reviewer_can_push {
            return Err(ConfigError::ReviewerCanPush);
        }
        if self.security.credential_can_merge {
            return Err(ConfigError::CredentialCanMerge);
        }
        validate_runtime_paths(&self.runtime)?;
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

fn validate_runtime_paths(runtime: &RuntimeConfig) -> Result<(), ConfigError> {
    let paths = [
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
schema_version: 1
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
github:
  repository: owner/repository
  base_branch: main
  required_checks: [test]
  installation_id: installation
  credential_ref: systemd:credentials/github-key
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
runtime: {data_root: /var/lib/spire/data, backup_root: /var/lib/spire/backups, workspace_root: /var/lib/spire/workspaces, evidence_root: /var/lib/spire/evidence, implementation_timeout_seconds: 7200, review_timeout_seconds: 1800}
server: {api_bind: 127.0.0.1:8080, admin_bind: 127.0.0.1:8081}
"#;

    #[test]
    fn validates_complete_provider_neutral_config() {
        assert!(Config::from_yaml(VALID_CONFIG).unwrap().validate().is_ok());
    }

    #[test]
    fn rejects_unknown_keys_and_empty_policy() {
        assert!(Config::from_yaml("schema_version: 1\nunknown: value").is_err());
        let invalid = VALID_CONFIG.replace("policy_version: 1", "policy_version: 0");
        assert!(matches!(
            Config::from_yaml(&invalid).unwrap().validate(),
            Err(ConfigError::Domain { .. })
        ));
    }
}
