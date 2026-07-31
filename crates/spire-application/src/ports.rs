use std::time::SystemTime;

use crate::{CanonicalLinearIssue, CanonicalPullRequest, CheckRun};
use serde::{Deserialize, Serialize};
use spire_domain::{CommitSha, LinearIssueId, ProviderCapacity, RepositoryName, RunId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternalResult<T> {
    Confirmed(T),
    NotFound,
    Ambiguous { detail: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedVersion(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdempotencyKey(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalIssue {
    pub id: LinearIssueId,
    pub revision: ExpectedVersion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelevantIssueQuery {
    pub team_id: String,
    pub cursor: Option<String>,
    pub workflow_state_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuePage {
    pub issues: Vec<CanonicalIssue>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowConfiguration {
    pub team_id: String,
    pub workflow_state_ids: Vec<String>,
}

/// Read-only Linear boundary used by ingestion and reconciliation.
#[allow(async_fn_in_trait)]
pub trait LinearReadPort {
    type Error;

    async fn get_canonical_issue(
        &self,
        issue_id: &LinearIssueId,
    ) -> Result<ExternalResult<CanonicalLinearIssue>, Self::Error>;

    async fn find_canonical_issues(
        &self,
        query: &RelevantIssueQuery,
    ) -> Result<ExternalResult<CanonicalIssuePage>, Self::Error>;
}

/// Linear's own classification of a workflow state. It narrows a suggestion but
/// never establishes a semantic mapping on its own: several Linear states share
/// one category, and only the operator can say which one Spire should treat as
/// ready, blocked, or specs-needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinearStateCategory {
    Triage,
    Backlog,
    Unstarted,
    Started,
    Completed,
    Canceled,
    Unrecognized,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LinearTeamSummary {
    pub id: String,
    pub key: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LinearWorkflowState {
    pub id: String,
    pub name: String,
    pub category: LinearStateCategory,
}

/// A team's estimate scale, already normalized to the point values Spire may
/// receive on an issue. `points` is empty when the team does not estimate, which
/// makes the team ineligible: Spire cannot classify complexity without it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LinearEstimateScale {
    pub kind: String,
    pub points: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LinearTeamConfiguration {
    pub team: LinearTeamSummary,
    pub states: Vec<LinearWorkflowState>,
    pub estimates: LinearEstimateScale,
}

/// Read-only Linear boundary used by first-run onboarding. It exists separately
/// from `LinearReadPort` because onboarding runs before a configuration exists,
/// so it cannot depend on a resolved team or workflow contract.
#[allow(async_fn_in_trait)]
pub trait LinearOnboardingDiscoveryPort {
    type Error;

    /// Every team the authenticated viewer can see, across all pages.
    async fn list_teams(&self) -> Result<Vec<LinearTeamSummary>, Self::Error>;

    async fn team_configuration(
        &self,
        team_id: &str,
    ) -> Result<ExternalResult<LinearTeamConfiguration>, Self::Error>;
}

/// Mutating Linear boundary. It is a separate port from `LinearReadPort` so a
/// build, a command, or a test can hold read authority without write authority.
///
/// Every operation is conditional and idempotent: transitions carry the state
/// the caller expects to overwrite, and comments carry a stable idempotency key.
#[allow(async_fn_in_trait)]
pub trait LinearWritePort {
    type Error;

    /// Applies a workflow transition only when Linear still reports
    /// `expected_state_id`. Returns `Ambiguous` when a human moved the ticket.
    async fn transition_issue(
        &self,
        issue_id: &LinearIssueId,
        expected_state_id: &str,
        target_state_id: &str,
    ) -> Result<ExternalResult<TransitionApplied>, Self::Error>;

    /// Publishes a comment at most once. The marker is searched for first, so a
    /// replayed outbox action re-uses the existing comment.
    async fn publish_comment(
        &self,
        issue_id: &LinearIssueId,
        idempotency_key: &IdempotencyKey,
        marker: &str,
        body: &str,
    ) -> Result<ExternalResult<PublishedComment>, Self::Error>;

    /// Reports whether the configured webhook still exists and is enabled.
    async fn webhook_configuration(
        &self,
        webhook_id: &str,
    ) -> Result<ExternalResult<WebhookConfigurationStatus>, Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionApplied {
    pub applied: bool,
    pub observed_state_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedComment {
    pub external_id: String,
    pub already_present: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebhookConfigurationStatus {
    pub webhook_id: String,
    pub enabled: bool,
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalIssuePage {
    pub issues: Vec<CanonicalLinearIssue>,
    pub next_cursor: Option<String>,
}

#[allow(async_fn_in_trait)]
pub trait GitHubPort {
    type Error;

    async fn required_checks(
        &self,
        repository: &RepositoryName,
        head_sha: &CommitSha,
    ) -> Result<ExternalResult<Vec<CheckRun>>, Self::Error>;
    async fn get_pull_request(
        &self,
        repository: &RepositoryName,
        number: u64,
    ) -> Result<ExternalResult<CanonicalPullRequest>, Self::Error>;
    async fn find_pull_request_by_branch(
        &self,
        repository: &RepositoryName,
        branch: &str,
    ) -> Result<ExternalResult<CanonicalPullRequest>, Self::Error>;
    async fn merge_state(
        &self,
        repository: &RepositoryName,
        number: u64,
    ) -> Result<ExternalResult<MergeState>, Self::Error>;
    async fn post_review_summary(
        &self,
        repository: &RepositoryName,
        number: u64,
        idempotency_key: &IdempotencyKey,
        body: &str,
    ) -> Result<ExternalResult<PublishedComment>, Self::Error>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeState {
    Open,
    Closed,
    Merged,
}

pub trait HarnessRunnerPort {
    type Error;

    fn start(&self, run_id: RunId) -> Result<ExternalResult<ExternalRun>, Self::Error>;
    fn probe_capacity(&self) -> Result<ExternalResult<ProviderCapacity>, Self::Error>;
    fn inspect(&self, external_run_id: &str) -> Result<ExternalResult<RunInspection>, Self::Error>;
    fn resume(
        &self,
        run_id: RunId,
        prior_external_run_id: Option<&str>,
    ) -> Result<ExternalResult<ExternalRun>, Self::Error>;
    fn cancel(
        &self,
        external_run_id: &str,
        idempotency_key: &IdempotencyKey,
    ) -> Result<ExternalResult<()>, Self::Error>;
    fn collect_result(
        &self,
        external_run_id: &str,
    ) -> Result<ExternalResult<NormalizedResult>, Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalRun {
    pub external_run_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunInspection {
    pub terminal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedResult {
    pub outcome: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceKind {
    Maker,
    Reviewer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceAllocationState {
    Allocating,
    Ready,
    Quarantined,
    Removing,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MakerWorkspaceRequest {
    pub workspace_id: String,
    pub work_item_id: String,
    pub linear_identifier: String,
    pub root_run_id: String,
    pub repository_source_path: String,
    pub git_common_directory: String,
    pub base_sha: String,
    pub workspace_root: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewerWorkspaceRequest {
    pub workspace_id: String,
    pub work_item_id: String,
    pub run_id: String,
    pub review_cycle_id: String,
    pub repository_source_path: String,
    pub git_common_directory: String,
    pub base_sha: String,
    pub head_sha: String,
    pub workspace_root: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceRecord {
    pub id: String,
    pub work_item_id: String,
    pub run_id: Option<String>,
    pub kind: WorkspaceKind,
    pub root_run_id: Option<String>,
    pub review_cycle_id: Option<String>,
    pub path: String,
    pub workspace_root: String,
    pub repository_source_path: String,
    pub git_common_directory: String,
    pub base_sha: String,
    pub head_sha: Option<String>,
    pub branch: Option<String>,
    pub allocation_state: WorkspaceAllocationState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct WorkspaceRecoverySummary {
    pub adopted: u64,
    pub quarantined: u64,
}

#[allow(async_fn_in_trait)]
pub trait WorkspacePort {
    type Error;

    async fn allocate_maker(
        &self,
        request: MakerWorkspaceRequest,
    ) -> Result<ExternalResult<WorkspaceRecord>, Self::Error>;
    async fn allocate_reviewer(
        &self,
        request: ReviewerWorkspaceRequest,
    ) -> Result<ExternalResult<WorkspaceRecord>, Self::Error>;
    async fn verify_reviewer_clean(
        &self,
        workspace_id: &str,
    ) -> Result<ExternalResult<bool>, Self::Error>;
    async fn recover_allocations(&self) -> Result<WorkspaceRecoverySummary, Self::Error>;
    async fn cleanup(&self, workspace_id: &str) -> Result<ExternalResult<()>, Self::Error>;
}

pub trait ClockPort {
    fn now(&self) -> SystemTime;
}

pub trait NotifierPort {
    type Error;

    fn notify(&self, notification: Notification) -> Result<ExternalResult<()>, Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub idempotency_key: IdempotencyKey,
    pub subject: String,
    pub body: String,
}

pub trait UnitOfWork {
    type Error;

    fn transaction<T>(
        &self,
        operation: impl FnOnce() -> Result<T, Self::Error>,
    ) -> Result<T, Self::Error>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixedClock(SystemTime);

    impl ClockPort for FixedClock {
        fn now(&self) -> SystemTime {
            self.0
        }
    }

    #[test]
    fn in_memory_clock_is_a_port_implementation() {
        let instant = SystemTime::UNIX_EPOCH;
        assert_eq!(FixedClock(instant).now(), instant);
    }
}
