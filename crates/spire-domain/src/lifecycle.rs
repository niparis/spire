use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    CandidateEvaluation, CommitSha, DispatchPolicyVersion, DispatchRuleId, HarnessId,
    RepositoryName, RunId, RunRole, WorkItemId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkItemState {
    Observed,
    Eligible,
    Claiming,
    Queued,
    Implementing,
    WaitingForCi,
    WaitingForReview,
    HumanReady,
    WaitingForProvider,
    Blocked,
    Completed,
    Canceled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Starting,
    Running,
    Succeeded,
    Failed,
    CapacityRejected,
    CapacityExhausted,
    CancelRequested,
    Canceled,
    TimedOut,
    Lost,
}

impl RunStatus {
    pub fn is_active(self) -> bool {
        matches!(
            self,
            Self::Queued | Self::Starting | Self::Running | Self::CancelRequested
        )
    }

    pub fn is_terminal(self) -> bool {
        !self.is_active()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub id: RunId,
    pub work_item_id: WorkItemId,
    pub repository: RepositoryName,
    pub role: RunRole,
    pub status: RunStatus,
    pub correction_cycle: u8,
}

impl Run {
    pub fn transition(&mut self, next: RunStatus) -> Result<(), LifecycleError> {
        let valid = matches!(
            (self.status, next),
            (RunStatus::Queued, RunStatus::Starting)
                | (
                    RunStatus::Starting,
                    RunStatus::Running | RunStatus::Failed | RunStatus::CapacityRejected
                )
                | (
                    RunStatus::Running,
                    RunStatus::Succeeded
                        | RunStatus::Failed
                        | RunStatus::CapacityExhausted
                        | RunStatus::CancelRequested
                        | RunStatus::TimedOut
                        | RunStatus::Lost
                )
                | (
                    RunStatus::CancelRequested,
                    RunStatus::Canceled | RunStatus::Lost
                )
        );
        if !valid {
            return Err(LifecycleError::InvalidRunTransition {
                from: self.status,
                to: next,
            });
        }
        self.status = next;
        Ok(())
    }

    pub fn consumes_correction_cycle(&self) -> bool {
        matches!(self.status, RunStatus::Failed | RunStatus::TimedOut)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkItem {
    pub id: WorkItemId,
    pub repository: RepositoryName,
    pub state: WorkItemState,
    pub active_run_id: Option<RunId>,
    pub sticky_maker: Option<HarnessId>,
}

impl WorkItem {
    pub fn attach_active_run(&mut self, run: &Run) -> Result<(), LifecycleError> {
        if self.active_run_id.is_some() {
            return Err(LifecycleError::ActiveRunAlreadyExists);
        }
        if self.id != run.work_item_id || self.repository != run.repository {
            return Err(LifecycleError::RunDoesNotBelongToWorkItem);
        }
        self.active_run_id = Some(run.id);
        Ok(())
    }

    pub fn clear_terminal_run(&mut self, run: &Run) -> Result<(), LifecycleError> {
        if self.active_run_id != Some(run.id) || !run.status.is_terminal() {
            return Err(LifecycleError::CannotClearActiveRun);
        }
        self.active_run_id = None;
        Ok(())
    }

    pub fn mark_maker_started(&mut self, harness: HarnessId) -> Result<(), LifecycleError> {
        if self.sticky_maker.is_some() {
            return Err(LifecycleError::MakerAlreadySticky);
        }
        self.sticky_maker = Some(harness);
        Ok(())
    }

    pub fn reopen(&mut self) -> Result<(), LifecycleError> {
        if !matches!(
            self.state,
            WorkItemState::Completed | WorkItemState::Canceled | WorkItemState::Blocked
        ) {
            return Err(LifecycleError::ReopenRequiresTerminalState);
        }
        self.state = WorkItemState::Eligible;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lease {
    pub run_id: RunId,
    pub owner: String,
    pub expires_at_unix_seconds: u64,
}

impl Lease {
    pub fn is_expired(&self, now_unix_seconds: u64) -> bool {
        now_unix_seconds >= self.expires_at_unix_seconds
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DispatchDecision {
    pub policy_version: DispatchPolicyVersion,
    pub rule_id: DispatchRuleId,
    pub candidates: Vec<CandidateEvaluation>,
}

#[derive(Debug, Default)]
pub struct ActiveRunRegistry {
    mutating_by_repository: BTreeMap<RepositoryName, RunId>,
    active_by_work_item: BTreeMap<WorkItemId, RunId>,
}

impl ActiveRunRegistry {
    pub fn reserve(&mut self, run: &Run) -> Result<(), LifecycleError> {
        if self.active_by_work_item.contains_key(&run.work_item_id) {
            return Err(LifecycleError::ActiveRunAlreadyExists);
        }
        if run.role == RunRole::Implementation
            && self.mutating_by_repository.contains_key(&run.repository)
        {
            return Err(LifecycleError::MutatingRunAlreadyExistsForRepository);
        }
        self.active_by_work_item.insert(run.work_item_id, run.id);
        if run.role == RunRole::Implementation {
            self.mutating_by_repository
                .insert(run.repository.clone(), run.id);
        }
        Ok(())
    }

    pub fn release(&mut self, run: &Run) -> Result<(), LifecycleError> {
        if !run.status.is_terminal()
            || self.active_by_work_item.get(&run.work_item_id) != Some(&run.id)
        {
            return Err(LifecycleError::CannotReleaseActiveRun);
        }
        self.active_by_work_item.remove(&run.work_item_id);
        if run.role == RunRole::Implementation {
            self.mutating_by_repository.remove(&run.repository);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewCycle {
    pub work_item_id: WorkItemId,
    pub head_sha: CommitSha,
    pub ci_green: bool,
    pub approved: bool,
}

impl ReviewCycle {
    pub fn approve(&mut self, reviewed_sha: &CommitSha) -> Result<(), LifecycleError> {
        if !self.ci_green {
            return Err(LifecycleError::ReviewRequiresGreenCi);
        }
        if &self.head_sha != reviewed_sha {
            return Err(LifecycleError::ReviewShaIsStale);
        }
        self.approved = true;
        Ok(())
    }

    pub fn update_head(&mut self, next: CommitSha) {
        self.head_sha = next;
        self.ci_green = false;
        self.approved = false;
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LifecycleError {
    #[error("invalid run transition from {from:?} to {to:?}")]
    InvalidRunTransition { from: RunStatus, to: RunStatus },
    #[error("work item already has an active run")]
    ActiveRunAlreadyExists,
    #[error("run does not belong to this work item")]
    RunDoesNotBelongToWorkItem,
    #[error("only the active terminal run can be cleared")]
    CannotClearActiveRun,
    #[error("maker is already sticky")]
    MakerAlreadySticky,
    #[error("only a terminal work item can be reopened")]
    ReopenRequiresTerminalState,
    #[error("a mutating run already exists for this repository")]
    MutatingRunAlreadyExistsForRepository,
    #[error("only a reserved terminal run can be released")]
    CannotReleaseActiveRun,
    #[error("review requires successful CI for the exact head SHA")]
    ReviewRequiresGreenCi,
    #[error("review result is for a stale SHA")]
    ReviewShaIsStale,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository() -> RepositoryName {
        RepositoryName::new("owner/repository").unwrap()
    }

    fn run(status: RunStatus) -> Run {
        Run {
            id: RunId::new(),
            work_item_id: WorkItemId::new(),
            repository: repository(),
            role: RunRole::Implementation,
            status,
            correction_cycle: 0,
        }
    }

    #[test]
    fn state_transitions_reject_terminal_restarts() {
        let mut run = run(RunStatus::Queued);
        run.transition(RunStatus::Starting).unwrap();
        run.transition(RunStatus::Running).unwrap();
        run.transition(RunStatus::CapacityExhausted).unwrap();
        assert!(!run.consumes_correction_cycle());
        assert!(run.transition(RunStatus::Running).is_err());
    }

    #[test]
    fn only_a_successfully_started_maker_becomes_sticky() {
        let work_item_id = WorkItemId::new();
        let mut item = WorkItem {
            id: work_item_id,
            repository: repository(),
            state: WorkItemState::Implementing,
            active_run_id: None,
            sticky_maker: None,
        };
        let mut run = run(RunStatus::Starting);
        run.work_item_id = work_item_id;
        item.attach_active_run(&run).unwrap();
        item.mark_maker_started(HarnessId::new("codex").unwrap())
            .unwrap();
        assert!(
            item.mark_maker_started(HarnessId::new("claude-code").unwrap())
                .is_err()
        );
    }

    #[test]
    fn approval_is_invalidated_by_a_new_head() {
        let sha = CommitSha::new("abc123").unwrap();
        let mut cycle = ReviewCycle {
            work_item_id: WorkItemId::new(),
            head_sha: sha.clone(),
            ci_green: true,
            approved: false,
        };
        cycle.approve(&sha).unwrap();
        cycle.update_head(CommitSha::new("def456").unwrap());
        assert!(!cycle.approved);
        assert!(!cycle.ci_green);
    }

    #[test]
    fn admission_prevents_concurrent_mutating_runs_per_repository() {
        let mut registry = ActiveRunRegistry::default();
        let first = run(RunStatus::Queued);
        let second = Run {
            id: RunId::new(),
            work_item_id: WorkItemId::new(),
            repository: first.repository.clone(),
            role: RunRole::Implementation,
            status: RunStatus::Queued,
            correction_cycle: 0,
        };
        registry.reserve(&first).unwrap();
        assert_eq!(
            registry.reserve(&second),
            Err(LifecycleError::MutatingRunAlreadyExistsForRepository)
        );
    }
}
