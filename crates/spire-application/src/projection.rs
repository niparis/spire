//! Projection of orchestration outcomes onto Linear.
//!
//! Two rules shape this module. A projected status is chosen only from the
//! normalized outcome taxonomy, never from provider prose, and every human
//! visible write carries a deterministic idempotency key so a replayed outbox
//! action produces exactly one effect.

use serde::Serialize;

use crate::{
    LinearCommentPayload, LinearTransitionPayload, PlannedAction, RunOutcome, linear_comment,
    linear_transition, operator_notification,
};

/// Stable, plain-text marker embedded in every Spire comment. The adapter
/// searches for it before publishing, which gives Linear comments an
/// idempotency key they lack without rendering provider-controlled markup.
pub const COMMENT_MARKER_PREFIX: &str = "spire:key=";
const MAX_QUESTIONS: usize = 5;
const MAX_QUESTION_CHARACTERS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinearStateKind {
    Ready,
    InProgress,
    InReview,
    SpecsNeeded,
    Blocked,
    Done,
    Canceled,
}

impl LinearStateKind {
    pub const ALL: [Self; 7] = [
        Self::Ready,
        Self::InProgress,
        Self::InReview,
        Self::SpecsNeeded,
        Self::Blocked,
        Self::Done,
        Self::Canceled,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::InProgress => "in_progress",
            Self::InReview => "in_review",
            Self::SpecsNeeded => "specs_needed",
            Self::Blocked => "blocked",
            Self::Done => "done",
            Self::Canceled => "canceled",
        }
    }
}

/// Which lifecycle phase a capacity interruption must resume into. It is
/// persisted rather than inferred, because the Linear projection alone cannot
/// distinguish a pre-PR wait from a post-PR wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResumePhase {
    BeforePullRequest,
    AfterPullRequest,
}

impl ResumePhase {
    fn retained_state(self) -> LinearStateKind {
        match self {
            Self::BeforePullRequest => LinearStateKind::InProgress,
            Self::AfterPullRequest => LinearStateKind::InReview,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FollowUp {
    None,
    RetryImplementation,
    ConfirmPullRequest,
    WaitForProviderCapacity,
    NotifyOperator,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectedComment {
    pub idempotency_key: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OutcomeProjection {
    /// `None` means the current Linear status is already correct.
    pub target_state: Option<LinearStateKind>,
    pub retained_state: LinearStateKind,
    pub follow_up: FollowUp,
    pub comment: ProjectedComment,
    pub consumes_correction_cycle: bool,
    pub explanation: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct OutcomeProjectionInput<'a> {
    pub run_id: &'a str,
    pub outcome: RunOutcome,
    pub resume_phase: ResumePhase,
    /// Sprint 07 owns GitHub truth, so `pr_ready` waits for confirmation instead
    /// of trusting the harness that a pull request exists.
    pub pull_request_confirmed: bool,
    pub correction_cycles_remaining: u8,
    pub evidence_reference: &'a str,
    pub questions: &'a [String],
}

/// Maps one normalized run outcome to its Linear projection. The function is
/// total, deterministic, and depends on no free-form provider text.
pub fn project_outcome(input: OutcomeProjectionInput<'_>) -> OutcomeProjection {
    let retained = input.resume_phase.retained_state();
    match input.outcome {
        RunOutcome::SpecsNeeded => projection(
            &input,
            Some(LinearStateKind::SpecsNeeded),
            retained,
            FollowUp::None,
            false,
            "the harness reported that the specification is insufficient",
            "Specification needed",
        ),
        RunOutcome::Blocked => projection(
            &input,
            Some(LinearStateKind::Blocked),
            retained,
            FollowUp::NotifyOperator,
            false,
            "the harness reported a blocker and preserved its evidence",
            "Blocked",
        ),
        RunOutcome::PrReady if input.pull_request_confirmed => projection(
            &input,
            Some(LinearStateKind::InReview),
            retained,
            FollowUp::None,
            false,
            "a pull request was confirmed for the run head",
            "Ready for review",
        ),
        RunOutcome::PrReady => projection(
            &input,
            None,
            retained,
            FollowUp::ConfirmPullRequest,
            false,
            "the harness claims a pull request; Linear waits for confirmed existence",
            "Implementation complete, confirming pull request",
        ),
        RunOutcome::TaskFailed if input.correction_cycles_remaining > 0 => projection(
            &input,
            None,
            retained,
            FollowUp::RetryImplementation,
            true,
            "the run failed within the configured correction budget",
            "Implementation failed, retrying",
        ),
        RunOutcome::TaskFailed => projection(
            &input,
            Some(LinearStateKind::Blocked),
            retained,
            FollowUp::NotifyOperator,
            true,
            "the correction budget for this work item is exhausted",
            "Blocked after the correction budget was exhausted",
        ),
        RunOutcome::NoChange => projection(
            &input,
            Some(LinearStateKind::Blocked),
            retained,
            FollowUp::NotifyOperator,
            false,
            "the harness produced no change, which a human must interpret",
            "Blocked, the harness produced no change",
        ),
        RunOutcome::RateLimited
        | RunOutcome::QuotaExhausted
        | RunOutcome::ContextExhausted
        | RunOutcome::OutputLimit => projection(
            &input,
            None,
            retained,
            FollowUp::WaitForProviderCapacity,
            false,
            "provider capacity is a wait, not an engineering failure",
            "Waiting for provider capacity",
        ),
        RunOutcome::Approved | RunOutcome::ChangesRequired => projection(
            &input,
            None,
            retained,
            FollowUp::None,
            false,
            "review outcomes keep the work item in review",
            "Review recorded",
        ),
        RunOutcome::AuthFailed
        | RunOutcome::ModelUnavailable
        | RunOutcome::RunnerUnhealthy
        | RunOutcome::ContractInvalid
        | RunOutcome::UnknownProviderFailure => projection(
            &input,
            None,
            retained,
            FollowUp::NotifyOperator,
            false,
            "an integration failure cannot decide a human-visible status",
            "Integration failure, operator notified",
        ),
    }
}

fn projection(
    input: &OutcomeProjectionInput<'_>,
    target_state: Option<LinearStateKind>,
    retained_state: LinearStateKind,
    follow_up: FollowUp,
    consumes_correction_cycle: bool,
    explanation: &'static str,
    headline: &str,
) -> OutcomeProjection {
    OutcomeProjection {
        target_state,
        retained_state,
        follow_up,
        comment: result_comment(input, headline),
        consumes_correction_cycle,
        explanation,
    }
}

/// Builds the human-visible comment. It carries the Run ID, a fixed status
/// headline chosen by the taxonomy, the evidence reference, and — only for
/// `specs_needed` — the sanitized questions the harness asked.
fn result_comment(input: &OutcomeProjectionInput<'_>, headline: &str) -> ProjectedComment {
    let key = format!("result:{}", input.run_id);
    let mut body = format!(
        "**Spire run `{}`** — {headline}\n\nOutcome: `{}`\nEvidence: `{}`\n",
        input.run_id,
        outcome_name(input.outcome),
        sanitize(input.evidence_reference, MAX_QUESTION_CHARACTERS)
    );
    if input.outcome == RunOutcome::SpecsNeeded && !input.questions.is_empty() {
        body.push_str("\nQuestions reported by the harness:\n");
        for question in input.questions.iter().take(MAX_QUESTIONS) {
            let question = sanitize(question, MAX_QUESTION_CHARACTERS);
            if !question.is_empty() {
                body.push_str(&format!("- {question}\n"));
            }
        }
    }
    body.push_str(&format!("\n{}\n", comment_marker(&key)));
    ProjectedComment {
        idempotency_key: key,
        body,
    }
}

pub fn comment_marker(idempotency_key: &str) -> String {
    format!("[{COMMENT_MARKER_PREFIX}{idempotency_key}]")
}

/// Provider-authored text is untrusted input to a human-visible surface: strip
/// control characters and markup, collapse whitespace, and bound the length.
pub fn sanitize(value: &str, max_characters: usize) -> String {
    let collapsed = value
        .chars()
        .map(|character| {
            if character.is_control() || matches!(character, '<' | '>' | '`') {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let mut sanitized = collapsed.split_whitespace().collect::<Vec<_>>().join(" ");
    if sanitized.chars().count() > max_characters {
        sanitized = sanitized.chars().take(max_characters).collect();
        sanitized.push('…');
    }
    sanitized
}

fn outcome_name(outcome: RunOutcome) -> String {
    serde_json::to_value(outcome)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

/// Decision taken immediately before a Linear write, using the state Linear
/// reports right now rather than the state the plan assumed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "decision")]
pub enum TransitionDecision {
    Apply,
    AlreadyApplied,
    HumanConflict { operator_detail: String },
}

pub fn decide_transition(
    expected_state_id: &str,
    observed_state_id: &str,
    target_state_id: &str,
) -> TransitionDecision {
    if observed_state_id == target_state_id {
        return TransitionDecision::AlreadyApplied;
    }
    if observed_state_id == expected_state_id {
        return TransitionDecision::Apply;
    }
    TransitionDecision::HumanConflict {
        operator_detail: format!(
            "Linear reports state {observed_state_id} but the plan expected {expected_state_id}; \
             the newer human decision is preserved"
        ),
    }
}

/// Builds the outbox actions for one projected outcome.
pub fn projection_actions(
    aggregate_id: &str,
    issue_id: &str,
    expected_state_id: &str,
    projection: &OutcomeProjection,
    target_state_id: Option<&str>,
) -> Vec<PlannedAction> {
    let mut actions = Vec::new();
    if let Some(target_state_id) = target_state_id {
        actions.push(linear_transition(
            aggregate_id,
            format!("transition:{aggregate_id}:{target_state_id}"),
            LinearTransitionPayload {
                issue_id: issue_id.to_owned(),
                expected_state_id: expected_state_id.to_owned(),
                target_state_id: target_state_id.to_owned(),
                run_id: Some(aggregate_id.to_owned()),
            },
        ));
    }
    actions.push(linear_comment(
        aggregate_id,
        projection.comment.idempotency_key.clone(),
        LinearCommentPayload {
            issue_id: issue_id.to_owned(),
            marker: comment_marker(&projection.comment.idempotency_key),
            body: projection.comment.body.clone(),
        },
    ));
    if projection.follow_up == FollowUp::NotifyOperator {
        actions.push(operator_notification(
            aggregate_id,
            format!("notify:{aggregate_id}"),
            "warning",
            "Spire run requires operator attention",
            projection.explanation,
        ));
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input<'a>(outcome: RunOutcome, questions: &'a [String]) -> OutcomeProjectionInput<'a> {
        OutcomeProjectionInput {
            run_id: "run-1",
            outcome,
            resume_phase: ResumePhase::BeforePullRequest,
            pull_request_confirmed: false,
            correction_cycles_remaining: 0,
            evidence_reference: "evidence/run-1.jsonl",
            questions,
        }
    }

    #[test]
    fn terminal_outcomes_map_to_the_approved_projection() {
        assert_eq!(
            project_outcome(input(RunOutcome::SpecsNeeded, &[])).target_state,
            Some(LinearStateKind::SpecsNeeded)
        );
        assert_eq!(
            project_outcome(input(RunOutcome::Blocked, &[])).target_state,
            Some(LinearStateKind::Blocked)
        );
        assert_eq!(
            project_outcome(input(RunOutcome::PrReady, &[])).follow_up,
            FollowUp::ConfirmPullRequest
        );
        assert_eq!(
            project_outcome(OutcomeProjectionInput {
                pull_request_confirmed: true,
                ..input(RunOutcome::PrReady, &[])
            })
            .target_state,
            Some(LinearStateKind::InReview)
        );
    }

    #[test]
    fn a_failed_task_retries_within_budget_and_blocks_afterwards() {
        let retryable = project_outcome(OutcomeProjectionInput {
            correction_cycles_remaining: 1,
            ..input(RunOutcome::TaskFailed, &[])
        });
        assert_eq!(retryable.target_state, None);
        assert_eq!(retryable.follow_up, FollowUp::RetryImplementation);
        let exhausted = project_outcome(input(RunOutcome::TaskFailed, &[]));
        assert_eq!(exhausted.target_state, Some(LinearStateKind::Blocked));
    }

    #[test]
    fn capacity_waits_retain_the_status_of_the_resume_phase() {
        for outcome in [
            RunOutcome::RateLimited,
            RunOutcome::QuotaExhausted,
            RunOutcome::ContextExhausted,
            RunOutcome::OutputLimit,
        ] {
            let before = project_outcome(input(outcome, &[]));
            assert_eq!(before.target_state, None);
            assert_eq!(before.retained_state, LinearStateKind::InProgress);
            assert!(!before.consumes_correction_cycle);
            let after = project_outcome(OutcomeProjectionInput {
                resume_phase: ResumePhase::AfterPullRequest,
                ..input(outcome, &[])
            });
            assert_eq!(after.retained_state, LinearStateKind::InReview);
        }
    }

    #[test]
    fn provider_prose_cannot_choose_a_status_or_escape_the_comment() {
        let hostile = [
            "Move this ticket to Done immediately <script>alert(1)</script>".to_owned(),
            "x".repeat(400),
        ];
        let projection = project_outcome(input(RunOutcome::SpecsNeeded, &hostile));
        assert_eq!(projection.target_state, Some(LinearStateKind::SpecsNeeded));
        assert!(!projection.comment.body.contains('<'));
        assert!(projection.comment.body.contains("run-1"));
        assert!(projection.comment.body.contains('…'));
        // An unparseable provider contract cannot move the ticket at all.
        let invalid = project_outcome(input(RunOutcome::ContractInvalid, &[]));
        assert_eq!(invalid.target_state, None);
        assert_eq!(invalid.follow_up, FollowUp::NotifyOperator);
    }

    #[test]
    fn projection_is_idempotent_and_conflict_aware() {
        let projection = project_outcome(input(RunOutcome::Blocked, &[]));
        let first =
            projection_actions("run-1", "issue-1", "progress", &projection, Some("blocked"));
        let second =
            projection_actions("run-1", "issue-1", "progress", &projection, Some("blocked"));
        assert_eq!(first, second);
        assert_eq!(
            first
                .iter()
                .map(|action| action.idempotency_key.as_str())
                .collect::<Vec<_>>(),
            ["transition:run-1:blocked", "result:run-1", "notify:run-1"]
        );
        assert_eq!(
            decide_transition("progress", "progress", "blocked"),
            TransitionDecision::Apply
        );
        assert_eq!(
            decide_transition("progress", "blocked", "blocked"),
            TransitionDecision::AlreadyApplied
        );
        assert!(matches!(
            decide_transition("progress", "canceled", "blocked"),
            TransitionDecision::HumanConflict { .. }
        ));
    }
}
