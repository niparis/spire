//! Vocabulary for external effects. Use cases plan actions; the outbox worker
//! delivers them. An action is only ever committed in the same transaction as
//! the state change that requires it.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboxKind {
    LinearTransition,
    LinearComment,
    GithubReviewSummary,
    OperatorNotification,
}

impl OutboxKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LinearTransition => "linear_transition",
            Self::LinearComment => "linear_comment",
            Self::GithubReviewSummary => "github_review_summary",
            Self::OperatorNotification => "operator_notification",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::LinearTransition,
            Self::LinearComment,
            Self::GithubReviewSummary,
            Self::OperatorNotification,
        ]
        .into_iter()
        .find(|kind| kind.as_str() == value)
    }
}

/// One planned external effect. `idempotency_key` is the durable uniqueness
/// constraint, so replaying a plan can never create a second visible effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlannedAction {
    pub kind: OutboxKind,
    pub aggregate_id: String,
    pub idempotency_key: String,
    pub payload: Value,
}

/// Conditional transition payload. The expected state is carried so the adapter
/// can re-read Linear and refuse to overwrite a human's newer decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinearTransitionPayload {
    pub issue_id: String,
    pub expected_state_id: String,
    pub target_state_id: String,
    pub run_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinearCommentPayload {
    pub issue_id: String,
    pub marker: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperatorNotificationPayload {
    pub severity: String,
    pub subject: String,
    pub body: String,
}

pub fn linear_transition(
    aggregate_id: &str,
    idempotency_key: String,
    payload: LinearTransitionPayload,
) -> PlannedAction {
    PlannedAction {
        kind: OutboxKind::LinearTransition,
        aggregate_id: aggregate_id.to_owned(),
        idempotency_key,
        payload: serde_json::to_value(payload).expect("transition payload is serializable"),
    }
}

pub fn linear_comment(
    aggregate_id: &str,
    idempotency_key: String,
    payload: LinearCommentPayload,
) -> PlannedAction {
    PlannedAction {
        kind: OutboxKind::LinearComment,
        aggregate_id: aggregate_id.to_owned(),
        idempotency_key,
        payload: serde_json::to_value(payload).expect("comment payload is serializable"),
    }
}

pub fn operator_notification(
    aggregate_id: &str,
    idempotency_key: String,
    severity: &str,
    subject: &str,
    body: &str,
) -> PlannedAction {
    PlannedAction {
        kind: OutboxKind::OperatorNotification,
        aggregate_id: aggregate_id.to_owned(),
        idempotency_key,
        payload: serde_json::json!({
            "severity": severity,
            "subject": subject,
            "body": body,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_round_trip_through_their_persisted_names() {
        for kind in [
            OutboxKind::LinearTransition,
            OutboxKind::LinearComment,
            OutboxKind::OperatorNotification,
        ] {
            assert_eq!(OutboxKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(OutboxKind::parse("start_harness"), None);
    }
}
