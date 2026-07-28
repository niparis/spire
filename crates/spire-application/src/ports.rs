use std::time::SystemTime;

use spire_domain::{
    CommitSha, LinearIssueId, ProviderCapacity, RepositoryName, RunId, WorkItemId, WorkspaceId,
};

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

pub trait LinearPort {
    type Error;

    fn get_issue(
        &self,
        issue_id: &LinearIssueId,
    ) -> Result<ExternalResult<CanonicalIssue>, Self::Error>;
    fn find_relevant_issues(
        &self,
        query: &RelevantIssueQuery,
    ) -> Result<ExternalResult<IssuePage>, Self::Error>;
    fn transition_issue(
        &self,
        issue_id: &LinearIssueId,
        expected_version: &ExpectedVersion,
        target_state_id: &str,
        idempotency_key: &IdempotencyKey,
    ) -> Result<ExternalResult<()>, Self::Error>;
    fn post_comment(
        &self,
        issue_id: &LinearIssueId,
        idempotency_key: &IdempotencyKey,
        body: &str,
    ) -> Result<ExternalResult<()>, Self::Error>;
    fn get_workflow_configuration(
        &self,
        team_id: &str,
    ) -> Result<ExternalResult<WorkflowConfiguration>, Self::Error>;
}

pub trait GitHubPort {
    type Error;

    fn required_checks(
        &self,
        repository: &RepositoryName,
        head_sha: &CommitSha,
    ) -> Result<ExternalResult<RequiredChecks>, Self::Error>;
    fn get_pull_request(
        &self,
        repository: &RepositoryName,
        number: u64,
    ) -> Result<ExternalResult<PullRequest>, Self::Error>;
    fn find_pull_request_by_branch(
        &self,
        repository: &RepositoryName,
        branch: &str,
    ) -> Result<ExternalResult<PullRequest>, Self::Error>;
    fn merge_state(
        &self,
        repository: &RepositoryName,
        number: u64,
    ) -> Result<ExternalResult<MergeState>, Self::Error>;
    fn post_review_summary(
        &self,
        repository: &RepositoryName,
        number: u64,
        idempotency_key: &IdempotencyKey,
        body: &str,
    ) -> Result<ExternalResult<()>, Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequiredChecks {
    pub head_sha: CommitSha,
    pub all_successful: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    pub number: u64,
    pub head_sha: CommitSha,
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

pub trait WorkspacePort {
    type Error;

    fn allocate(
        &self,
        work_item_id: WorkItemId,
        run_id: RunId,
    ) -> Result<ExternalResult<Workspace>, Self::Error>;
    fn cleanup(
        &self,
        workspace_id: WorkspaceId,
        idempotency_key: &IdempotencyKey,
    ) -> Result<ExternalResult<()>, Self::Error>;
    fn inspect(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<ExternalResult<WorkspaceStatus>, Self::Error>;
    fn quarantine(
        &self,
        workspace_id: WorkspaceId,
        reason: &str,
        idempotency_key: &IdempotencyKey,
    ) -> Result<ExternalResult<()>, Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub repository: RepositoryName,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceStatus {
    Allocated,
    Missing,
    Quarantined,
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
