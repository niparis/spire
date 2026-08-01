//! Independent review contracts and deterministic review-loop policy.
//!
//! Reviewers provide structured findings for one exact PR head.  This module
//! deliberately has no provider, GitHub, or persistence dependencies.

use serde::{Deserialize, Serialize};
use spire_domain::{
    CommitSha, DispatchEvaluation, HarnessCapabilityRegistry, HarnessId, ProviderHealth, RunRole,
};

use crate::{CapacityCounts, CapacityLimits, ClaimBlock, SchedulerInitiator, capacity_allows};

pub const REVIEW_RESULT_SCHEMA_VERSION: u8 = 1;
pub const MAX_INITIAL_REVIEW_CORRECTION_CYCLES: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewVerdict {
    Approved,
    ChangesRequired,
    Blocked,
}

impl ReviewVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::ChangesRequired => "changes_required",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewFinding {
    pub stable_id: String,
    pub severity: FindingSeverity,
    pub file: String,
    pub line: Option<u32>,
    pub title: String,
    pub rationale: String,
    pub requested_change: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewResult {
    pub schema_version: u8,
    pub verdict: ReviewVerdict,
    pub reviewed_head_sha: CommitSha,
    pub summary: String,
    pub findings: Vec<ReviewFinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReviewContractError {
    #[error("review result schema_version must be {REVIEW_RESULT_SCHEMA_VERSION}")]
    UnsupportedSchemaVersion,
    #[error("review result does not match the requested head SHA")]
    WrongHeadSha,
    #[error("review result summary is required")]
    MissingSummary,
    #[error("review finding {stable_id:?} is invalid: {field} is required")]
    InvalidFinding {
        stable_id: String,
        field: &'static str,
    },
    #[error("review finding {0:?} is duplicated")]
    DuplicateFinding(String),
    #[error("an approval cannot contain unresolved findings")]
    ApprovalHasFindings,
    #[error("changes_required must include at least one actionable finding")]
    ChangesRequiredWithoutFindings,
}

impl ReviewResult {
    pub fn validate_for(&self, requested_head_sha: &CommitSha) -> Result<(), ReviewContractError> {
        if self.schema_version != REVIEW_RESULT_SCHEMA_VERSION {
            return Err(ReviewContractError::UnsupportedSchemaVersion);
        }
        if &self.reviewed_head_sha != requested_head_sha {
            return Err(ReviewContractError::WrongHeadSha);
        }
        if self.summary.trim().is_empty() {
            return Err(ReviewContractError::MissingSummary);
        }
        let mut finding_ids = std::collections::BTreeSet::new();
        for finding in &self.findings {
            for (field, value) in [
                ("stable_id", finding.stable_id.as_str()),
                ("file", finding.file.as_str()),
                ("title", finding.title.as_str()),
                ("rationale", finding.rationale.as_str()),
                ("requested_change", finding.requested_change.as_str()),
            ] {
                if value.trim().is_empty() {
                    return Err(ReviewContractError::InvalidFinding {
                        stable_id: finding.stable_id.clone(),
                        field,
                    });
                }
            }
            if finding.line == Some(0) {
                return Err(ReviewContractError::InvalidFinding {
                    stable_id: finding.stable_id.clone(),
                    field: "line",
                });
            }
            if !finding_ids.insert(&finding.stable_id) {
                return Err(ReviewContractError::DuplicateFinding(
                    finding.stable_id.clone(),
                ));
            }
        }
        match self.verdict {
            ReviewVerdict::Approved if !self.findings.is_empty() => {
                Err(ReviewContractError::ApprovalHasFindings)
            }
            ReviewVerdict::ChangesRequired if self.findings.is_empty() => {
                Err(ReviewContractError::ChangesRequiredWithoutFindings)
            }
            _ => Ok(()),
        }
    }
}

pub fn parse_review_result(
    input: &str,
    requested_head_sha: &CommitSha,
) -> Result<ReviewResult, ReviewContractError> {
    let result: ReviewResult =
        serde_json::from_str(input).map_err(|_| ReviewContractError::UnsupportedSchemaVersion)?;
    result.validate_for(requested_head_sha)?;
    Ok(result)
}

/// Evaluates the persisted review candidates. `DispatchPolicy::evaluate` keeps
/// every rejected candidate in the audit trail and cannot choose the maker.
pub fn evaluate_review_dispatch(
    policy: &spire_domain::DispatchPolicy,
    capabilities: &HarnessCapabilityRegistry,
    complexity: spire_domain::ComplexityClass,
    health: &[ProviderHealth],
    sticky_maker: &HarnessId,
) -> Result<DispatchEvaluation, spire_domain::DispatchPolicyError> {
    policy.evaluate(
        capabilities,
        RunRole::Review,
        complexity,
        health,
        Some(sticky_maker),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewCorrectionDecision {
    Dispatch { next_cycle: u8 },
    WaitForCapacity(ClaimBlock),
    Exhausted,
}

pub fn plan_review_correction(
    completed_cycles: u8,
    limits: CapacityLimits,
    counts: CapacityCounts,
) -> ReviewCorrectionDecision {
    if completed_cycles >= MAX_INITIAL_REVIEW_CORRECTION_CYCLES {
        return ReviewCorrectionDecision::Exhausted;
    }
    match capacity_allows(limits, counts, SchedulerInitiator::Ai) {
        Ok(()) => ReviewCorrectionDecision::Dispatch {
            next_cycle: completed_cycles + 1,
        },
        Err(block) => ReviewCorrectionDecision::WaitForCapacity(block),
    }
}

/// The data deliberately excludes maker session IDs, provider transcripts, and
/// write credentials. The harness gets a new session for each review SHA.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FreshReviewContext {
    pub ticket: String,
    pub repository_instructions: String,
    pub pull_request_number: u64,
    pub base_sha: CommitSha,
    pub head_sha: CommitSha,
    pub required_ci_evidence: Vec<String>,
    pub permission_mode: &'static str,
}

impl FreshReviewContext {
    pub fn new(
        ticket: String,
        repository_instructions: String,
        pull_request_number: u64,
        base_sha: CommitSha,
        head_sha: CommitSha,
        required_ci_evidence: Vec<String>,
    ) -> Self {
        Self {
            ticket,
            repository_instructions,
            pull_request_number,
            base_sha,
            head_sha,
            required_ci_evidence,
            permission_mode: "read_only",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use spire_domain::{
        ComplexityClass, DispatchCandidate, DispatchPolicy, DispatchPolicyVersion, DispatchRule,
        DispatchRuleId, Effort, ModelId,
    };

    fn sha(value: &str) -> CommitSha {
        CommitSha::new(value).unwrap()
    }
    fn finding() -> ReviewFinding {
        ReviewFinding {
            stable_id: "SPI-1".into(),
            severity: FindingSeverity::High,
            file: "src/lib.rs".into(),
            line: Some(3),
            title: "Bug".into(),
            rationale: "Incorrect result".into(),
            requested_change: "Correct the result".into(),
        }
    }

    #[test]
    fn review_results_are_sha_bound_and_actionable() {
        let result = ReviewResult {
            schema_version: 1,
            verdict: ReviewVerdict::ChangesRequired,
            reviewed_head_sha: sha("head"),
            summary: "One fix needed".into(),
            findings: vec![finding()],
        };
        assert!(result.validate_for(&sha("head")).is_ok());
        assert_eq!(
            result.validate_for(&sha("new")),
            Err(ReviewContractError::WrongHeadSha)
        );
        let approved = ReviewResult {
            verdict: ReviewVerdict::Approved,
            ..result.clone()
        };
        assert_eq!(
            approved.validate_for(&sha("head")),
            Err(ReviewContractError::ApprovalHasFindings)
        );
    }

    #[test]
    fn review_correction_capacity_does_not_consume_a_round() {
        let limits = CapacityLimits {
            total: 3,
            ai: 1,
            per_repository: 1,
            per_ticket: 1,
        };
        assert_eq!(
            plan_review_correction(
                1,
                limits,
                CapacityCounts {
                    ai: 1,
                    ..CapacityCounts::default()
                }
            ),
            ReviewCorrectionDecision::WaitForCapacity(ClaimBlock::AiCapacity)
        );
        assert_eq!(
            plan_review_correction(3, limits, CapacityCounts::default()),
            ReviewCorrectionDecision::Exhausted
        );
    }

    #[test]
    fn dispatch_marks_the_maker_ineligible() {
        let codex = HarnessId::new("codex").unwrap();
        let claude = HarnessId::new("claude-code").unwrap();
        let candidate = |harness: HarnessId| DispatchCandidate {
            harness,
            model: ModelId::new("model").unwrap(),
            effort: Effort::High,
        };
        let mut capabilities = HarnessCapabilityRegistry::default();
        for harness in [codex.clone(), claude.clone()] {
            capabilities.register(
                harness,
                [(
                    ModelId::new("model").unwrap(),
                    BTreeSet::from([Effort::High]),
                )],
            );
        }
        let policy = DispatchPolicy {
            policy_version: DispatchPolicyVersion::new(1).unwrap(),
            rules: vec![
                DispatchRule {
                    id: DispatchRuleId::new("implementation").unwrap(),
                    role: RunRole::Implementation,
                    complexity: ComplexityClass::ALL.to_vec(),
                    candidates: vec![candidate(codex.clone()), candidate(claude.clone())],
                },
                DispatchRule {
                    id: DispatchRuleId::new("review").unwrap(),
                    role: RunRole::Review,
                    complexity: ComplexityClass::ALL.to_vec(),
                    candidates: vec![candidate(codex), candidate(claude)],
                },
            ],
        };
        let evaluation = evaluate_review_dispatch(
            &policy,
            &capabilities,
            ComplexityClass::Small,
            &[],
            &HarnessId::new("codex").unwrap(),
        )
        .unwrap();
        assert_eq!(
            evaluation.candidates[0].reason,
            spire_domain::CandidateSkipReason::SameAsMaker
        );
        assert_eq!(
            evaluation.selected,
            Some(spire_domain::CandidateIndex::new(1))
        );
    }
}
