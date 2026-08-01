//! Pure first-run onboarding logic: workflow-state suggestions, complexity
//! mapping, and configuration rendering.
//!
//! Nothing here performs IO or prompting. Suggestions are advisory only; the
//! caller is required to confirm every semantic binding before rendering, which
//! is what keeps a renamed or ambiguous Linear state from silently becoming
//! Spire's ready queue.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    path::Path,
};

use serde::{Deserialize, Serialize};
use spire_domain::{ComplexityClass, ComplexityEstimate, Effort, HarnessId, ModelId};

use crate::{
    Config, LinearEstimateScale, LinearStateCategory, LinearStateKind, LinearTeamConfiguration,
    LinearTeamSummary, LinearWorkflowState, ResolvedPaths, SCHEMA_VERSION,
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
const UNSELECTED_MODEL: &str = "unselected";

/// The type labels Spire admits by default. They are a Spire convention rather
/// than a Linear discovery, so init offers them and the operator confirms.
pub const DEFAULT_TYPE_LABELS: &[&str] =
    &["type:bug", "type:feature", "type:refactor", "type:chore"];

/// A value in the editor is independent from the other values. A stale value is
/// retained deliberately: reopening a section can offer it as the default, but
/// the editor must not write it until the operator confirms the dependency again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Editable<T> {
    pub value: Option<T>,
    pub stale_reason: Option<String>,
}

impl<T> Default for Editable<T> {
    fn default() -> Self {
        Self {
            value: None,
            stale_reason: None,
        }
    }
}

impl<T> Editable<T> {
    pub fn new(value: Option<T>) -> Self {
        Self {
            value,
            stale_reason: None,
        }
    }

    pub fn complete(value: T) -> Self {
        Self::new(Some(value))
    }

    pub fn clear_stale(&mut self) {
        self.stale_reason = None;
    }

    pub fn mark_stale(&mut self, reason: impl Into<String>) {
        self.stale_reason = Some(reason.into());
    }

    pub fn is_stale(&self) -> bool {
        self.stale_reason.is_some()
    }
}

/// The editor's independently mutable sections. These names are also used in
/// the trace, so changing one is an observable contract change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OnboardingSection {
    Linear,
    WorkflowStates,
    Complexity,
    Maker,
    Reviewer,
    TypeLabels,
    Rollout,
    Paths,
    ReviewAndWrite,
}

impl OnboardingSection {
    pub const ALL: [Self; 9] = [
        Self::Linear,
        Self::WorkflowStates,
        Self::Complexity,
        Self::Maker,
        Self::Reviewer,
        Self::TypeLabels,
        Self::Rollout,
        Self::Paths,
        Self::ReviewAndWrite,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::WorkflowStates => "workflow_states",
            Self::Complexity => "complexity",
            Self::Maker => "maker",
            Self::Reviewer => "reviewer",
            Self::TypeLabels => "type_labels",
            Self::Rollout => "rollout",
            Self::Paths => "paths",
            Self::ReviewAndWrite => "review_and_write",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionStatus {
    Complete,
    Incomplete { reasons: Vec<String> },
    Stale { reason: String },
}

impl SectionStatus {
    pub fn is_complete(&self) -> bool {
        matches!(self, Self::Complete)
    }

    pub fn is_blocking(&self) -> bool {
        !self.is_complete()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OnboardingRole {
    Maker,
    Reviewer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ComplexitySelection {
    pub scale: LinearEstimateScale,
    pub mapping: BTreeMap<ComplexityEstimate, ComplexityClass>,
}

/// The complete editable document handed to an `OnboardingEditorPort`.
///
/// It contains no secret material. `credential_verified` is capability evidence,
/// not the API key itself. The model is intentionally independent of a terminal,
/// renderer, Tokio runtime, and provider adapter.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OnboardingModel {
    pub credential_verified: bool,
    pub organization_id: Editable<String>,
    pub bot_actor_id: Editable<String>,
    pub team_id: Editable<String>,
    pub workflow_states: BTreeMap<LinearStateKind, Editable<String>>,
    pub complexity: Editable<ComplexitySelection>,
    pub maker: Editable<HarnessSelection>,
    pub reviewer: Editable<HarnessSelection>,
    pub maker_model_confirmed: bool,
    pub reviewer_model_confirmed: bool,
    pub type_labels: Editable<Vec<String>>,
    pub rollout_allowed_team_ids: Editable<Vec<String>>,
    pub catalog_version: Option<String>,
    pub off_catalog_roles: BTreeSet<OnboardingRole>,
}

impl OnboardingModel {
    pub fn empty() -> Self {
        let mut model = Self::default();
        for kind in LinearStateKind::ALL {
            model.workflow_states.insert(kind, Editable::default());
        }
        model.rollout_allowed_team_ids = Editable::complete(Vec::new());
        model
    }

    /// Builds an editor model from the values already written by init. Values
    /// outside this model are preserved by the CLI's round-trip renderer.
    pub fn from_config(config: &Config) -> Self {
        let mut model = Self::empty();
        model.organization_id = Editable::complete(config.linear.organization_id.clone());
        model.bot_actor_id = Editable::complete(config.linear.bot_actor_id.clone());
        model.team_id = Editable::complete(config.linear.team_id.clone());
        for kind in LinearStateKind::ALL {
            model.workflow_states.insert(
                kind,
                Editable::complete(config.linear.state_id(kind).to_owned()),
            );
        }
        model.complexity = Editable::complete(ComplexitySelection {
            scale: LinearEstimateScale {
                kind: "loaded".to_owned(),
                points: config
                    .linear
                    .complexity_mapping
                    .keys()
                    .map(|estimate| estimate.value())
                    .collect(),
            },
            mapping: config.linear.complexity_mapping.clone(),
        });
        model.maker = Editable::complete(HarnessSelection {
            provider: config.harnesses.maker.provider.clone(),
            model: config.harnesses.maker.model.clone(),
            effort: config.harnesses.maker.effort,
        });
        model.reviewer = Editable::complete(HarnessSelection {
            provider: config.harnesses.reviewer.provider.clone(),
            model: config.harnesses.reviewer.model.clone(),
            effort: config.harnesses.reviewer.effort,
        });
        model.maker_model_confirmed = true;
        model.reviewer_model_confirmed = true;
        model.type_labels = Editable::complete(config.linear.supported_type_labels.clone());
        model.rollout_allowed_team_ids =
            Editable::complete(config.rollout.allowed_team_ids.clone());
        model.catalog_version = config.harnesses.catalog_version.clone();
        model
    }

    pub fn set_team(&mut self, team_id: impl Into<String>) -> MutationOutcome {
        let team_id = team_id.into();
        if self.team_id.value.as_deref() == Some(team_id.as_str()) {
            self.team_id.value = Some(team_id);
            return MutationOutcome::default();
        }
        self.team_id.value = Some(team_id.clone());
        self.team_id.clear_stale();
        for field in self.workflow_states.values_mut() {
            field.mark_stale("team changed; workflow state IDs are team-scoped");
        }
        self.complexity
            .mark_stale("team changed; the complexity mapping derives from its estimate scale");
        self.type_labels
            .mark_stale("team changed; supported labels are team-scoped");
        MutationOutcome {
            mutation: "team".to_owned(),
            invalidated: vec![
                OnboardingSection::WorkflowStates,
                OnboardingSection::Complexity,
                OnboardingSection::TypeLabels,
            ],
        }
    }

    pub fn set_maker_provider(&mut self, provider: HarnessId) -> MutationOutcome {
        let Some(current_provider) = self
            .maker
            .value
            .as_ref()
            .map(|maker| maker.provider.clone())
        else {
            self.maker.value = Some(HarnessSelection {
                provider,
                model: ModelId::new(UNSELECTED_MODEL).expect("literal model is valid"),
                effort: Effort::Medium,
            });
            return MutationOutcome::default();
        };
        if current_provider == provider {
            return MutationOutcome::default();
        }
        if let Some(maker) = self.maker.value.as_mut() {
            maker.provider = provider.clone();
        }
        self.maker_model_confirmed = false;
        self.maker
            .mark_stale("maker provider changed; choose a model again");
        let mut outcome = MutationOutcome {
            mutation: "maker.provider".to_owned(),
            ..MutationOutcome::default()
        };
        outcome.invalidated.push(OnboardingSection::Maker);
        if self
            .reviewer
            .value
            .as_ref()
            .is_some_and(|reviewer| reviewer.provider == provider)
        {
            self.reviewer
                .mark_stale("maker provider changed; reviewer must use a different provider");
            outcome.invalidated.push(OnboardingSection::Reviewer);
        }
        outcome
    }

    pub fn set_reviewer_provider(&mut self, provider: HarnessId) -> MutationOutcome {
        if self
            .reviewer
            .value
            .as_ref()
            .is_some_and(|reviewer| reviewer.provider == provider)
        {
            return MutationOutcome::default();
        }
        self.reviewer
            .mark_stale("reviewer provider changed; choose a model again");
        self.reviewer_model_confirmed = false;
        if let Some(reviewer) = self.reviewer.value.as_mut() {
            reviewer.provider = provider;
        } else {
            self.reviewer.value = Some(HarnessSelection {
                provider,
                model: ModelId::new(UNSELECTED_MODEL).expect("literal model is valid"),
                effort: Effort::Medium,
            });
        }
        MutationOutcome {
            mutation: "reviewer.provider".to_owned(),
            invalidated: vec![OnboardingSection::Reviewer],
        }
    }

    pub fn set_maker(&mut self, selection: HarnessSelection) -> MutationOutcome {
        let provider_changed = self
            .maker
            .value
            .as_ref()
            .is_none_or(|current| current.provider != selection.provider);
        self.maker = Editable::complete(selection);
        self.maker_model_confirmed = true;
        let mut outcome = MutationOutcome {
            mutation: "maker".to_owned(),
            ..MutationOutcome::default()
        };
        if provider_changed {
            outcome.invalidated.push(OnboardingSection::Maker);
            if self.reviewer.value.as_ref().is_some_and(|reviewer| {
                reviewer.provider == self.maker.value.as_ref().expect("just set").provider
            }) {
                self.reviewer
                    .mark_stale("maker provider changed; reviewer must use a different provider");
                outcome.invalidated.push(OnboardingSection::Reviewer);
            }
        }
        outcome
    }

    pub fn set_reviewer(&mut self, selection: HarnessSelection) -> MutationOutcome {
        let provider_changed = self
            .reviewer
            .value
            .as_ref()
            .is_none_or(|current| current.provider != selection.provider);
        self.reviewer = Editable::complete(selection);
        self.reviewer_model_confirmed = true;
        MutationOutcome {
            mutation: "reviewer".to_owned(),
            invalidated: provider_changed
                .then_some(OnboardingSection::Reviewer)
                .into_iter()
                .collect(),
        }
    }

    pub fn confirm_section(&mut self, section: OnboardingSection) {
        match section {
            OnboardingSection::WorkflowStates => {
                for state in self.workflow_states.values_mut() {
                    state.clear_stale();
                }
            }
            OnboardingSection::Linear => self.team_id.clear_stale(),
            OnboardingSection::Complexity => self.complexity.clear_stale(),
            OnboardingSection::Maker => self.maker.clear_stale(),
            OnboardingSection::Reviewer => self.reviewer.clear_stale(),
            OnboardingSection::TypeLabels => self.type_labels.clear_stale(),
            OnboardingSection::Rollout
            | OnboardingSection::Paths
            | OnboardingSection::ReviewAndWrite => {}
        }
    }

    /// Choosing a model can strand the effort: a model with a lower ceiling does
    /// not accept the level the previous one did. Rather than let the pair go
    /// out to a provider that will reject it, the effort falls back to the new
    /// model's own default and the change is reported as an invalidation.
    pub fn set_model(
        &mut self,
        role: OnboardingRole,
        model: ModelId,
        catalog: &ModelCatalog,
    ) -> MutationOutcome {
        let section = match role {
            OnboardingRole::Maker => OnboardingSection::Maker,
            OnboardingRole::Reviewer => OnboardingSection::Reviewer,
        };
        let editable = match role {
            OnboardingRole::Maker => &mut self.maker,
            OnboardingRole::Reviewer => &mut self.reviewer,
        };
        let Some(selection) = editable.value.as_mut() else {
            return MutationOutcome::default();
        };
        selection.model = model;
        let mut outcome = MutationOutcome {
            mutation: format!("{}.model", section.as_str()),
            ..MutationOutcome::default()
        };
        if !catalog.accepts_effort(&selection.provider, &selection.model, selection.effort) {
            selection.effort = catalog.default_effort_for(&selection.provider, &selection.model);
            outcome.invalidated.push(section);
        }
        let selection = selection.clone();
        *editable = Editable::complete(selection);
        match role {
            OnboardingRole::Maker => self.maker_model_confirmed = true,
            OnboardingRole::Reviewer => self.reviewer_model_confirmed = true,
        }
        outcome
    }

    pub fn set_effort(&mut self, role: OnboardingRole, effort: Effort) {
        let editable = match role {
            OnboardingRole::Maker => &mut self.maker,
            OnboardingRole::Reviewer => &mut self.reviewer,
        };
        if let Some(selection) = editable.value.as_mut() {
            selection.effort = effort;
        }
    }

    pub fn set_model_catalog_state(&mut self, role: OnboardingRole, off_catalog: bool) {
        if off_catalog {
            self.off_catalog_roles.insert(role);
        } else {
            self.off_catalog_roles.remove(&role);
        }
    }

    pub fn statuses(&self) -> BTreeMap<OnboardingSection, SectionStatus> {
        let mut statuses = BTreeMap::new();
        let linear_reasons = missing_values([
            ("credential", self.credential_verified),
            (
                "team",
                self.team_id
                    .value
                    .as_ref()
                    .is_some_and(|v| !v.trim().is_empty()),
            ),
        ]);
        statuses.insert(
            OnboardingSection::Linear,
            status_for_editable(&self.team_id, linear_reasons),
        );
        let workflow_reasons = LinearStateKind::ALL
            .into_iter()
            .filter(|kind| {
                self.workflow_states
                    .get(kind)
                    .and_then(|state| state.value.as_deref())
                    .is_none_or(|value| value.trim().is_empty())
            })
            .map(|kind| format!("{} is unbound", kind.as_str()))
            .collect();
        statuses.insert(
            OnboardingSection::WorkflowStates,
            status_for_stale_fields(&self.workflow_states, workflow_reasons),
        );
        let complexity_reasons = if self.complexity.value.is_none() {
            vec!["estimate scale and mapping are not accepted".to_owned()]
        } else {
            Vec::new()
        };
        statuses.insert(
            OnboardingSection::Complexity,
            status_for_editable(&self.complexity, complexity_reasons),
        );
        statuses.insert(
            OnboardingSection::Maker,
            status_for_editable(
                &self.maker,
                if !self.maker_model_confirmed
                    || !has_usable_harness_selection(self.maker.value.as_ref())
                {
                    vec!["provider, model, and effort are required".to_owned()]
                } else {
                    Vec::new()
                },
            ),
        );
        let reviewer_reasons = if self
            .maker
            .value
            .as_ref()
            .zip(self.reviewer.value.as_ref())
            .is_some_and(|(maker, reviewer)| maker.provider == reviewer.provider)
        {
            vec!["reviewer provider must differ from maker".to_owned()]
        } else if !self.reviewer_model_confirmed
            || !has_usable_harness_selection(self.reviewer.value.as_ref())
        {
            vec!["provider, model, and effort are required".to_owned()]
        } else {
            Vec::new()
        };
        statuses.insert(
            OnboardingSection::Reviewer,
            status_for_editable(&self.reviewer, reviewer_reasons),
        );
        statuses.insert(
            OnboardingSection::TypeLabels,
            status_for_editable(
                &self.type_labels,
                if self.type_labels.value.as_ref().is_none_or(Vec::is_empty) {
                    vec!["select at least one type label".to_owned()]
                } else {
                    Vec::new()
                },
            ),
        );
        statuses.insert(OnboardingSection::Rollout, SectionStatus::Complete);
        statuses.insert(OnboardingSection::Paths, SectionStatus::Complete);
        let blocking = statuses
            .iter()
            .filter(|(section, status)| {
                **section != OnboardingSection::ReviewAndWrite && status.is_blocking()
            })
            .map(|(section, status)| format!("{}: {}", section.as_str(), status_summary(status)))
            .collect::<Vec<_>>();
        statuses.insert(
            OnboardingSection::ReviewAndWrite,
            if blocking.is_empty() {
                SectionStatus::Complete
            } else {
                SectionStatus::Incomplete { reasons: blocking }
            },
        );
        statuses
    }

    pub fn validate(&self) -> Result<(), OnboardingError> {
        if !self.credential_verified {
            return Err(OnboardingError::CredentialNotVerified);
        }
        if self.team_id.value.as_deref().is_none_or(str::is_empty) {
            return Err(OnboardingError::MissingTeam);
        }
        for (field, value) in [
            ("organization_id", self.organization_id.value.as_deref()),
            ("bot_actor_id", self.bot_actor_id.value.as_deref()),
        ] {
            if value.is_none_or(str::is_empty) {
                return Err(OnboardingError::MissingIdentity(field));
            }
        }
        for kind in LinearStateKind::ALL {
            if self
                .workflow_states
                .get(&kind)
                .and_then(|state| state.value.as_deref())
                .is_none_or(str::is_empty)
            {
                return Err(OnboardingError::MissingState(kind));
            }
        }
        let complexity = self
            .complexity
            .value
            .as_ref()
            .ok_or(OnboardingError::NoEstimateScale)?;
        if complexity.mapping.is_empty() {
            return Err(OnboardingError::NoEstimateScale);
        }
        if self
            .maker
            .value
            .as_ref()
            .zip(self.reviewer.value.as_ref())
            .is_some_and(|(maker, reviewer)| maker.provider == reviewer.provider)
        {
            return Err(OnboardingError::SameMakerReviewerProvider);
        }
        if !self.maker_model_confirmed
            || !self.reviewer_model_confirmed
            || !has_usable_harness_selection(self.maker.value.as_ref())
            || !has_usable_harness_selection(self.reviewer.value.as_ref())
        {
            return Err(OnboardingError::HarnessSelectionMissing);
        }
        if self.type_labels.value.as_ref().is_none_or(Vec::is_empty) {
            return Err(OnboardingError::NoTypeLabels);
        }
        if self
            .statuses()
            .values()
            .any(|status| matches!(status, SectionStatus::Stale { .. }))
        {
            return Err(OnboardingError::StaleSections);
        }
        Ok(())
    }

    pub fn to_answers(&self) -> Result<OnboardingAnswers, OnboardingError> {
        self.validate()?;
        Ok(OnboardingAnswers {
            organization_id: self.organization_id.value.clone().unwrap_or_default(),
            team_id: self.team_id.value.clone().unwrap_or_default(),
            bot_actor_id: self.bot_actor_id.value.clone().unwrap_or_default(),
            state_ids: self
                .workflow_states
                .iter()
                .filter_map(|(kind, state)| state.value.clone().map(|value| (*kind, value)))
                .collect(),
            complexity_mapping: self
                .complexity
                .value
                .as_ref()
                .map(|complexity| complexity.mapping.clone())
                .unwrap_or_default(),
            type_labels: self.type_labels.value.clone().unwrap_or_default(),
            maker: self
                .maker
                .value
                .clone()
                .ok_or(OnboardingError::HarnessSelectionMissing)?,
            reviewer: self
                .reviewer
                .value
                .clone()
                .ok_or(OnboardingError::HarnessSelectionMissing)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MutationOutcome {
    pub mutation: String,
    pub invalidated: Vec<OnboardingSection>,
}

fn missing_values(values: impl IntoIterator<Item = (&'static str, bool)>) -> Vec<String> {
    values
        .into_iter()
        .filter_map(|(name, present)| (!present).then_some(name.to_owned()))
        .collect()
}

fn status_for_editable<T>(field: &Editable<T>, reasons: Vec<String>) -> SectionStatus {
    if let Some(reason) = field.stale_reason.clone() {
        SectionStatus::Stale { reason }
    } else if reasons.is_empty() && field.value.is_some() {
        SectionStatus::Complete
    } else {
        SectionStatus::Incomplete { reasons }
    }
}

fn status_for_stale_fields<T>(
    fields: &BTreeMap<LinearStateKind, Editable<T>>,
    reasons: Vec<String>,
) -> SectionStatus {
    fields
        .values()
        .find_map(|field| field.stale_reason.clone())
        .map_or_else(
            || {
                if reasons.is_empty() {
                    SectionStatus::Complete
                } else {
                    SectionStatus::Incomplete { reasons }
                }
            },
            |reason| SectionStatus::Stale { reason },
        )
}

fn has_usable_harness_selection(selection: Option<&HarnessSelection>) -> bool {
    selection.is_some_and(|selection| selection.model.as_str() != UNSELECTED_MODEL)
}

fn status_summary(status: &SectionStatus) -> String {
    match status {
        SectionStatus::Complete => "complete".to_owned(),
        SectionStatus::Incomplete { reasons } => reasons.join(", "),
        SectionStatus::Stale { reason } => format!("stale: {reason}"),
    }
}

/// A discovery request is a message, not a provider call. The terminal editor
/// can send it to a Tokio task and remain responsive while the task performs IO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryRequest {
    ListTeams,
    TeamConfiguration { team_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryResponse {
    Teams(Result<Vec<LinearTeamSummary>, String>),
    TeamConfiguration {
        team_id: String,
        result: Result<LinearTeamConfiguration, String>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OnboardingDiscovery {
    pub teams: Vec<LinearTeamSummary>,
    pub team_configurations: BTreeMap<String, LinearTeamConfiguration>,
    pub failures: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnboardingEditorResult {
    Committed(Box<OnboardingModel>),
    Cancelled,
}

/// Push-based editor boundary. Implementations receive the whole document and
/// return it only after the operator commits; they do not expose one method per
/// question.
pub trait OnboardingEditorPort {
    type Error;

    fn edit(
        &mut self,
        model: OnboardingModel,
        discovery: OnboardingDiscovery,
    ) -> Result<OnboardingEditorResult, Self::Error>;
}

/// A deterministic, terminal-free editor adapter used by application tests and
/// by higher-level integration tests. An event that is invalid for the current
/// screen fails loudly instead of being silently ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadlessEvent {
    SetCredentialVerified(bool),
    SelectTeam(String),
    ConfirmSection(OnboardingSection),
    SetMaker(HarnessSelection),
    SetReviewer(HarnessSelection),
    SetTypeLabels(Vec<String>),
    SetRolloutTeams(Vec<String>),
    Commit,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadlessEditorError(pub String);

impl std::fmt::Display for HeadlessEditorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::error::Error for HeadlessEditorError {}

#[derive(Debug, Clone)]
pub struct HeadlessOnboardingEditor {
    pub events: Vec<HeadlessEvent>,
}

impl OnboardingEditorPort for HeadlessOnboardingEditor {
    type Error = HeadlessEditorError;

    fn edit(
        &mut self,
        mut model: OnboardingModel,
        _discovery: OnboardingDiscovery,
    ) -> Result<OnboardingEditorResult, Self::Error> {
        for event in self.events.drain(..) {
            match event {
                HeadlessEvent::SetCredentialVerified(value) => model.credential_verified = value,
                HeadlessEvent::SelectTeam(team_id) => {
                    model.set_team(team_id);
                }
                HeadlessEvent::ConfirmSection(section) => model.confirm_section(section),
                HeadlessEvent::SetMaker(selection) => {
                    model.set_maker(selection);
                }
                HeadlessEvent::SetReviewer(selection) => {
                    model.set_reviewer(selection);
                }
                HeadlessEvent::SetTypeLabels(labels) => {
                    model.type_labels = Editable::complete(labels);
                }
                HeadlessEvent::SetRolloutTeams(teams) => {
                    model.rollout_allowed_team_ids = Editable::complete(teams);
                }
                HeadlessEvent::Commit => {
                    model
                        .validate()
                        .map_err(|error| HeadlessEditorError(error.to_string()))?;
                    return Ok(OnboardingEditorResult::Committed(Box::new(model)));
                }
                HeadlessEvent::Cancel => return Ok(OnboardingEditorResult::Cancelled),
            }
        }
        Err(HeadlessEditorError(
            "script ended without commit or cancellation".to_owned(),
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HarnessSelection {
    pub provider: HarnessId,
    pub model: ModelId,
    pub effort: Effort,
}

/// The list of models an operator may pick from without typing one in. Loading
/// it is the CLI's job; every derivation over it is pure and lives here so the
/// editor never decides which effort a model accepts.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct ModelCatalog {
    pub version: String,
    pub providers: BTreeMap<String, Vec<CatalogModel>>,
}

/// Effort belongs to the model rather than the provider: one provider serves
/// models with different reasoning ceilings.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CatalogModel {
    pub id: String,
    pub default_effort: Effort,
    pub efforts: Vec<Effort>,
}

impl ModelCatalog {
    pub fn models_for(&self, provider: &HarnessId) -> Vec<ModelId> {
        self.entries_for(provider)
            .iter()
            .filter_map(|entry| ModelId::new(entry.id.clone()).ok())
            .collect()
    }

    pub fn entries_for(&self, provider: &HarnessId) -> &[CatalogModel] {
        self.providers
            .get(provider.as_str())
            .map_or(&[], Vec::as_slice)
    }

    pub fn entry(&self, provider: &HarnessId, model: &ModelId) -> Option<&CatalogModel> {
        self.entries_for(provider)
            .iter()
            .find(|entry| entry.id == model.as_str())
    }

    /// An off-catalog model declares no ceiling, so every level stays offered
    /// and the operator owns the choice.
    pub fn efforts_for(&self, provider: &HarnessId, model: &ModelId) -> Vec<Effort> {
        self.entry(provider, model)
            .map_or_else(|| Effort::ALL.to_vec(), |entry| entry.efforts.clone())
    }

    pub fn default_effort_for(&self, provider: &HarnessId, model: &ModelId) -> Effort {
        self.entry(provider, model)
            .map_or(Effort::Medium, |entry| entry.default_effort)
    }

    pub fn accepts_effort(&self, provider: &HarnessId, model: &ModelId, effort: Effort) -> bool {
        self.efforts_for(provider, model).contains(&effort)
    }

    /// Advances to the next effort this model accepts, wrapping at the ceiling.
    pub fn next_effort(&self, provider: &HarnessId, model: &ModelId, current: Effort) -> Effort {
        let efforts = self.efforts_for(provider, model);
        let Some(index) = efforts.iter().position(|effort| *effort == current) else {
            return self.default_effort_for(provider, model);
        };
        efforts[(index + 1) % efforts.len()]
    }

    /// Advances to the next model this provider lists, wrapping at the end.
    pub fn next_model(&self, provider: &HarnessId, current: &ModelId) -> Option<ModelId> {
        let models = self.models_for(provider);
        if models.is_empty() {
            return None;
        }
        let next = models
            .iter()
            .position(|model| model == current)
            .map_or(0, |index| (index + 1) % models.len());
        models.get(next).cloned()
    }
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
    CredentialNotVerified,
    MissingIdentity(&'static str),
    MissingTeam,
    NoEstimateScale,
    MissingState(LinearStateKind),
    HarnessSelectionMissing,
    SameMakerReviewerProvider,
    NoTypeLabels,
    StaleSections,
    ExistingConfigurationMalformed(String),
}

impl std::fmt::Display for OnboardingError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CredentialNotVerified => {
                write!(formatter, "the Linear credential is not verified")
            }
            Self::MissingIdentity(field) => write!(
                formatter,
                "{field} is missing from the verified Linear identity"
            ),
            Self::MissingTeam => write!(formatter, "a Linear team must be selected"),
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
            Self::HarnessSelectionMissing => {
                write!(formatter, "maker and reviewer selections are required")
            }
            Self::SameMakerReviewerProvider => {
                write!(formatter, "the maker and reviewer providers must differ")
            }
            Self::NoTypeLabels => {
                write!(formatter, "at least one supported type label is required")
            }
            Self::StaleSections => write!(formatter, "one or more onboarding sections are stale"),
            Self::ExistingConfigurationMalformed(detail) => {
                write!(
                    formatter,
                    "existing configuration cannot be edited: {detail}"
                )
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
        maker_effort = answers.maker.effort.as_str(),
        reviewer_provider = scalar(answers.reviewer.provider.as_str()),
        reviewer_model = scalar(answers.reviewer.model.as_str()),
        reviewer_effort = answers.reviewer.effort.as_str(),
        database = scalar_path(&data_root.join("spire.db")),
        data = scalar_path(data_root),
        backups = scalar_path(&data_root.join("backups")),
        workspaces = scalar_path(&data_root.join("workspaces")),
        evidence = scalar_path(&data_root.join("evidence")),
    );
    Ok(yaml)
}

/// Renders the editable model while retaining configuration values that the
/// onboarding UI does not own. This is the round-trip boundary for reruns: the
/// known editable paths are replaced, while GitHub, Cloudflare, webhook,
/// dispatch, advanced harness, and repository values remain untouched.
pub fn render_model_config(
    model: &OnboardingModel,
    paths: &ResolvedPaths,
    existing: Option<&str>,
) -> Result<String, OnboardingError> {
    let answers = model.to_answers()?;
    let generated = render_config(&answers, paths)?;
    let Some(existing) = existing else {
        if let Some(version) = model.catalog_version.as_deref() {
            let mut generated_value: serde_yaml::Value =
                serde_yaml::from_str(&generated).map_err(|error| {
                    OnboardingError::ExistingConfigurationMalformed(error.to_string())
                })?;
            set_value_at_path(
                &mut generated_value,
                "harnesses.catalog_version",
                serde_yaml::Value::String(version.to_owned()),
            )
            .map_err(OnboardingError::ExistingConfigurationMalformed)?;
            return serde_yaml::to_string(&generated_value).map_err(|error| {
                OnboardingError::ExistingConfigurationMalformed(error.to_string())
            });
        }
        return Ok(generated);
    };
    let mut current: serde_yaml::Value = serde_yaml::from_str(existing)
        .map_err(|error| OnboardingError::ExistingConfigurationMalformed(error.to_string()))?;
    let mut generated: serde_yaml::Value = serde_yaml::from_str(&generated)
        .map_err(|error| OnboardingError::ExistingConfigurationMalformed(error.to_string()))?;
    if let Some(version) = model.catalog_version.as_deref() {
        set_value_at_path(
            &mut generated,
            "harnesses.catalog_version",
            serde_yaml::Value::String(version.to_owned()),
        )
        .map_err(OnboardingError::ExistingConfigurationMalformed)?;
    }
    let editable_paths = [
        "schema_version",
        "linear.organization_id",
        "linear.team_id",
        "linear.ready_state_id",
        "linear.in_progress_state_id",
        "linear.in_review_state_id",
        "linear.specs_needed_state_id",
        "linear.blocked_state_id",
        "linear.done_state_id",
        "linear.canceled_state_id",
        "linear.bot_actor_id",
        "linear.complexity_mapping",
        "linear.supported_type_labels",
        "harnesses.maker",
        "harnesses.reviewer",
        "harnesses.catalog_version",
        "rollout.allowed_team_ids",
    ];
    for path in editable_paths {
        let Some(value) = value_at_path(&generated, path) else {
            continue;
        };
        set_value_at_path(&mut current, path, value.clone())
            .map_err(OnboardingError::ExistingConfigurationMalformed)?;
    }
    serde_yaml::to_string(&current)
        .map_err(|error| OnboardingError::ExistingConfigurationMalformed(error.to_string()))
}

fn value_at_path<'a>(value: &'a serde_yaml::Value, path: &str) -> Option<&'a serde_yaml::Value> {
    let mut current = value;
    for segment in path.split('.') {
        current = current.get(segment)?;
    }
    Some(current)
}

fn set_value_at_path(
    value: &mut serde_yaml::Value,
    path: &str,
    replacement: serde_yaml::Value,
) -> Result<(), String> {
    let segments = path.split('.').collect::<Vec<_>>();
    let mut current = value;
    for segment in &segments[..segments.len().saturating_sub(1)] {
        let mapping = current
            .as_mapping_mut()
            .ok_or_else(|| format!("{path} is not under a YAML mapping"))?;
        current = mapping
            .entry(serde_yaml::Value::String((*segment).to_owned()))
            .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
    }
    let mapping = current
        .as_mapping_mut()
        .ok_or_else(|| format!("{path} is not under a YAML mapping"))?;
    mapping.insert(
        serde_yaml::Value::String(segments.last().unwrap_or(&"").to_string()),
        replacement,
    );
    Ok(())
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

    fn model() -> OnboardingModel {
        let answers = answers();
        let mut model = OnboardingModel::empty();
        model.credential_verified = true;
        model.organization_id = Editable::complete(answers.organization_id);
        model.bot_actor_id = Editable::complete(answers.bot_actor_id);
        model.team_id = Editable::complete(answers.team_id);
        model.workflow_states = answers
            .state_ids
            .into_iter()
            .map(|(kind, id)| (kind, Editable::complete(id)))
            .collect();
        model.complexity = Editable::complete(ComplexitySelection {
            scale: LinearEstimateScale {
                kind: "fibonacci".to_owned(),
                points: vec![1, 2, 3, 5, 8],
            },
            mapping: answers.complexity_mapping,
        });
        model.type_labels = Editable::complete(answers.type_labels);
        model.maker = Editable::complete(answers.maker);
        model.reviewer = Editable::complete(answers.reviewer);
        model.maker_model_confirmed = true;
        model.reviewer_model_confirmed = true;
        model
    }

    #[test]
    fn the_model_is_complete_without_constructing_an_editor() {
        let model = model();
        assert!(model.validate().is_ok());
        assert!(model.statuses().values().all(SectionStatus::is_complete));
    }

    #[test]
    fn changing_team_stales_only_team_scoped_sections() {
        let mut model = model();
        let outcome = model.set_team("team-2");
        assert_eq!(
            outcome.invalidated,
            [
                OnboardingSection::WorkflowStates,
                OnboardingSection::Complexity,
                OnboardingSection::TypeLabels
            ]
        );
        assert!(matches!(
            model.statuses()[&OnboardingSection::WorkflowStates],
            SectionStatus::Stale { .. }
        ));
        assert!(matches!(
            model.statuses()[&OnboardingSection::Complexity],
            SectionStatus::Stale { .. }
        ));
        assert!(matches!(
            model.validate(),
            Err(OnboardingError::StaleSections)
        ));
        assert!(model.maker.value.is_some());
        assert!(model.reviewer.value.is_some());
    }

    #[test]
    fn provider_changes_stale_reviewer_but_effort_changes_do_not() {
        let mut model = model();
        let maker_provider = HarnessId::new("claude-code").unwrap();
        let outcome = model.set_maker_provider(maker_provider);
        assert!(outcome.invalidated.contains(&OnboardingSection::Maker));
        assert!(outcome.invalidated.contains(&OnboardingSection::Reviewer));
        assert!(matches!(
            model.statuses()[&OnboardingSection::Reviewer],
            SectionStatus::Stale { .. }
        ));

        let maker = model.maker.value.clone().unwrap();
        model.set_maker(HarnessSelection {
            effort: Effort::Low,
            ..maker
        });
        assert!(!model.statuses()[&OnboardingSection::Maker].is_blocking());
    }

    #[test]
    fn a_provider_change_cannot_commit_without_a_model_choice() {
        let mut model = model();
        model.set_maker_provider(HarnessId::new("new-provider").unwrap());
        assert!(matches!(
            model.statuses()[&OnboardingSection::Maker],
            SectionStatus::Stale { .. }
        ));
        model.confirm_section(OnboardingSection::Maker);
        assert!(model.statuses()[&OnboardingSection::Maker].is_blocking());
        assert!(matches!(
            model.validate(),
            Err(OnboardingError::HarnessSelectionMissing)
        ));
    }

    #[test]
    fn the_headless_editor_commits_a_complete_document() {
        let mut editor = HeadlessOnboardingEditor {
            events: vec![HeadlessEvent::Commit],
        };
        let result = editor
            .edit(model(), OnboardingDiscovery::default())
            .unwrap();
        assert!(matches!(result, OnboardingEditorResult::Committed(_)));
    }

    #[test]
    fn rerendering_preserves_values_outside_the_editor_model() {
        let mut existing = render_config(&answers(), &paths()).unwrap();
        existing = existing.replace(
            "installation_id: REPLACE_ME_GITHUB_INSTALLATION_ID",
            "installation_id: github-installation-kept",
        );
        let mut edited = model();
        edited.catalog_version = Some("catalog-test".to_owned());
        let rendered = render_model_config(&edited, &paths(), Some(&existing)).unwrap();
        let config = Config::from_yaml(&rendered).unwrap();
        assert_eq!(config.github.installation_id, "github-installation-kept");
        assert_eq!(
            config.harnesses.catalog_version.as_deref(),
            Some("catalog-test")
        );
    }

    fn catalog() -> ModelCatalog {
        ModelCatalog {
            version: "test".to_owned(),
            providers: BTreeMap::from([(
                "codex".to_owned(),
                vec![
                    CatalogModel {
                        id: "wide".to_owned(),
                        default_effort: Effort::Low,
                        efforts: vec![Effort::Low, Effort::Medium, Effort::High, Effort::Ultra],
                    },
                    CatalogModel {
                        id: "narrow".to_owned(),
                        default_effort: Effort::Medium,
                        efforts: vec![Effort::Low, Effort::Medium],
                    },
                ],
            )]),
        }
    }

    #[test]
    fn effort_cycles_only_through_the_levels_the_model_accepts() {
        let catalog = catalog();
        let codex = HarnessId::new("codex").unwrap();
        let narrow = ModelId::new("narrow").unwrap();
        assert_eq!(
            catalog.next_effort(&codex, &narrow, Effort::Medium),
            Effort::Low
        );
        // An effort the model does not accept falls back to its default rather
        // than advancing from a level that was never legal.
        assert_eq!(
            catalog.next_effort(&codex, &narrow, Effort::Ultra),
            Effort::Medium
        );
        // Off-catalog models declare no ceiling, so nothing is withheld.
        let unknown = ModelId::new("unknown").unwrap();
        assert_eq!(catalog.efforts_for(&codex, &unknown), Effort::ALL.to_vec());
    }

    #[test]
    fn choosing_a_narrower_model_resets_a_now_illegal_effort() {
        let catalog = catalog();
        let codex = HarnessId::new("codex").unwrap();
        let mut model = OnboardingModel::empty();
        model.maker.value = Some(HarnessSelection {
            provider: codex.clone(),
            model: ModelId::new("wide").unwrap(),
            effort: Effort::Ultra,
        });

        let outcome = model.set_model(
            OnboardingRole::Maker,
            ModelId::new("narrow").unwrap(),
            &catalog,
        );
        assert_eq!(outcome.invalidated, vec![OnboardingSection::Maker]);
        assert_eq!(
            model.maker.value.as_ref().unwrap().effort,
            Effort::Medium,
            "the effort falls back to the new model's own default"
        );

        // A model that still accepts the current effort leaves it alone.
        let outcome = model.set_model(
            OnboardingRole::Maker,
            ModelId::new("wide").unwrap(),
            &catalog,
        );
        assert!(outcome.invalidated.is_empty());
        assert_eq!(model.maker.value.as_ref().unwrap().effort, Effort::Medium);
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
