//! The single lifecycle use case behind both webhooks and reconciliation.
//!
//! A webhook only tells the orchestrator to look; the decision is always taken
//! from canonical Linear state plus local state. Because the trigger is not an
//! input to the decision, a delayed, duplicated, or reordered delivery converges
//! on the same plan a reconciliation pass would produce.

use serde::Serialize;
use spire_domain::{
    ComplexityClass, LinearIssueId, ProjectMappingRevision, ProjectRepositoryMappingId,
    RepositoryName, WorkItemState,
};

use crate::{
    AdmissionCandidate, CanonicalLinearIssue, EligibilityReason, EligibilityResult,
    LinearStateKind, PlannedAction, RolloutDecision, RolloutGate, RolloutRefusal, evaluate_rollout,
    operator_notification,
};

/// Why the orchestrator is looking at this ticket. It is recorded for audit and
/// never consulted while deciding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "trigger")]
pub enum IngestionTrigger {
    Webhook {
        delivery_id: String,
        actor_id: Option<String>,
    },
    Reconciliation,
}

impl IngestionTrigger {
    pub fn trigger_kind(&self) -> &'static str {
        match self {
            Self::Webhook { .. } => "linear_webhook",
            Self::Reconciliation => "reconciliation_recovery",
        }
    }

    /// Actor filtering is noise reduction for logs only; correctness never
    /// depends on recognizing the orchestrator's own writes.
    pub fn is_self_generated(&self, bot_actor_id: &str) -> bool {
        matches!(self, Self::Webhook { actor_id: Some(actor), .. } if actor == bot_actor_id)
    }
}

/// Local snapshot the plan is compared against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LocalWorkItem {
    pub work_item_id: String,
    pub state: WorkItemState,
    pub revision: String,
    pub active_run_id: Option<String>,
}

impl LocalWorkItem {
    /// States in which the orchestrator, not an inbound event, owns the ticket.
    pub fn is_locally_owned(&self) -> bool {
        matches!(
            self.state,
            WorkItemState::Claiming
                | WorkItemState::Queued
                | WorkItemState::Implementing
                | WorkItemState::WaitingForCi
                | WorkItemState::WaitingForReview
        )
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            WorkItemState::Completed | WorkItemState::Canceled
        )
    }
}

/// The canonical facts that must be persisted for the work item, whatever the
/// lifecycle decision is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ObservationUpsert {
    pub work_item_id: String,
    pub linear_issue_id: LinearIssueId,
    pub linear_identifier: String,
    pub team_id: String,
    pub workflow_state_id: String,
    pub linear_project_id: Option<spire_domain::LinearProjectId>,
    pub linear_project_name_snapshot: Option<String>,
    pub revision: String,
    pub raw_estimate: Option<u8>,
    pub complexity_class: Option<ComplexityClass>,
    pub eligibility_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "decision")]
pub enum LifecycleDecision {
    /// Eligible and inside every rollout gate: the scheduler may claim it.
    Admit {
        repository: RepositoryName,
        mapping_id: ProjectRepositoryMappingId,
        mapping_revision: ProjectMappingRevision,
        complexity: ComplexityClass,
    },
    HoldIneligible {
        reason: EligibilityReason,
    },
    HoldWaitingForDependency {
        blockers: Vec<LinearIssueId>,
    },
    HoldRolloutRefused {
        reason: RolloutRefusal,
    },
    /// The orchestrator already owns this ticket; the event only confirms state.
    RespectLocalOwnership,
    RespectLocalTerminal,
    /// Linear reached a terminal status while a run is still live.
    ConflictTerminalWithLiveRun,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IngestionPlan {
    pub observation: ObservationUpsert,
    pub decision: LifecycleDecision,
    pub actions: Vec<PlannedAction>,
    pub operator_detail: String,
}

#[derive(Debug, Clone, Copy)]
pub struct IngestionInput<'a> {
    pub issue: &'a CanonicalLinearIssue,
    pub local: Option<&'a LocalWorkItem>,
    pub eligibility: &'a EligibilityResult,
    pub rollout: &'a RolloutGate,
    pub active_harness_runs: u16,
    /// Lifecycle kind the canonical workflow state maps to, when it is one the
    /// orchestrator projects.
    pub canonical_state_kind: Option<LinearStateKind>,
}

/// Stable work-item identity derived from the Linear issue.
pub fn work_item_id(issue_id: &LinearIssueId) -> String {
    format!("linear:{issue_id}")
}

/// Plans the lifecycle response to canonical Linear state. Pure and total.
pub fn plan_ingestion(input: IngestionInput<'_>) -> IngestionPlan {
    let issue = input.issue;
    let work_item_id = work_item_id(&issue.id);
    let (complexity, reason) = match input.eligibility {
        EligibilityResult::Eligible { complexity, .. } => (Some(*complexity), None),
        EligibilityResult::Ineligible { reason, .. } => (None, Some(format!("{reason:?}"))),
        EligibilityResult::WaitingForDependency { .. } => {
            (None, Some("waiting_for_dependency".to_owned()))
        }
    };
    let observation = ObservationUpsert {
        work_item_id: work_item_id.clone(),
        linear_issue_id: issue.id.clone(),
        linear_identifier: issue.identifier.clone(),
        team_id: issue.team_id.clone(),
        workflow_state_id: issue.workflow_state_id.clone(),
        linear_project_id: issue.project_id.clone(),
        linear_project_name_snapshot: issue.project_name_snapshot.clone(),
        revision: issue.revision.clone(),
        raw_estimate: issue.estimate,
        complexity_class: complexity,
        eligibility_reason: reason,
    };

    if let Some(local) = input.local {
        let linear_is_terminal = matches!(
            input.canonical_state_kind,
            Some(LinearStateKind::Done | LinearStateKind::Canceled)
        );
        if local.is_locally_owned() {
            return if linear_is_terminal {
                let detail = format!(
                    "Linear reports a terminal status while run {} is still live; cancellation is required",
                    local.active_run_id.as_deref().unwrap_or("unknown")
                );
                plan(
                    observation,
                    LifecycleDecision::ConflictTerminalWithLiveRun,
                    vec![operator_notification(
                        &work_item_id,
                        format!("notify:terminal-conflict:{work_item_id}"),
                        "warning",
                        "Linear ticket became terminal while a run is live",
                        &detail,
                    )],
                    detail,
                )
            } else {
                plan(
                    observation,
                    LifecycleDecision::RespectLocalOwnership,
                    Vec::new(),
                    format!(
                        "the orchestrator already owns this ticket in state {:?}",
                        local.state
                    ),
                )
            };
        }
        if local.is_terminal() {
            return plan(
                observation,
                LifecycleDecision::RespectLocalTerminal,
                Vec::new(),
                "the work item is locally terminal and is not reopened by an event".to_owned(),
            );
        }
    }

    match input.eligibility {
        EligibilityResult::WaitingForDependency { blockers } => plan(
            observation,
            LifecycleDecision::HoldWaitingForDependency {
                blockers: blockers.clone(),
            },
            Vec::new(),
            "the ticket has incomplete blocking dependencies".to_owned(),
        ),
        EligibilityResult::Ineligible {
            reason,
            operator_detail,
        } => plan(
            observation,
            LifecycleDecision::HoldIneligible { reason: *reason },
            Vec::new(),
            operator_detail.clone(),
        ),
        EligibilityResult::Eligible {
            repository,
            mapping_id,
            mapping_revision,
            complexity,
        } => {
            let candidate = AdmissionCandidate {
                team_id: &issue.team_id,
                repository: repository.as_str(),
                labels: &issue.labels,
                active_harness_runs: input.active_harness_runs,
            };
            match evaluate_rollout(input.rollout, candidate) {
                RolloutDecision::Admit => plan(
                    observation,
                    LifecycleDecision::Admit {
                        repository: repository.clone(),
                        mapping_id: *mapping_id,
                        mapping_revision: *mapping_revision,
                        complexity: *complexity,
                    },
                    Vec::new(),
                    "the ticket is eligible and inside every rollout gate".to_owned(),
                ),
                RolloutDecision::Refuse {
                    reason,
                    operator_detail,
                } => plan(
                    observation,
                    LifecycleDecision::HoldRolloutRefused { reason },
                    Vec::new(),
                    operator_detail,
                ),
            }
        }
    }
}

fn plan(
    observation: ObservationUpsert,
    decision: LifecycleDecision,
    actions: Vec<PlannedAction>,
    operator_detail: String,
) -> IngestionPlan {
    IngestionPlan {
        observation,
        decision,
        actions,
        operator_detail,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;
    use crate::{EligibilityInput, ProjectRoutingDecision, evaluate_eligibility};
    use spire_domain::{ProjectMappingRevision, ProjectRepositoryMappingId};

    fn issue(state: &str, revision: &str) -> CanonicalLinearIssue {
        CanonicalLinearIssue {
            id: LinearIssueId::new("issue-1").unwrap(),
            identifier: "SPI-1".into(),
            team_id: "team".into(),
            workflow_state_id: state.into(),
            project_id: Some(spire_domain::LinearProjectId::new("project").unwrap()),
            project_name_snapshot: Some("Project".into()),
            estimate: Some(2),
            priority: Some(1),
            labels: ["type:chore".into(), "repo:spire".into()]
                .into_iter()
                .collect(),
            blockers: vec![],
            description: Some("acceptance criteria: it works".into()),
            acceptance_criteria: Some("it works".into()),
            assignee_id: None,
            creator_id: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-02T00:00:00Z".into(),
            revision: revision.into(),
        }
    }

    fn eligibility(issue: &CanonicalLinearIssue) -> EligibilityResult {
        let types = ["type:chore".to_owned()].into_iter().collect();
        let routing = ProjectRoutingDecision::Mapped {
            mapping_id: ProjectRepositoryMappingId::new(),
            mapping_revision: ProjectMappingRevision::new(1).unwrap(),
            repository: RepositoryName::new("owner/spire").unwrap(),
        };
        let complexity = BTreeMap::from([(2, ComplexityClass::Medium)]);
        evaluate_eligibility(EligibilityInput {
            issue,
            ready_state_id: "ready",
            supported_type_labels: &types,
            project_routing: &routing,
            complexity_mapping: &complexity,
            incomplete_blockers: &BTreeSet::new(),
            locally_active: false,
            locally_terminal: false,
            dispatch_covers_implementation_and_review: true,
        })
    }

    fn gate() -> RolloutGate {
        RolloutGate {
            linear_writes_enabled: true,
            kill_switch_engaged: false,
            allowed_team_ids: ["team".to_owned()].into_iter().collect(),
            allowed_repositories: ["owner/spire".to_owned()].into_iter().collect(),
            allowed_type_labels: ["type:chore".to_owned()].into_iter().collect(),
            max_active_harness_runs: 1,
        }
    }

    fn plan_for<'a>(
        issue: &'a CanonicalLinearIssue,
        eligibility: &'a EligibilityResult,
        gate: &'a RolloutGate,
        local: Option<&'a LocalWorkItem>,
        canonical_state_kind: Option<LinearStateKind>,
    ) -> IngestionPlan {
        plan_ingestion(IngestionInput {
            issue,
            local,
            eligibility,
            rollout: gate,
            active_harness_runs: 0,
            canonical_state_kind,
        })
    }

    #[test]
    fn webhook_and_reconciliation_produce_the_same_plan() {
        let issue = issue("ready", "revision-1");
        let eligibility = eligibility(&issue);
        let gate = gate();
        let webhook = IngestionTrigger::Webhook {
            delivery_id: "delivery-1".into(),
            actor_id: Some("human".into()),
        };
        let from_webhook = plan_for(
            &issue,
            &eligibility,
            &gate,
            None,
            Some(LinearStateKind::Ready),
        );
        let from_reconciliation = plan_for(
            &issue,
            &eligibility,
            &gate,
            None,
            Some(LinearStateKind::Ready),
        );
        assert_eq!(from_webhook, from_reconciliation);
        assert!(matches!(
            from_webhook.decision,
            LifecycleDecision::Admit { .. }
        ));
        assert_eq!(webhook.trigger_kind(), "linear_webhook");
        assert_eq!(
            IngestionTrigger::Reconciliation.trigger_kind(),
            "reconciliation_recovery"
        );
    }

    #[test]
    fn duplicate_delayed_and_reordered_deliveries_converge_on_canonical_state() {
        let gate = gate();
        let stale = issue("ready", "revision-1");
        let current = issue("progress", "revision-2");
        let stale_eligibility = eligibility(&stale);
        let current_eligibility = eligibility(&current);
        let local = LocalWorkItem {
            work_item_id: "linear:issue-1".into(),
            state: WorkItemState::Implementing,
            revision: "revision-2".into(),
            active_run_id: Some("run-1".into()),
        };
        // A delayed Ready delivery arriving after the claim cannot restart work.
        let delayed = plan_for(
            &stale,
            &stale_eligibility,
            &gate,
            Some(&local),
            Some(LinearStateKind::Ready),
        );
        assert_eq!(delayed.decision, LifecycleDecision::RespectLocalOwnership);
        assert!(delayed.actions.is_empty());
        // The orchestrator's own In Progress event is harmless and idempotent.
        let self_generated = plan_for(
            &current,
            &current_eligibility,
            &gate,
            Some(&local),
            Some(LinearStateKind::InProgress),
        );
        assert_eq!(
            self_generated.decision,
            LifecycleDecision::RespectLocalOwnership
        );
        assert!(self_generated.actions.is_empty());
        assert_eq!(
            plan_for(
                &current,
                &current_eligibility,
                &gate,
                Some(&local),
                Some(LinearStateKind::InProgress)
            ),
            self_generated
        );
        let self_event = IngestionTrigger::Webhook {
            delivery_id: "delivery-2".into(),
            actor_id: Some("bot".into()),
        };
        assert!(self_event.is_self_generated("bot"));
    }

    #[test]
    fn a_terminal_linear_status_over_a_live_run_raises_one_operator_action() {
        let issue = issue("canceled", "revision-3");
        let eligibility = eligibility(&issue);
        let local = LocalWorkItem {
            work_item_id: "linear:issue-1".into(),
            state: WorkItemState::Implementing,
            revision: "revision-3".into(),
            active_run_id: Some("run-1".into()),
        };
        let plan = plan_for(
            &issue,
            &eligibility,
            &gate(),
            Some(&local),
            Some(LinearStateKind::Canceled),
        );
        assert_eq!(
            plan.decision,
            LifecycleDecision::ConflictTerminalWithLiveRun
        );
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(
            plan.actions[0].idempotency_key,
            "notify:terminal-conflict:linear:issue-1"
        );
    }

    #[test]
    fn tickets_outside_the_rollout_remain_untouched_and_explain_why() {
        let issue = issue("ready", "revision-1");
        let eligibility = eligibility(&issue);
        let gate = RolloutGate {
            allowed_repositories: ["owner/other".to_owned()].into_iter().collect(),
            ..gate()
        };
        let plan = plan_for(
            &issue,
            &eligibility,
            &gate,
            None,
            Some(LinearStateKind::Ready),
        );
        assert_eq!(
            plan.decision,
            LifecycleDecision::HoldRolloutRefused {
                reason: RolloutRefusal::RepositoryNotAllowlisted
            }
        );
        assert!(plan.actions.is_empty());
        assert!(plan.operator_detail.contains("owner/spire"));
        // The canonical observation is still recorded, so reconciliation can see it.
        assert_eq!(plan.observation.workflow_state_id, "ready");
        assert_eq!(
            plan.observation.complexity_class,
            Some(ComplexityClass::Medium)
        );
    }
}
