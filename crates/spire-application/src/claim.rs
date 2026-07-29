//! Root-claim projection.
//!
//! The claim is durable locally first, then made visible in Linear, and only a
//! confirmed claim may start a harness. A Linear outage therefore reserves the
//! ticket without ever starting invisible work.

use serde::Serialize;
use spire_domain::WorkItemState;

use crate::{
    LinearCommentPayload, LinearTransitionPayload, PlannedAction, comment_marker, linear_comment,
    linear_transition,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClaimPlan {
    pub transition_key: String,
    pub comment_key: String,
    pub actions: Vec<PlannedAction>,
}

/// Builds the two effects a claim must publish: the Ready → In Progress
/// transition and the claim comment carrying the Run ID.
pub fn claim_plan(
    run_id: &str,
    issue_id: &str,
    linear_identifier: &str,
    expected_state_id: &str,
    in_progress_state_id: &str,
) -> ClaimPlan {
    let transition_key = format!("transition:{run_id}:{in_progress_state_id}");
    let comment_key = format!("claim:{run_id}");
    let body = format!(
        "**Spire run `{run_id}`** — claimed `{linear_identifier}`\n\n\
         This ticket is now owned by the orchestrator and moved to In Progress. \
         Move it out of In Progress to hand ownership back to a human.\n\n{}\n",
        comment_marker(&comment_key)
    );
    ClaimPlan {
        actions: vec![
            linear_transition(
                run_id,
                transition_key.clone(),
                LinearTransitionPayload {
                    issue_id: issue_id.to_owned(),
                    expected_state_id: expected_state_id.to_owned(),
                    target_state_id: in_progress_state_id.to_owned(),
                    run_id: Some(run_id.to_owned()),
                },
            ),
            linear_comment(
                run_id,
                comment_key.clone(),
                LinearCommentPayload {
                    issue_id: issue_id.to_owned(),
                    marker: comment_marker(&comment_key),
                    body,
                },
            ),
        ],
        transition_key,
        comment_key,
    }
}

/// Observed delivery state of the two claim effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct ClaimEffects {
    pub transition_delivered: bool,
    pub comment_delivered: bool,
    pub conflict_detected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "readiness")]
pub enum ClaimReadiness {
    AwaitingLinearConfirmation { pending: Vec<&'static str> },
    ReadyToQueue,
    FailedSafely { operator_detail: String },
}

/// Decides whether a claim may become `queued`.
///
/// `canonical_state_is_claim_target` is the state Linear reports right now. A
/// conflicting claim is only released once Linear no longer shows the claim
/// target, so a partially applied claim is never abandoned while it is visible.
pub fn claim_readiness(
    effects: ClaimEffects,
    canonical_state_is_claim_target: bool,
) -> ClaimReadiness {
    if effects.conflict_detected && !effects.transition_delivered {
        return if canonical_state_is_claim_target {
            ClaimReadiness::AwaitingLinearConfirmation {
                pending: vec!["claim_comment"],
            }
        } else {
            ClaimReadiness::FailedSafely {
                operator_detail:
                    "a human moved the ticket before the claim was applied; it returns to eligible"
                        .to_owned(),
            }
        };
    }
    let mut pending = Vec::new();
    if !effects.transition_delivered {
        pending.push("linear_transition");
    }
    if !effects.comment_delivered {
        pending.push("claim_comment");
    }
    if pending.is_empty() {
        ClaimReadiness::ReadyToQueue
    } else {
        ClaimReadiness::AwaitingLinearConfirmation { pending }
    }
}

/// A harness may only start from `queued`, which is reachable only after both
/// claim effects are confirmed.
pub fn harness_start_permitted(state: WorkItemState) -> bool {
    state == WorkItemState::Queued
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_claim_publishes_one_transition_and_one_comment_with_stable_keys() {
        let plan = claim_plan("run-1", "issue-1", "SPI-42", "ready", "progress");
        assert_eq!(plan.transition_key, "transition:run-1:progress");
        assert_eq!(plan.comment_key, "claim:run-1");
        assert_eq!(
            plan,
            claim_plan("run-1", "issue-1", "SPI-42", "ready", "progress")
        );
        let comment = &plan.actions[1].payload;
        assert!(comment["body"].as_str().unwrap().contains("run-1"));
        assert!(comment["body"].as_str().unwrap().contains("SPI-42"));
    }

    #[test]
    fn a_harness_starts_only_after_both_claim_effects_are_confirmed() {
        assert_eq!(
            claim_readiness(ClaimEffects::default(), false),
            ClaimReadiness::AwaitingLinearConfirmation {
                pending: vec!["linear_transition", "claim_comment"],
            }
        );
        assert_eq!(
            claim_readiness(
                ClaimEffects {
                    transition_delivered: true,
                    ..ClaimEffects::default()
                },
                true
            ),
            ClaimReadiness::AwaitingLinearConfirmation {
                pending: vec!["claim_comment"],
            }
        );
        assert_eq!(
            claim_readiness(
                ClaimEffects {
                    transition_delivered: true,
                    comment_delivered: true,
                    conflict_detected: false,
                },
                true
            ),
            ClaimReadiness::ReadyToQueue
        );
        for state in [
            WorkItemState::Eligible,
            WorkItemState::Claiming,
            WorkItemState::Implementing,
        ] {
            assert!(!harness_start_permitted(state));
        }
        assert!(harness_start_permitted(WorkItemState::Queued));
    }

    #[test]
    fn a_conflicting_claim_is_released_only_when_linear_permits() {
        let conflicted = ClaimEffects {
            conflict_detected: true,
            ..ClaimEffects::default()
        };
        assert!(matches!(
            claim_readiness(conflicted, true),
            ClaimReadiness::AwaitingLinearConfirmation { .. }
        ));
        assert!(matches!(
            claim_readiness(conflicted, false),
            ClaimReadiness::FailedSafely { .. }
        ));
    }
}
