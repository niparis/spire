//! Pure first-run onboarding logic: workflow-state suggestions, complexity
//! mapping, and configuration rendering.
//!
//! Nothing here performs IO or prompting. Suggestions are advisory only; the
//! caller is required to confirm every semantic binding before rendering, which
//! is what keeps a renamed or ambiguous Linear state from silently becoming
//! Spire's ready queue.

use std::{collections::BTreeMap, fmt::Write as _, path::Path};

use spire_domain::{ComplexityClass, ComplexityEstimate, Effort, HarnessId, ModelId};

use crate::{
    LinearEstimateScale, LinearStateCategory, LinearStateKind, LinearWorkflowState, ResolvedPaths,
    SCHEMA_VERSION,
};

/// Values `spire init` cannot discover yet. They are written with the
/// `REPLACE_ME_` prefix that `Config::validate` already rejects, so an
/// incomplete installation fails closed at its named field instead of starting.
pub const UNRESOLVED_FIELDS: &[(&str, &str)] = &[
    (
        "github.installation_id",
        "install the registered GitHub App, then record its installation ID",
    ),
    (
        "github.repositories",
        "add the repositories Spire may operate on",
    ),
    (
        "cloudflare.account_ref",
        "name the Cloudflare account that fronts the webhook ingress",
    ),
    (
        "cloudflare.zone_ref",
        "name the Cloudflare zone that fronts the webhook ingress",
    ),
    (
        "cloudflare.webhook_hostname",
        "set the public hostname Linear and GitHub deliver webhooks to",
    ),
    (
        "webhook.webhook_id",
        "record the Linear webhook ID after creating the webhook",
    ),
];

const PLACEHOLDER: &str = "REPLACE_ME_";

/// The type labels Spire admits by default. They are a Spire convention rather
/// than a Linear discovery, so init offers them and the operator confirms.
pub const DEFAULT_TYPE_LABELS: &[&str] =
    &["type:bug", "type:feature", "type:refactor", "type:chore"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessSelection {
    pub provider: HarnessId,
    pub model: ModelId,
    pub effort: Effort,
}

/// Everything an operator confirmed during `spire init`. Secrets never appear
/// here: the API key lives in the secret store and is referenced by neither this
/// value nor the rendered configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnboardingAnswers {
    pub organization_id: String,
    pub team_id: String,
    pub bot_actor_id: String,
    pub state_ids: BTreeMap<LinearStateKind, String>,
    pub complexity_mapping: BTreeMap<ComplexityEstimate, ComplexityClass>,
    pub type_labels: Vec<String>,
    pub maker: HarnessSelection,
    pub reviewer: HarnessSelection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnboardingError {
    NoEstimateScale,
    MissingState(LinearStateKind),
    SameMakerReviewerProvider,
    NoTypeLabels,
}

impl std::fmt::Display for OnboardingError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoEstimateScale => write!(
                formatter,
                "the selected Linear team does not use issue estimates, so Spire cannot classify complexity"
            ),
            Self::MissingState(kind) => {
                write!(
                    formatter,
                    "no workflow state was confirmed for {}",
                    kind.as_str()
                )
            }
            Self::SameMakerReviewerProvider => {
                write!(formatter, "the maker and reviewer providers must differ")
            }
            Self::NoTypeLabels => {
                write!(formatter, "at least one supported type label is required")
            }
        }
    }
}

impl std::error::Error for OnboardingError {}

/// Ranks the workflow states most likely to carry a semantic meaning, best
/// first. The caller presents the ranking and takes the operator's answer; an
/// empty result simply means every state is equally plausible.
pub fn rank_states(kind: LinearStateKind, states: &[LinearWorkflowState]) -> Vec<usize> {
    let mut ranked: Vec<(u8, usize)> = states
        .iter()
        .enumerate()
        .filter_map(|(index, state)| suggestion_score(kind, state).map(|score| (score, index)))
        .collect();
    // Stable within a score so Linear's own workflow order breaks ties.
    ranked.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    ranked.into_iter().map(|(_, index)| index).collect()
}

/// A higher score is a stronger suggestion. A name match outranks a category
/// match because several Linear states share one category.
fn suggestion_score(kind: LinearStateKind, state: &LinearWorkflowState) -> Option<u8> {
    let name = state.name.to_ascii_lowercase();
    let (names, category) = match kind {
        LinearStateKind::Ready => (
            ["ready", "todo", "to do", "backlog"].as_slice(),
            LinearStateCategory::Unstarted,
        ),
        LinearStateKind::InProgress => (
            ["in progress", "started", "doing"].as_slice(),
            LinearStateCategory::Started,
        ),
        LinearStateKind::InReview => (
            ["in review", "review", "reviewing"].as_slice(),
            LinearStateCategory::Started,
        ),
        LinearStateKind::SpecsNeeded => (
            ["specs needed", "needs specs", "triage", "needs info"].as_slice(),
            LinearStateCategory::Triage,
        ),
        LinearStateKind::Blocked => (
            ["blocked", "on hold", "paused"].as_slice(),
            LinearStateCategory::Backlog,
        ),
        LinearStateKind::Done => (
            ["done", "completed", "shipped", "merged"].as_slice(),
            LinearStateCategory::Completed,
        ),
        LinearStateKind::Canceled => (
            ["canceled", "cancelled", "duplicate", "wont do"].as_slice(),
            LinearStateCategory::Canceled,
        ),
    };
    let name_rank = names.iter().position(|candidate| name == *candidate);
    match (name_rank, state.category == category) {
        // Saturates at 3 so a long alias list cannot outrank an exact match.
        (Some(rank), _) => {
            Some(u8::try_from(names.len().saturating_sub(rank)).unwrap_or(u8::MAX) + 3)
        }
        (None, true) => Some(1),
        (None, false) => None,
    }
}

/// Splits a team's estimate scale into Spire's four complexity classes. Buckets
/// are proportional so a five-point and an eight-point scale both cover every
/// class without the operator inventing a mapping.
pub fn suggest_complexity_mapping(
    scale: &LinearEstimateScale,
) -> Result<BTreeMap<ComplexityEstimate, ComplexityClass>, OnboardingError> {
    if scale.points.is_empty() {
        return Err(OnboardingError::NoEstimateScale);
    }
    let count = scale.points.len();
    let mut mapping = BTreeMap::new();
    for (index, point) in scale.points.iter().enumerate() {
        let Ok(estimate) = ComplexityEstimate::new(*point) else {
            continue;
        };
        let bucket = (index * ComplexityClass::ALL.len()) / count;
        mapping.insert(estimate, ComplexityClass::ALL[bucket.min(3)]);
    }
    if mapping.is_empty() {
        return Err(OnboardingError::NoEstimateScale);
    }
    Ok(mapping)
}

/// Renders the schema-4 configuration `spire init` writes. Discovered values are
/// bound; everything init cannot yet learn is emitted as a named placeholder.
pub fn render_config(
    answers: &OnboardingAnswers,
    paths: &ResolvedPaths,
) -> Result<String, OnboardingError> {
    if answers.maker.provider == answers.reviewer.provider {
        return Err(OnboardingError::SameMakerReviewerProvider);
    }
    if answers.type_labels.is_empty() {
        return Err(OnboardingError::NoTypeLabels);
    }
    let state = |kind: LinearStateKind| {
        answers
            .state_ids
            .get(&kind)
            .filter(|id| !id.trim().is_empty())
            .ok_or(OnboardingError::MissingState(kind))
    };

    let data_root = paths.data_root.as_path();
    let mut yaml = String::new();
    let _ = write!(
        yaml,
        "# Generated by `spire init`. Values prefixed with {PLACEHOLDER} are not yet
# resolved; `spire config validate` names each one until they are.
schema_version: {SCHEMA_VERSION}
linear:
  organization_id: {organization}
  team_id: {team}
  ready_state_id: {ready}
  in_progress_state_id: {in_progress}
  in_review_state_id: {in_review}
  specs_needed_state_id: {specs_needed}
  blocked_state_id: {blocked}
  done_state_id: {done}
  canceled_state_id: {canceled}
  bot_actor_id: {bot}
  complexity_mapping:
",
        organization = scalar(&answers.organization_id),
        team = scalar(&answers.team_id),
        ready = scalar(state(LinearStateKind::Ready)?),
        in_progress = scalar(state(LinearStateKind::InProgress)?),
        in_review = scalar(state(LinearStateKind::InReview)?),
        specs_needed = scalar(state(LinearStateKind::SpecsNeeded)?),
        blocked = scalar(state(LinearStateKind::Blocked)?),
        done = scalar(state(LinearStateKind::Done)?),
        canceled = scalar(state(LinearStateKind::Canceled)?),
        bot = scalar(&answers.bot_actor_id),
    );
    for (estimate, class) in &answers.complexity_mapping {
        let _ = writeln!(
            yaml,
            "    {}: {}",
            estimate.value(),
            complexity_class_name(*class)
        );
    }
    yaml.push_str("  supported_type_labels:\n");
    for label in &answers.type_labels {
        let _ = writeln!(yaml, "    - {}", scalar(label));
    }
    let _ = write!(
        yaml,
        "  repository_mappings: []
github:
  installation_id: {PLACEHOLDER}GITHUB_INSTALLATION_ID
  request_timeout_seconds: 10
  repositories: []
cloudflare:
  account_ref: {PLACEHOLDER}CLOUDFLARE_ACCOUNT
  zone_ref: {PLACEHOLDER}CLOUDFLARE_ZONE
  webhook_hostname: {PLACEHOLDER}WEBHOOK_HOSTNAME
harnesses:
  maker:
    provider: {maker_provider}
    model: {maker_model}
    effort: {maker_effort}
  reviewer:
    provider: {reviewer_provider}
    model: {reviewer_model}
    effort: {reviewer_effort}
concurrency:
  total_active_harness_runs: 2
  ai_initiated_active_harness_runs: 1
  mutating_runs_per_repository: 1
  active_runs_per_ticket: 1
  cleanup_global: 1
security:
  admin_access: loopback
  maker_push_mode: mechanical_publisher
  reviewer_can_push: false
  credential_can_merge: false
runtime:
  database_path: {database}
  database_max_connections: 4
  data_root: {data}
  backup_root: {backups}
  workspace_root: {workspaces}
  evidence_root: {evidence}
  implementation_timeout_seconds: 7200
  review_timeout_seconds: 1800
operations:
  minimum_free_disk_bytes: 10737418240
  minimum_free_inodes: 10000
  workspace_terminal_retention_seconds: 604800
  evidence_terminal_retention_seconds: 604800
  backup_retention_count: 7
  backup_interval_seconds: 86400
server:
  api_bind: 127.0.0.1:8080
  admin_bind: 127.0.0.1:8081
webhook:
  path: /webhooks/linear
  signing_secret_ref: env:SPIRE_LINEAR_WEBHOOK_SECRET
  webhook_id: {PLACEHOLDER}LINEAR_WEBHOOK_ID
  replay_window_seconds: 60
  max_body_bytes: 262144
rollout:
  linear_writes_enabled: false
  allowed_team_ids: []
  allowed_repositories: []
  allowed_type_labels: []
  max_active_harness_runs: 1
",
        maker_provider = scalar(answers.maker.provider.as_str()),
        maker_model = scalar(answers.maker.model.as_str()),
        maker_effort = effort_name(answers.maker.effort),
        reviewer_provider = scalar(answers.reviewer.provider.as_str()),
        reviewer_model = scalar(answers.reviewer.model.as_str()),
        reviewer_effort = effort_name(answers.reviewer.effort),
        database = scalar_path(&data_root.join("spire.db")),
        data = scalar_path(data_root),
        backups = scalar_path(&data_root.join("backups")),
        workspaces = scalar_path(&data_root.join("workspaces")),
        evidence = scalar_path(&data_root.join("evidence")),
    );
    Ok(yaml)
}

/// Emits a value as a YAML double-quoted scalar. It always stays on one line, so
/// a provider-supplied name containing a newline cannot introduce a sibling key.
fn scalar(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for character in value.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            control if control.is_control() => {
                let _ = write!(quoted, "\\x{:02x}", control as u32);
            }
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

fn scalar_path(value: &Path) -> String {
    scalar(&value.to_string_lossy())
}

fn complexity_class_name(class: ComplexityClass) -> &'static str {
    match class {
        ComplexityClass::Small => "small",
        ComplexityClass::Medium => "medium",
        ComplexityClass::Large => "large",
        ComplexityClass::Xlarge => "xlarge",
    }
}

fn effort_name(effort: Effort) -> &'static str {
    match effort {
        Effort::Low => "low",
        Effort::Medium => "medium",
        Effort::High => "high",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Config, InstallationProfile};
    use std::path::PathBuf;

    fn state(id: &str, name: &str, category: LinearStateCategory) -> LinearWorkflowState {
        LinearWorkflowState {
            id: id.to_owned(),
            name: name.to_owned(),
            category,
        }
    }

    fn paths() -> ResolvedPaths {
        ResolvedPaths {
            profile: InstallationProfile::User,
            config_file: PathBuf::from("/home/operator/.config/spire/spire.yaml"),
            config_root: PathBuf::from("/home/operator/.config/spire"),
            data_root: PathBuf::from("/home/operator/.local/share/spire"),
            state_root: PathBuf::from("/home/operator/.local/state/spire"),
            cache_root: PathBuf::from("/home/operator/.cache/spire"),
        }
    }

    fn answers() -> OnboardingAnswers {
        OnboardingAnswers {
            organization_id: "org-1".to_owned(),
            team_id: "team-1".to_owned(),
            bot_actor_id: "viewer-1".to_owned(),
            state_ids: LinearStateKind::ALL
                .into_iter()
                .map(|kind| (kind, format!("state-{}", kind.as_str())))
                .collect(),
            complexity_mapping: suggest_complexity_mapping(&LinearEstimateScale {
                kind: "fibonacci".to_owned(),
                points: vec![1, 2, 3, 5, 8],
            })
            .unwrap(),
            type_labels: DEFAULT_TYPE_LABELS
                .iter()
                .map(|label| (*label).to_owned())
                .collect(),
            maker: HarnessSelection {
                provider: HarnessId::new("codex").unwrap(),
                model: ModelId::new("codex-model").unwrap(),
                effort: Effort::High,
            },
            reviewer: HarnessSelection {
                provider: HarnessId::new("claude-code").unwrap(),
                model: ModelId::new("claude-model").unwrap(),
                effort: Effort::Medium,
            },
        }
    }

    #[test]
    fn an_exact_name_outranks_a_shared_category() {
        let states = [
            state("a", "Started", LinearStateCategory::Started),
            state("b", "In Progress", LinearStateCategory::Started),
        ];
        let ranked = rank_states(LinearStateKind::InProgress, &states);
        assert_eq!(ranked.first(), Some(&1));
        assert_eq!(ranked.len(), 2);
    }

    #[test]
    fn a_renamed_state_still_ranks_through_its_category() {
        let states = [state("a", "Cooking", LinearStateCategory::Started)];
        assert_eq!(rank_states(LinearStateKind::InProgress, &states), [0]);
        // An unrecognized category offers nothing rather than guessing.
        let unrecognized = [state("a", "Cooking", LinearStateCategory::Unrecognized)];
        assert!(rank_states(LinearStateKind::InProgress, &unrecognized).is_empty());
    }

    #[test]
    fn every_scale_covers_all_four_complexity_classes() {
        for points in [
            vec![1u8, 2, 4, 8],
            vec![1, 2, 3, 5, 8],
            vec![1, 2, 3, 4, 5],
            vec![1, 2, 3, 5, 8, 13],
        ] {
            let mapping = suggest_complexity_mapping(&LinearEstimateScale {
                kind: "test".to_owned(),
                points,
            })
            .unwrap();
            let classes: std::collections::BTreeSet<_> = mapping.values().copied().collect();
            assert_eq!(classes.len(), ComplexityClass::ALL.len(), "{mapping:?}");
        }
        assert_eq!(
            suggest_complexity_mapping(&LinearEstimateScale {
                kind: "notUsed".to_owned(),
                points: Vec::new(),
            }),
            Err(OnboardingError::NoEstimateScale)
        );
    }

    #[test]
    fn the_generated_configuration_parses_and_fails_closed_on_its_placeholders() {
        let rendered = render_config(&answers(), &paths()).unwrap();
        let config = Config::from_yaml(&rendered).expect("init must generate a parseable schema");

        assert_eq!(config.linear.team_id, "team-1");
        assert_eq!(config.linear.bot_actor_id, "viewer-1");
        assert!(!config.rollout.linear_writes_enabled);
        assert!(config.linear.repository_mappings.is_empty());

        let error = config
            .validate()
            .expect_err("placeholders must not validate");
        assert!(
            error.to_string().contains("github.installation_id"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn a_provider_supplied_name_cannot_break_out_of_its_value() {
        let mut hostile = answers();
        hostile.organization_id = "org\nrollout:\n  linear_writes_enabled: true".to_owned();
        let rendered = render_config(&hostile, &paths()).unwrap();
        let config = Config::from_yaml(&rendered).expect("the injected newline stays quoted");

        assert!(!config.rollout.linear_writes_enabled);
        assert!(config.linear.organization_id.contains('\n'));
    }

    #[test]
    fn rendering_refuses_an_incomplete_or_single_provider_selection() {
        let mut missing = answers();
        missing.state_ids.remove(&LinearStateKind::Blocked);
        assert_eq!(
            render_config(&missing, &paths()),
            Err(OnboardingError::MissingState(LinearStateKind::Blocked))
        );

        let mut same = answers();
        same.reviewer.provider = same.maker.provider.clone();
        assert_eq!(
            render_config(&same, &paths()),
            Err(OnboardingError::SameMakerReviewerProvider)
        );
    }
}
