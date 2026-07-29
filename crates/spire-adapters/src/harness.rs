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
}
