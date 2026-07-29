//! Offline parser and transient-unit specification for harness execution.
//! Actual process start is deliberately absent until captured provider contracts
//! and systemd authority are available.
use spire_application::{RunOutcome, StructuredRunResult, parse_structured_result};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransientUnitSpec {
    pub unit_name: String,
    pub working_directory: String,
    pub evidence_path: String,
    pub timeout_seconds: u64,
}
pub fn transient_unit(
    run_id: &str,
    worktree: &str,
    evidence_path: &str,
    timeout_seconds: u64,
) -> Result<TransientUnitSpec, HarnessError> {
    if run_id.is_empty() || worktree.is_empty() || evidence_path.is_empty() || timeout_seconds == 0
    {
        return Err(HarnessError::InvalidSpec);
    }
    Ok(TransientUnitSpec {
        unit_name: format!("spire-run-{run_id}.service"),
        working_directory: worktree.into(),
        evidence_path: evidence_path.into(),
        timeout_seconds,
    })
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum HarnessError {
    #[error("harness contract is invalid")]
    InvalidContract,
    #[error("transient unit specification is invalid")]
    InvalidSpec,
}
pub fn parse_jsonl_result(jsonl: &str) -> Result<StructuredRunResult, HarnessError> {
    let result = jsonl
        .lines()
        .rev()
        .find_map(|line| parse_structured_result(line).ok())
        .ok_or(HarnessError::InvalidContract)?;
    Ok(result)
}
pub fn should_open_circuit(outcome: RunOutcome) -> bool {
    matches!(
        outcome,
        RunOutcome::RateLimited
            | RunOutcome::QuotaExhausted
            | RunOutcome::AuthFailed
            | RunOutcome::ModelUnavailable
    )
}

/// A review is a distinct provider invocation: it receives no session reference
/// and is constrained to a read-only checkout. The actual process runner must
/// use this specification instead of a maker specification for review roles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadOnlyReviewSpec {
    pub unit: TransientUnitSpec,
    pub permission_mode: &'static str,
    pub provider_session_reference: Option<String>,
}

pub fn read_only_review_unit(
    run_id: &str,
    worktree: &str,
    evidence_path: &str,
    timeout_seconds: u64,
) -> Result<ReadOnlyReviewSpec, HarnessError> {
    Ok(ReadOnlyReviewSpec {
        unit: transient_unit(run_id, worktree, evidence_path, timeout_seconds)?,
        permission_mode: "read_only",
        provider_session_reference: None,
    })
}

/// The caller snapshots tracked-file status before and after invoking the
/// reviewer. Any change is a contract breach, including an attempted approval.
pub fn review_worktree_is_unchanged(before: &str, after: &str) -> Result<(), HarnessError> {
    if before == after {
        Ok(())
    } else {
        Err(HarnessError::InvalidContract)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unknown_jsonl_and_preserves_capacity_taxonomy() {
        assert_eq!(parse_jsonl_result("{}"), Err(HarnessError::InvalidContract));
        let result = parse_jsonl_result("noise\n{\"schema_version\":1,\"outcome\":\"quota_exhausted\",\"session_id\":null,\"evidence_reference\":\"evidence/run.jsonl\"}").unwrap();
        assert_eq!(result.outcome, RunOutcome::QuotaExhausted);
        assert!(should_open_circuit(result.outcome));
    }

    #[test]
    fn review_spec_has_no_maker_session_or_write_authority() {
        let review = read_only_review_unit("review-1", "/worktree", "/evidence", 60).unwrap();
        assert_eq!(review.permission_mode, "read_only");
        assert_eq!(review.provider_session_reference, None);
        assert!(review_worktree_is_unchanged("", "M src/lib.rs").is_err());
    }
}
