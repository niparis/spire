use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunInput {
    pub run_id: String,
    pub work_item_id: String,
    pub parent_run_id: Option<String>,
    pub root_run_id: String,
    pub harness: String,
    pub model: String,
    pub effort: String,
    pub ticket_revision: String,
    pub repository: String,
    pub branch: String,
    pub worktree: String,
    pub base_sha: String,
    pub deadline_unix_seconds: i64,
    pub permission_mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcome {
    PrReady,
    SpecsNeeded,
    Blocked,
    NoChange,
    TaskFailed,
    Approved,
    ChangesRequired,
    RateLimited,
    QuotaExhausted,
    ContextExhausted,
    OutputLimit,
    AuthFailed,
    ModelUnavailable,
    RunnerUnhealthy,
    ContractInvalid,
    UnknownProviderFailure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredRunResult {
    pub schema_version: u8,
    pub outcome: RunOutcome,
    pub session_id: Option<String>,
    pub evidence_reference: String,
}

pub fn parse_structured_result(input: &str) -> Result<StructuredRunResult, RunOutcome> {
    let result: StructuredRunResult =
        serde_json::from_str(input).map_err(|_| RunOutcome::ContractInvalid)?;
    if result.schema_version != 1 || result.evidence_reference.trim().is_empty() {
        return Err(RunOutcome::ContractInvalid);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_or_unknown_contracts_fail_closed() {
        assert_eq!(
            parse_structured_result("not json"),
            Err(RunOutcome::ContractInvalid)
        );
        assert_eq!(
            parse_structured_result(
                r#"{"schema_version":2,"outcome":"pr_ready","session_id":null,"evidence_reference":"run.log"}"#
            ),
            Err(RunOutcome::ContractInvalid)
        );
        assert!(matches!(
            parse_structured_result(
                r#"{"schema_version":1,"outcome":"rate_limited","session_id":"s","evidence_reference":"run.log"}"#
            ),
            Ok(StructuredRunResult {
                outcome: RunOutcome::RateLimited,
                ..
            })
        ));
    }
}
