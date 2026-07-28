use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use spire_domain::{ComplexityClass, LinearIssueId, RepositoryName, RunRole};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalLinearIssue {
    pub id: LinearIssueId,
    pub identifier: String,
    pub team_id: String,
    pub workflow_state_id: String,
    pub estimate: Option<u8>,
    pub priority: Option<u8>,
    pub labels: BTreeSet<String>,
    pub blockers: Vec<LinearIssueId>,
    pub description: Option<String>,
    pub acceptance_criteria: Option<String>,
    pub assignee_id: Option<String>,
    pub creator_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub revision: String,
}

impl CanonicalLinearIssue {
    pub fn revision(updated_at: &str, content: &str) -> String {
        let digest = Sha256::digest(content.as_bytes());
        format!("{updated_at}:{}", hex::encode(digest))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryMapping {
    pub label: String,
    pub repository: RepositoryName,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EligibilityResult {
    Eligible {
        repository: RepositoryName,
        complexity: ComplexityClass,
    },
    Ineligible {
        reason: EligibilityReason,
        operator_detail: String,
    },
    WaitingForDependency {
        blockers: Vec<LinearIssueId>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EligibilityReason {
    NotReady,
    UnsupportedType,
    MissingAcceptanceCriteria,
    ComplexityMissing,
    ComplexityUnmapped,
    RepositoryUnmapped,
    RepositoryAmbiguous,
    AutomationDisabled,
    AlreadyActive,
    LocallyTerminal,
    DispatchCoverageMissing,
}

#[derive(Debug, Clone)]
pub struct EligibilityInput<'a> {
    pub issue: &'a CanonicalLinearIssue,
    pub ready_state_id: &'a str,
    pub supported_type_labels: &'a BTreeSet<String>,
    pub repository_mappings: &'a [RepositoryMapping],
    pub complexity_mapping: &'a std::collections::BTreeMap<u8, ComplexityClass>,
    pub incomplete_blockers: &'a BTreeSet<LinearIssueId>,
    pub locally_active: bool,
    pub locally_terminal: bool,
    pub dispatch_covers_implementation_and_review: bool,
}

pub fn evaluate_eligibility(input: EligibilityInput<'_>) -> EligibilityResult {
    let issue = input.issue;
    if issue.workflow_state_id != input.ready_state_id {
        return ineligible(
            EligibilityReason::NotReady,
            "issue is not in the configured Ready-for-Agent workflow state",
        );
    }
    if !issue
        .labels
        .iter()
        .any(|label| input.supported_type_labels.contains(label))
    {
        return ineligible(
            EligibilityReason::UnsupportedType,
            "issue has no configured supported work-type label",
        );
    }
    if issue.labels.iter().any(|label| {
        matches!(
            label.as_str(),
            "type:architecture" | "type:adr" | "type:spike"
        )
    }) {
        return ineligible(
            EligibilityReason::UnsupportedType,
            "architecture, ADR, and spike work is excluded from automation",
        );
    }
    if issue
        .acceptance_criteria
        .as_deref()
        .is_none_or(|value| value.trim().is_empty())
    {
        return ineligible(
            EligibilityReason::MissingAcceptanceCriteria,
            "issue has no acceptance-criteria evidence",
        );
    }
    let Some(estimate) = issue.estimate else {
        return ineligible(
            EligibilityReason::ComplexityMissing,
            "issue has no Linear estimate",
        );
    };
    let Some(complexity) = input.complexity_mapping.get(&estimate).copied() else {
        return ineligible(
            EligibilityReason::ComplexityUnmapped,
            "issue estimate is not in the configured complexity mapping",
        );
    };
    let mappings = input
        .repository_mappings
        .iter()
        .filter(|mapping| issue.labels.contains(&mapping.label))
        .collect::<Vec<_>>();
    let [mapping] = mappings.as_slice() else {
        return if mappings.is_empty() {
            ineligible(
                EligibilityReason::RepositoryUnmapped,
                "issue has no allowlisted repository mapping label",
            )
        } else {
            ineligible(
                EligibilityReason::RepositoryAmbiguous,
                "issue matches more than one repository mapping label",
            )
        };
    };
    if !mapping.enabled {
        return ineligible(
            EligibilityReason::AutomationDisabled,
            "mapped repository is disabled for automation",
        );
    }
    let blockers = issue
        .blockers
        .iter()
        .filter(|id| input.incomplete_blockers.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    if !blockers.is_empty() {
        return EligibilityResult::WaitingForDependency { blockers };
    }
    if input.locally_active {
        return ineligible(
            EligibilityReason::AlreadyActive,
            "work item already has local active ownership",
        );
    }
    if input.locally_terminal {
        return ineligible(
            EligibilityReason::LocallyTerminal,
            "work item is locally terminal",
        );
    }
    if !input.dispatch_covers_implementation_and_review {
        return ineligible(
            EligibilityReason::DispatchCoverageMissing,
            "dispatch policy does not cover both implementation and review",
        );
    }
    EligibilityResult::Eligible {
        repository: mapping.repository.clone(),
        complexity,
    }
}

fn ineligible(reason: EligibilityReason, operator_detail: &str) -> EligibilityResult {
    EligibilityResult::Ineligible {
        reason,
        operator_detail: operator_detail.to_owned(),
    }
}

pub fn dispatch_is_covered(
    policy: &spire_domain::DispatchPolicy,
    capabilities: &spire_domain::HarnessCapabilityRegistry,
    complexity: ComplexityClass,
) -> bool {
    [RunRole::Implementation, RunRole::Review]
        .into_iter()
        .all(|role| {
            policy
                .evaluate(capabilities, role, complexity, &[], None)
                .is_ok()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn issue() -> CanonicalLinearIssue {
        CanonicalLinearIssue {
            id: LinearIssueId::new("issue-1").unwrap(),
            identifier: "SPI-1".into(),
            team_id: "team".into(),
            workflow_state_id: "ready".into(),
            estimate: Some(2),
            priority: None,
            labels: ["type:bug".into(), "repo:spire".into()]
                .into_iter()
                .collect(),
            blockers: vec![],
            description: Some("untrusted".into()),
            acceptance_criteria: Some("works".into()),
            assignee_id: None,
            creator_id: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-02T00:00:00Z".into(),
            revision: CanonicalLinearIssue::revision("2026-01-02T00:00:00Z", "untrusted"),
        }
    }

    #[test]
    fn eligibility_reports_each_individual_failure_without_defaults() {
        let issue = issue();
        let types = ["type:bug".into()].into_iter().collect();
        let mappings = vec![RepositoryMapping {
            label: "repo:spire".into(),
            repository: RepositoryName::new("owner/spire").unwrap(),
            enabled: true,
        }];
        let complexity = BTreeMap::from([(2, ComplexityClass::Medium)]);
        let result = evaluate_eligibility(EligibilityInput {
            issue: &issue,
            ready_state_id: "ready",
            supported_type_labels: &types,
            repository_mappings: &mappings,
            complexity_mapping: &complexity,
            incomplete_blockers: &BTreeSet::new(),
            locally_active: false,
            locally_terminal: false,
            dispatch_covers_implementation_and_review: true,
        });
        assert!(matches!(
            result,
            EligibilityResult::Eligible {
                complexity: ComplexityClass::Medium,
                ..
            }
        ));
        let mut missing = issue.clone();
        missing.estimate = None;
        assert!(matches!(
            evaluate_eligibility(EligibilityInput {
                issue: &missing,
                ready_state_id: "ready",
                supported_type_labels: &types,
                repository_mappings: &mappings,
                complexity_mapping: &complexity,
                incomplete_blockers: &BTreeSet::new(),
                locally_active: false,
                locally_terminal: false,
                dispatch_covers_implementation_and_review: true
            }),
            EligibilityResult::Ineligible {
                reason: EligibilityReason::ComplexityMissing,
                ..
            }
        ));
    }
}
